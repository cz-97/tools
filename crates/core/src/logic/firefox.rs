//! 火狐系浏览器的书签与历史：直接从 profile 里的 `places.sqlite` 读。
//!
//! 「火狐系」不止 Firefox 一个：LibreWolf、Waterfox、Floorp、Zen 都用同一套
//! `profiles.ini` + `places.sqlite`，所以这里按产品目录逐个找，找到哪个用哪个；
//! 用户也可以直接指定 profile 目录（见设置 → 工具 → 火狐系浏览器）。
//!
//! 数据库是浏览器自己开着的，还带着 WAL，所以**先只读打开**；只读打不开（临时文件被清掉、
//! 或者库正被独占）就把几个文件复制到临时目录再读，读完删掉。绝不写用户的原库。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rusqlite::{Connection, OpenFlags, params};

/// 历史最多读这么多条：再往后翻也没人会按到。
pub const HISTORY_LIMIT: usize = 500;

/// `%APPDATA%` 下面可能出现的火狐系产品目录，顺序就是自动探测的顺序。
const PRODUCTS: [&str; 6] = [
    "Mozilla/Firefox",
    "zen",
    "LibreWolf",
    "Waterfox",
    "Floorp",
    "Mozilla/SeaMonkey",
];

/// 一张站点图标。
///
/// `favicons.sqlite` 里没有 MIME 字段，格式只能从文件头嗅出来。
#[derive(Clone)]
pub struct Favicon {
    pub bytes: Vec<u8>,
    pub kind: IconKind,
}

/// 嗅出来的图标格式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconKind {
    Png,
    Jpeg,
    Gif,
    Webp,
    Bmp,
    Ico,
    /// 有些站点直接把 SVG 存进来（Zen 里就有）。
    Svg,
}

/// 一条书签。
pub struct Bookmark {
    /// 书签自己的名字，没改名时是页面标题。
    pub title: String,
    /// 网页标题，和书签名字不一样时才值得显示。
    pub page_title: String,
    pub url: String,
    pub icon: Option<Arc<Favicon>>,
}

/// 一条历史记录。
pub struct Visit {
    pub title: String,
    pub url: String,
    /// 访问次数。
    pub visits: u64,
    /// 最后访问时间，写成「几分钟前」这种好读的样子。
    pub visited: String,
    pub icon: Option<Arc<Favicon>>,
}

/// 用户指定的目录是不是一个 profile、或者装着 profile 的根目录。
///
/// 两种都认：直接指到 profile（里面有 `places.sqlite`），或者指到 `profiles.ini` 那一层。
pub fn resolve_profile(configured: Option<&str>) -> Option<PathBuf> {
    match configured.map(str::trim).filter(|path| !path.is_empty()) {
        Some(path) => profile_in(Path::new(path)),
        None => detect_profile(),
    }
}

/// 按产品目录自动找一个能用的 profile。
pub fn detect_profile() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    let appdata = PathBuf::from(appdata);
    PRODUCTS
        .iter()
        .filter_map(|product| profile_in(&appdata.join(product)))
        .next()
}

/// 在一个目录里找 profile：先是它自己，再按 `profiles.ini` 找。
fn profile_in(root: &Path) -> Option<PathBuf> {
    if root.join("places.sqlite").is_file() {
        return Some(root.to_path_buf());
    }
    let ini = std::fs::read_to_string(root.join("profiles.ini")).ok()?;
    if let Some(relative) = profile_from_ini(&ini) {
        let path = if relative.is_absolute() {
            relative
        } else {
            root.join(relative)
        };
        if path.join("places.sqlite").is_file() {
            return Some(path);
        }
    }
    // `profiles.ini` 里写的路径已经失效时，退一步用最近改过的那个 profile。
    newest_profile(&root.join("Profiles"))
}

/// `Profiles/` 下面最近动过的那个 profile。
fn newest_profile(profiles: &Path) -> Option<PathBuf> {
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(profiles).ok()?.flatten() {
        let path = entry.path();
        let places = path.join("places.sqlite");
        if !places.is_file() {
            continue;
        }
        let Ok(modified) = places.metadata().and_then(|meta| meta.modified()) else {
            continue;
        };
        if best.as_ref().is_none_or(|(seen, _)| modified > *seen) {
            best = Some((modified, path));
        }
    }
    best.map(|(_, path)| path)
}

/// 从 `profiles.ini` 里挑出浏览器实际在用的 profile 路径。
///
/// 优先 `[Install…]` 段的 `Default=`（那才是这次安装真正打开的 profile），
/// 其次 `[Profile…]` 段里 `Default=1` 的那一条。
pub fn profile_from_ini(text: &str) -> Option<PathBuf> {
    let mut installed: Option<String> = None;
    let mut marked: Option<String> = None;
    let mut section = String::new();
    let mut fields: Vec<(String, String)> = Vec::new();

    let mut flush = |section: &str, fields: &mut Vec<(String, String)>| {
        let value = |name: &str| {
            fields
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(name))
                .map(|(_, value)| value.trim().to_string())
                .filter(|value| !value.is_empty())
        };
        if section.starts_with("Install") {
            if let Some(path) = value("Default") {
                installed = Some(path);
            }
        } else if section.starts_with("Profile") {
            // `Default=1` 表示「默认 profile」，`Default=0` 只是占位。
            let is_default = fields
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case("Default"))
                .is_some_and(|(_, value)| value.trim() == "1");
            if is_default && let Some(path) = value("Path") {
                marked = Some(path);
            }
        }
        fields.clear();
    };

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            flush(&section, &mut fields);
            section = name.trim().to_string();
        } else if let Some((key, value)) = line.split_once('=') {
            fields.push((key.trim().to_string(), value.trim().to_string()));
        }
    }
    flush(&section, &mut fields);

    installed.or(marked).map(PathBuf::from)
}

/// 读这个 profile 的所有书签，最近改过的排前面。
pub fn bookmarks(profile: &Path) -> Result<Vec<Bookmark>, String> {
    with_places(profile, |connection, icons| {
        let table = icons.then(|| load_icons(connection));
        let sql = format!(
            "SELECT title, url, page_title, icon_id FROM (
                 SELECT b.id AS bid,
                        COALESCE(NULLIF(b.title, ''), p.title, p.url) AS title,
                        p.url AS url,
                        COALESCE(p.title, '') AS page_title,
                        {icon_id} AS icon_id,
                        {icon_width} AS icon_width,
                        b.lastModified AS ts,
                        ROW_NUMBER() OVER (
                            PARTITION BY b.id
                            ORDER BY COALESCE({icon_width}, 0) DESC, {icon_id} DESC
                        ) AS rn
                 FROM moz_bookmarks b
                 JOIN moz_places p ON b.fk = p.id
                 {icon_join}
                 WHERE b.type = 1 AND p.url IS NOT NULL AND p.url <> ''
             ) WHERE rn = 1
             ORDER BY ts DESC",
            icon_width = icon_column(icons, "i.width"),
            icon_id = icon_column(icons, "i.id"),
            icon_join = icon_join(icons),
        );
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map([], |row| {
            let url: String = row.get(1)?;
            let icon_id: Option<i64> = row.get(3)?;
            Ok(Bookmark {
                title: row.get(0)?,
                page_title: row.get(2)?,
                icon: table.as_ref().and_then(|table| table.pick(icon_id, &url)),
                url,
            })
        })?;
        rows.collect()
    })
}

/// 读历史，最近访问的排前面，最多 [`HISTORY_LIMIT`] 条。
pub fn history(profile: &Path) -> Result<Vec<Visit>, String> {
    let now = unix_now();
    with_places(profile, move |connection, icons| {
        let table = icons.then(|| load_icons(connection));
        let sql = format!(
            "SELECT title, url, visits, visited, icon_id FROM (
                 SELECT p.id AS pid,
                        COALESCE(NULLIF(p.title, ''), p.url) AS title,
                        p.url AS url,
                        p.visit_count AS visits,
                        p.last_visit_date AS visited,
                        {icon_id} AS icon_id,
                        {icon_width} AS icon_width,
                        ROW_NUMBER() OVER (
                            PARTITION BY p.id
                            ORDER BY COALESCE({icon_width}, 0) DESC, {icon_id} DESC
                        ) AS rn
                 FROM moz_places p
                 {icon_join}
                 WHERE p.visit_count > 0 AND p.title IS NOT NULL AND p.hidden = 0
             ) WHERE rn = 1
             ORDER BY visited DESC
             LIMIT ?1",
            icon_width = icon_column(icons, "i.width"),
            icon_id = icon_column(icons, "i.id"),
            icon_join = icon_join(icons),
        );
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(params![HISTORY_LIMIT as i64], |row| {
            let micros: Option<i64> = row.get(3)?;
            let url: String = row.get(1)?;
            let icon_id: Option<i64> = row.get(4)?;
            Ok(Visit {
                title: row.get(0)?,
                visits: row.get::<_, i64>(2)?.max(0) as u64,
                visited: micros.map_or_else(String::new, |micros| visited_text(micros, now)),
                icon: table.as_ref().and_then(|table| table.pick(icon_id, &url)),
                url,
            })
        })?;
        rows.collect()
    })
}

/// 有没有图标，SQL 长得不一样：没有 `favicons.sqlite` 时不去 join 它。
fn icon_join(icons: bool) -> &'static str {
    if icons {
        "LEFT JOIN favicons.moz_pages_w_icons pw ON pw.page_url_hash = p.url_hash
         LEFT JOIN favicons.moz_icons_to_pages ip ON ip.page_id = pw.id
         LEFT JOIN favicons.moz_icons i ON i.id = ip.icon_id"
    } else {
        ""
    }
}

/// 列名：没有图标表时补一个 `NULL`，两种 SQL 的输出列数才一样。
fn icon_column(icons: bool, column: &str) -> &str {
    if icons { column } else { "NULL" }
}

/// `favicons.sqlite` 里的全部图标，按 id 和按域名各索引一份。
///
/// 为什么要有域名这一份：书签和历史页只靠 `moz_pages_w_icons` 的关联，很多站点压根
/// 没有那条记录，图标就全是空的。改成整表读一遍、再按页面域名兜底，覆盖率才够。
#[derive(Default)]
struct IconTable {
    by_id: HashMap<i64, (i64, Arc<Favicon>)>,
    by_domain: HashMap<String, (i64, Arc<Favicon>)>,
}

impl IconTable {
    /// 先按 `icon_id` 找，找不到就按页面域名找。
    fn pick(&self, icon_id: Option<i64>, page_url: &str) -> Option<Arc<Favicon>> {
        if let Some((_, icon)) = icon_id.and_then(|id| self.by_id.get(&id)) {
            return Some(icon.clone());
        }
        let host = host_of(page_url)?;
        self.by_domain.get(&host).map(|(_, icon)| icon.clone())
    }

    /// 同一个键只留更清楚的那张（`width` 大的；SVG 存的是 65535，天然最优先）。
    fn remember(
        slot: &mut HashMap<i64, (i64, Arc<Favicon>)>,
        key: i64,
        width: i64,
        icon: &Arc<Favicon>,
    ) {
        if slot.get(&key).is_none_or(|(seen, _)| width > *seen) {
            slot.insert(key, (width, icon.clone()));
        }
    }
}

/// 把 `moz_icons` 整表读成两张索引。
fn load_icons(connection: &Connection) -> IconTable {
    let mut by_id: HashMap<i64, (i64, Arc<Favicon>)> = HashMap::new();
    let mut by_domain: HashMap<String, (i64, Arc<Favicon>)> = HashMap::new();

    let Ok(mut statement) =
        connection.prepare("SELECT id, width, icon_url, data FROM favicons.moz_icons")
    else {
        return IconTable::default();
    };
    let Ok(mut rows) = statement.query([]) else {
        return IconTable::default();
    };
    while let Ok(Some(row)) = rows.next() {
        let Some(bytes) = row.get::<_, Option<Vec<u8>>>(3).unwrap_or(None) else {
            continue;
        };
        let Some(kind) = sniff(&bytes) else {
            continue;
        };
        let id: i64 = row.get(0).unwrap_or_default();
        let width: i64 = row.get(1).unwrap_or_default();
        let icon_url: Option<String> = row.get(2).unwrap_or(None);
        let icon = Arc::new(Favicon { bytes, kind });

        IconTable::remember(&mut by_id, id, width, &icon);
        if let Some(host) = icon_url.as_deref().and_then(host_of)
            && by_domain.get(&host).is_none_or(|(seen, _)| width > *seen)
        {
            by_domain.insert(host, (width, icon));
        }
    }

    IconTable { by_id, by_domain }
}

/// 从网址里取域名：小写、去端口、去 `www.`。取不到就是 `None`。
fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let host = host.split(':').next()?.trim_end_matches('.');
    if host.is_empty() {
        return None;
    }
    let host = host.to_ascii_lowercase();
    Some(host.strip_prefix("www.").unwrap_or(&host).to_string())
}

/// 按文件头判断图标格式；认不出的直接放弃（宁可不画图标，也不画一个裂图）。
fn sniff(bytes: &[u8]) -> Option<IconKind> {
    if bytes.len() < 4 {
        return None;
    }
    let head = &bytes[..4];
    match head {
        [0x89, b'P', b'N', b'G'] => return Some(IconKind::Png),
        [0xff, 0xd8, 0xff, _] => return Some(IconKind::Jpeg),
        [b'G', b'I', b'F', b'8'] => return Some(IconKind::Gif),
        [b'B', b'M', _, _] => return Some(IconKind::Bmp),
        [b'R', b'I', b'F', b'F'] if bytes.get(8..12) == Some(b"WEBP") => Some(IconKind::Webp),
        [0, 0, 1, 0] | [0, 0, 2, 0] => Some(IconKind::Ico),
        _ => {
            let text = String::from_utf8_lossy(&bytes[..bytes.len().min(256)]);
            text.trim_start()
                .starts_with("<svg")
                .then_some(IconKind::Svg)
        }
    }
}

/// 打开 profile 的 `places.sqlite` 读一次；`icons` 说明 `favicons.sqlite` 挂上了没有。
///
/// 先只读直连；不行就把文件复制到临时目录再读。用户的库全程只读。
fn with_places<T>(
    profile: &Path,
    action: impl FnOnce(&Connection, bool) -> rusqlite::Result<T>,
) -> Result<T, String> {
    let places = profile.join("places.sqlite");
    if !places.is_file() {
        return Err(format!("{} 里没有 places.sqlite", profile.display()));
    }
    let favicons = profile.join("favicons.sqlite");
    let has_icons = favicons.is_file();

    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI;
    if let Ok(connection) = Connection::open_with_flags(&places, flags) {
        let _ = connection.busy_timeout(std::time::Duration::from_millis(500));
        let attached = has_icons
            && connection
                .execute(
                    "ATTACH DATABASE ?1 AS favicons",
                    params![path_text(&favicons)],
                )
                .is_ok();
        return action(&connection, attached).map_err(|error| error.to_string());
    }

    // 只读打不开（WAL 的 `-shm` 没了、或者库正被独占）：复制一份再读。
    let copy = std::env::temp_dir().join(format!("tools-places-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&copy);
    std::fs::create_dir_all(&copy).map_err(|error| format!("建临时目录失败：{error}"))?;
    for name in [
        "places.sqlite",
        "places.sqlite-wal",
        "places.sqlite-shm",
        "favicons.sqlite",
        "favicons.sqlite-wal",
        "favicons.sqlite-shm",
    ] {
        let source = profile.join(name);
        if source.is_file() {
            let _ = std::fs::copy(&source, copy.join(name));
        }
    }
    let result = Connection::open(copy.join("places.sqlite")).map_err(|error| {
        let _ = std::fs::remove_dir_all(&copy);
        format!("打不开 places.sqlite：{error}")
    })?;
    let _ = result.busy_timeout(std::time::Duration::from_millis(500));
    let attached = has_icons
        && result
            .execute(
                "ATTACH DATABASE ?1 AS favicons",
                params![path_text(&copy.join("favicons.sqlite"))],
            )
            .is_ok();
    let outcome = action(&result, attached).map_err(|error| error.to_string());
    drop(result);
    let _ = std::fs::remove_dir_all(&copy);
    outcome
}

/// rusqlite 认的参数形式。
fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// 现在这一刻，单位秒。
fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}

/// 把 `last_visit_date`（微秒）写成「几分钟前」这种好读的样子。
fn visited_text(micros: i64, now: i64) -> String {
    let at = micros / 1_000_000;
    let ago = (now - at).max(0);
    match ago {
        0..=59 => "刚刚".to_string(),
        60..=3599 => format!("{} 分钟前", ago / 60),
        3600..=86_399 => format!("{} 小时前", ago / 3600),
        86_400..=2_591_999 => format!("{} 天前", ago / 86_400),
        _ => local_text(at),
    }
}

/// 绝对时间，按本地时区写成 `2026-10-23 18:21`。
fn local_text(unix_seconds: i64) -> String {
    let Some((year, month, day, hour, minute)) = local_parts(unix_seconds) else {
        return String::new();
    };
    format!("{year}-{month:02}-{day:02} {hour:02}:{minute:02}")
}

#[cfg(windows)]
fn local_parts(unix_seconds: i64) -> Option<(i32, u32, u32, u32, u32)> {
    use windows_sys::Win32::Foundation::SYSTEMTIME;
    use windows_sys::Win32::System::Time::SystemTimeToTzSpecificLocalTime;

    // 先把 UTC 拆成年月日时分，再让系统按时区（含夏令时）翻成本地时间。
    let days = unix_seconds.div_euclid(86_400);
    let rest = unix_seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let utc = SYSTEMTIME {
        wYear: year as u16,
        wMonth: month as u16,
        wDayOfWeek: 0,
        wDay: day as u16,
        wHour: (rest / 3600) as u16,
        wMinute: ((rest % 3600) / 60) as u16,
        wSecond: (rest % 60) as u16,
        wMilliseconds: 0,
    };
    let mut local = SYSTEMTIME {
        wYear: 0,
        wMonth: 0,
        wDayOfWeek: 0,
        wDay: 0,
        wHour: 0,
        wMinute: 0,
        wSecond: 0,
        wMilliseconds: 0,
    };
    // SAFETY: 两个参数都是本栈上的 SYSTEMTIME；第一个传空表示用当前时区。
    let ok = unsafe { SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut local) };
    (ok != 0).then_some((
        local.wYear as i32,
        local.wMonth as u32,
        local.wDay as u32,
        local.wHour as u32,
        local.wMinute as u32,
    ))
}

#[cfg(not(windows))]
fn local_parts(unix_seconds: i64) -> Option<(i32, u32, u32, u32, u32)> {
    let days = unix_seconds.div_euclid(86_400);
    let rest = unix_seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    Some((
        year,
        month,
        day,
        (rest / 3600) as u32,
        ((rest % 3600) / 60) as u32,
    ))
}

/// 1970 年起的第 `days` 天是公历哪一天（Howard Hinnant 的算法，换算不留误差）。
fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted + 2) / 5 + 1) as u32;
    let month = if shifted < 10 {
        shifted + 3
    } else {
        shifted - 9
    } as u32;
    let year = if month <= 2 { year + 1 } else { year };
    (year as i32, month, day)
}

#[cfg(test)]
mod tests {
    use super::{IconKind, civil_from_days, profile_from_ini, sniff, visited_text};

    #[test]
    fn a_host_is_read_out_of_a_url() {
        use super::host_of;
        assert_eq!(
            host_of("https://www.Massgrave.dev/x?y=1"),
            Some("massgrave.dev".to_string())
        );
        assert_eq!(
            host_of("https://192.168.100.251:8443/web/frame"),
            Some("192.168.100.251".to_string())
        );
        assert_eq!(
            host_of("http://user:pw@example.com/"),
            Some("example.com".to_string())
        );
        assert_eq!(host_of("about:blank"), None);
    }

    #[test]
    fn the_install_section_wins_over_a_marked_profile() {
        let ini = "\
[InstallF0DC299D809B9700]
Default=Profiles/blah4vst.Default (release)
Locked=1

[Profile1]
Name=Default Profile
IsRelative=1
Path=Profiles/nns1uwaz.Default Profile
Default=1
";
        assert_eq!(
            profile_from_ini(ini).expect("有 profile").to_string_lossy(),
            "Profiles/blah4vst.Default (release)"
        );
    }

    #[test]
    fn a_marked_profile_is_used_when_no_install_section_exists() {
        let ini = "\
[Profile0]
Name=default
IsRelative=1
Path=Profiles/abc.default
Default=1

[Profile1]
Name=other
IsRelative=1
Path=Profiles/def.other
";
        assert_eq!(
            profile_from_ini(ini).expect("有 profile").to_string_lossy(),
            "Profiles/abc.default"
        );
    }

    #[test]
    fn an_ini_without_any_profile_has_nothing_to_offer() {
        assert!(profile_from_ini("[General]\nVersion=2\n").is_none());
    }

    #[test]
    fn icons_are_recognised_by_their_first_bytes() {
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\n"), Some(IconKind::Png));
        assert_eq!(sniff(b"\xff\xd8\xff\xe0"), Some(IconKind::Jpeg));
        assert_eq!(sniff(b"GIF89a"), Some(IconKind::Gif));
        assert_eq!(sniff(b"\x00\x00\x01\x00\x10"), Some(IconKind::Ico));
        assert_eq!(
            sniff(b"  <svg xmlns=\"http://www.w3.org/2000/svg\">"),
            Some(IconKind::Svg)
        );
        assert_eq!(sniff(b"\x00\x01\x02\x03"), None);
    }

    #[test]
    fn a_visit_just_now_reads_as_just_now() {
        let now = 1_000_000_000;
        assert_eq!(visited_text(now * 1_000_000, now), "刚刚");
        assert_eq!(visited_text((now - 120) * 1_000_000, now), "2 分钟前");
        assert_eq!(visited_text((now - 7200) * 1_000_000, now), "2 小时前");
        assert_eq!(visited_text((now - 172_800) * 1_000_000, now), "2 天前");
    }

    #[test]
    fn the_civil_calendar_starts_at_the_epoch() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_726), (2026, 9, 30));
    }
}
