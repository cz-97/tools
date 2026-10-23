//! 「指令匹配」：把搜索框里的输入当成一条指令来解析。
//!
//! 命中就把结果排在命令列表最前面，分两类：
//!
//! - **显示结果**：算式的值、金额大写——点一下（或回车）复制走。
//! - **执行动作**：网址、文件/文件夹路径——点一下打开。
//!
//! 哪些匹配开着，由设置页「指令匹配」标签逐项控制（见 [`MatchConfig`]）。
//! 这一层只做「从字符串到命中」的纯逻辑，不碰 UI 也不碰剪贴板。

use std::path::PathBuf;

use crate::calculator;
use crate::settings::Settings;

/// 一条命中的指令匹配。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    /// 命中的是哪种匹配，界面据此选图标。
    pub kind: MatchKind,
    /// 主行：结果（算式的值 / 金额大写 / 网址 / 路径）。
    pub title: String,
    /// 副行：这条命中是什么、点了会怎样。
    pub description: String,
    /// 点一下或回车做什么。
    pub action: MatchAction,
}

/// 命中的是哪种匹配。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchKind {
    /// 可计算的表达式。
    Calculator,
    /// 金额（最多两位小数）转中文大写。
    Amount,
    /// 网址。
    Url,
    /// 文件或文件夹路径。
    Path,
}

/// 点一下（或回车）做什么。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MatchAction {
    /// 复制一段文字：显示结果类。
    Copy(String),
    /// 交给 shell 打开：网址或路径。
    Open(String),
    /// 什么都不做（比如不存在的路径，只提示一下）。
    None,
}

/// 当前哪些匹配开着，外加算式要用的精度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatchConfig {
    pub calculator: bool,
    pub amount: bool,
    pub url: bool,
    pub path: bool,
    pub precision: u32,
}

impl Default for MatchConfig {
    fn default() -> Self {
        Self {
            calculator: true,
            amount: true,
            url: true,
            path: true,
            precision: 10,
        }
    }
}

impl MatchConfig {
    /// 从设置里取当前开着的匹配与计算精度。
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            calculator: settings.matcher_calculator,
            amount: settings.matcher_amount,
            url: settings.matcher_url,
            path: settings.matcher_path,
            precision: settings.calculator_precision,
        }
    }
}

/// 解析输入，按优先级给出所有命中的匹配。
pub fn matches(query: &str, config: &MatchConfig) -> Vec<Match> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    if config.calculator
        && let Some(found) = calculator_match(query, config.precision)
    {
        out.push(found);
    }
    if config.amount
        && let Some(found) = amount_match(query)
    {
        out.push(found);
    }
    if config.url
        && let Some(found) = url_match(query)
    {
        out.push(found);
    }
    if config.path
        && let Some(found) = path_match(query)
    {
        out.push(found);
    }
    out
}

/// 算式：含运算符、且真能算出结果。光秃秃的数字不算，免得和金额匹配撞车。
fn calculator_match(query: &str, precision: u32) -> Option<Match> {
    if !has_operator(query) {
        return None;
    }
    let result = calculator::result_text(query, precision)?;
    Some(Match {
        kind: MatchKind::Calculator,
        title: result.clone(),
        description: "计算结果 · 点一下复制".to_string(),
        action: MatchAction::Copy(result),
    })
}

/// 金额大写：最多两位小数的数字，转成中文大写。
fn amount_match(query: &str) -> Option<Match> {
    let upper = amount_uppercase(query)?;
    Some(Match {
        kind: MatchKind::Amount,
        title: upper.clone(),
        description: format!("{query} · 金额大写 · 点一下复制"),
        action: MatchAction::Copy(upper),
    })
}

/// 网址：补全协议后交给浏览器。
fn url_match(query: &str) -> Option<Match> {
    let url = normalize_url(query)?;
    Some(Match {
        kind: MatchKind::Url,
        title: url.clone(),
        description: "在浏览器打开网址".to_string(),
        action: MatchAction::Open(url),
    })
}

/// 文件/文件夹：存在就打开，不存在的绝对路径给一句提示。
fn path_match(query: &str) -> Option<Match> {
    if !looks_like_path(query) {
        return None;
    }
    let expanded = expand_home(query);
    let path = PathBuf::from(&expanded);
    if path.is_dir() {
        Some(Match {
            kind: MatchKind::Path,
            title: expanded.clone(),
            description: "打开文件夹".to_string(),
            action: MatchAction::Open(expanded),
        })
    } else if path.is_file() {
        // 文件打开它所在的文件夹，不直接启动它。
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map_or_else(|| expanded.clone(), |parent| parent.display().to_string());
        Some(Match {
            kind: MatchKind::Path,
            title: expanded,
            description: "打开所在文件夹".to_string(),
            action: MatchAction::Open(parent),
        })
    } else if is_absolute_hint(query) {
        // 只有明显是绝对路径时才报「不存在」，否则「1/2」这类会误报。
        Some(Match {
            kind: MatchKind::Path,
            title: expanded,
            description: "文件或文件夹不存在".to_string(),
            action: MatchAction::None,
        })
    } else {
        None
    }
}

/// 有没有运算符，判断是不是算式。
///
/// 全角运算符也算：`calculator::evaluate` 会先把它们归一化。
fn has_operator(query: &str) -> bool {
    query.chars().any(|c| {
        matches!(
            c,
            '+' | '*'
                | '/'
                | '^'
                | '('
                | ')'
                | '\u{FF0B}'
                | '\u{FF0A}'
                | '\u{FF0F}'
                | '\u{FF3E}'
                | '\u{FF08}'
                | '\u{FF09}'
        )
    }) || query
        .char_indices()
        .any(|(index, c)| (c == '-' || c == '\u{FF0D}') && index > 0)
}

/// 看着像不像一个文件系统路径。
fn looks_like_path(query: &str) -> bool {
    // 带协议的肯定是网址，别当路径去查。
    if query.contains("://") {
        return false;
    }
    has_drive(query) || query.starts_with("\\\\") || query.starts_with('~') || {
        query.contains('\\') || query.contains('/')
    }
}

/// 是不是「明显是绝对路径」——报「不存在」时只认这种，避免相对路径误报。
fn is_absolute_hint(query: &str) -> bool {
    has_drive(query)
        || query.starts_with("\\\\")
        || query.starts_with('~')
        || query.starts_with('/')
        || query.starts_with('\\')
}

/// `C:` 这种盘符开头。
fn has_drive(query: &str) -> bool {
    let bytes = query.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

/// 把开头的 `~` 换成用户主目录。
fn expand_home(query: &str) -> String {
    if let Some(rest) = query.strip_prefix('~')
        && let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))
    {
        let rest = rest.trim_start_matches(['/', '\\']);
        let mut path = PathBuf::from(home);
        if !rest.is_empty() {
            path.push(rest);
        }
        return path.display().to_string();
    }
    query.to_string()
}

/// 把可能是网址的输入补全成可打开的 URL。
fn normalize_url(query: &str) -> Option<String> {
    if query.contains(char::is_whitespace) {
        return None;
    }
    let lower = query.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        let rest = &query[query.find("://")? + 3..];
        return (!rest.is_empty()).then(|| query.to_string());
    }
    if is_localhost(query) {
        return Some(format!("http://{query}"));
    }
    looks_like_domain(query).then(|| format!("https://{query}"))
}

/// `localhost` / `127.0.0.1`，可带端口。
fn is_localhost(query: &str) -> bool {
    let host = host_of(query).to_ascii_lowercase();
    host == "localhost" || host == "127.0.0.1"
}

/// 取出主机名（去掉路径、查询和端口）。
fn host_of(query: &str) -> &str {
    let host_port = query.split(['/', '?', '#']).next().unwrap_or(query);
    host_port.split(':').next().unwrap_or(host_port)
}

/// 像不像一个带公网后缀的域名。
fn looks_like_domain(query: &str) -> bool {
    let host = host_of(query);
    if host.is_empty() {
        return false;
    }
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 {
        return false;
    }
    if !labels.iter().all(|label| {
        !label.is_empty()
            && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
            && !label.starts_with('-')
            && !label.ends_with('-')
    }) {
        return false;
    }
    let tld = labels
        .last()
        .copied()
        .unwrap_or_default()
        .to_ascii_lowercase();
    KNOWN_TLDS.contains(&tld.as_str())
}

/// 认得出来的常见后缀；不在表里的不当网址，免得把 `config.toml` 这类文件误判。
const KNOWN_TLDS: &[&str] = &[
    "com", "cn", "net", "org", "io", "dev", "app", "co", "me", "info", "edu", "gov", "mil", "xyz",
    "top", "site", "online", "shop", "vip", "cc", "tv", "ai", "tech", "cloud", "page", "link",
    "wang", "run", "sh", "gg", "uk", "us", "jp", "kr", "de", "fr", "ru", "hk", "tw", "mo",
];

/// 把金额（最多两位小数）转成中文大写，如 `123.45` → `壹佰贰拾叁元肆角伍分`。
///
/// 不是纯数字、超过两位小数、或者整数位太长（超过万亿级）就返回 `None`。
pub fn amount_uppercase(text: &str) -> Option<String> {
    let trimmed = text.trim();
    let digits = trimmed
        .strip_prefix('¥')
        .or_else(|| trimmed.strip_prefix('￥'))
        .unwrap_or(trimmed);

    let (integer, fraction) = match digits.split_once('.') {
        Some((integer, fraction)) => (integer, Some(fraction)),
        None => (digits, None),
    };

    if !integer.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if let Some(fraction) = fraction
        && (fraction.len() > 2 || !fraction.chars().all(|c| c.is_ascii_digit()))
    {
        return None;
    }

    let integer = if integer.is_empty() { "0" } else { integer };
    let trimmed_integer = integer.trim_start_matches('0');
    let integer = if trimmed_integer.is_empty() {
        "0"
    } else {
        trimmed_integer
    };
    if integer.len() > 16 {
        return None;
    }

    let jiao = fraction
        .and_then(|fraction| fraction.chars().next())
        .and_then(|c| c.to_digit(10))
        .unwrap_or(0);
    let fen = fraction
        .and_then(|fraction| fraction.chars().nth(1))
        .and_then(|c| c.to_digit(10))
        .unwrap_or(0);

    Some(format!(
        "{}元{}",
        integer_upper(integer),
        decimal_upper(jiao, fen)
    ))
}

const DIGITS: [&str; 10] = ["零", "壹", "贰", "叁", "肆", "伍", "陆", "柒", "捌", "玖"];
const SMALL_UNITS: [&str; 4] = ["", "拾", "佰", "仟"];
const BIG_UNITS: [&str; 4] = ["", "万", "亿", "万亿"];

/// 整数部分转大写：每四位一组，组内按「仟佰拾个」拼，组间补「万/亿/万亿」。
fn integer_upper(integer: &str) -> String {
    let digits: Vec<u32> = integer
        .chars()
        .map(|c| c.to_digit(10).unwrap_or(0))
        .collect();
    let len = digits.len();
    let mut out = String::new();
    let mut zero_pending = false;

    for (index, &digit) in digits.iter().enumerate() {
        let position = len - 1 - index;
        let group = position / 4;
        let within = position % 4;

        if digit == 0 {
            zero_pending = true;
        } else {
            if zero_pending {
                out.push_str("零");
                zero_pending = false;
            }
            out.push_str(DIGITS[digit as usize]);
            out.push_str(SMALL_UNITS[within]);
        }

        if within == 0 && group > 0 && group_nonzero(&digits, group) {
            out.push_str(BIG_UNITS[group.min(BIG_UNITS.len() - 1)]);
        }
    }

    if out.is_empty() {
        "零".to_string()
    } else {
        out
    }
}

/// 这一组（从低位起第 `group` 个四位）里有没有非零数字。
fn group_nonzero(digits: &[u32], group: usize) -> bool {
    let len = digits.len();
    let high = len.saturating_sub(group * 4);
    let low = high.saturating_sub(4);
    digits
        .get(low..high)
        .is_some_and(|slice| slice.iter().any(|&digit| digit != 0))
}

/// 小数部分（角、分）转大写。
fn decimal_upper(jiao: u32, fen: u32) -> String {
    if jiao == 0 && fen == 0 {
        return "整".to_string();
    }
    let mut out = String::new();
    if jiao != 0 {
        out.push_str(DIGITS[jiao as usize]);
        out.push_str("角");
    }
    if fen != 0 {
        if jiao == 0 {
            out.push_str("零");
        }
        out.push_str(DIGITS[fen as usize]);
        out.push_str("分");
    } else {
        out.push_str("整");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{MatchAction, MatchConfig, MatchKind, amount_uppercase, matches};
    use std::path::Path;

    fn kinds(query: &str) -> Vec<MatchKind> {
        matches(query, &MatchConfig::default())
            .into_iter()
            .map(|found| found.kind)
            .collect()
    }

    #[test]
    fn amounts_become_chinese_uppercase() {
        assert_eq!(
            amount_uppercase("123.45").as_deref(),
            Some("壹佰贰拾叁元肆角伍分")
        );
        assert_eq!(amount_uppercase("123").as_deref(), Some("壹佰贰拾叁元整"));
        assert_eq!(
            amount_uppercase("123.4").as_deref(),
            Some("壹佰贰拾叁元肆角整")
        );
        assert_eq!(
            amount_uppercase("123.05").as_deref(),
            Some("壹佰贰拾叁元零伍分")
        );
        assert_eq!(amount_uppercase("0.5").as_deref(), Some("零元伍角整"));
        assert_eq!(amount_uppercase("0.05").as_deref(), Some("零元零伍分"));
        assert_eq!(amount_uppercase("0").as_deref(), Some("零元整"));
        assert_eq!(
            amount_uppercase("100000001").as_deref(),
            Some("壹亿零壹元整")
        );
        assert_eq!(amount_uppercase("10001").as_deref(), Some("壹万零壹元整"));
        assert_eq!(amount_uppercase("¥10000").as_deref(), Some("壹万元整"));
    }

    #[test]
    fn more_than_two_decimals_is_not_an_amount() {
        assert_eq!(amount_uppercase("1.234"), None);
        assert_eq!(amount_uppercase("1.2.3"), None);
        assert_eq!(amount_uppercase("12,345"), None);
        assert_eq!(amount_uppercase("abc"), None);
        assert_eq!(amount_uppercase("-5"), None);
    }

    #[test]
    fn a_plain_number_is_an_amount_not_a_calculation() {
        assert_eq!(kinds("123.45"), [MatchKind::Amount]);
    }

    #[test]
    fn an_expression_shows_its_result() {
        let found = matches("1+2", &MatchConfig::default());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, MatchKind::Calculator);
        assert_eq!(found[0].title, "3");
        assert!(matches!(found[0].action, MatchAction::Copy(_)));
    }

    #[test]
    fn a_fraction_expression_is_not_mistaken_for_a_path() {
        // 1/2 是算式，不该再冒出「路径不存在」。
        assert_eq!(kinds("1/2"), [MatchKind::Calculator]);
    }

    #[test]
    fn urls_are_completed_and_opened() {
        let found = matches("github.com", &MatchConfig::default());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, MatchKind::Url);
        assert_eq!(found[0].title, "https://github.com");

        let found = matches("https://example.com/a", &MatchConfig::default());
        assert_eq!(found[0].kind, MatchKind::Url);
        assert_eq!(found[0].title, "https://example.com/a");
    }

    #[test]
    fn a_plain_file_name_is_not_a_url() {
        // config.toml 的后缀不在常见 TLD 里，别当网址。
        assert!(kinds("config.toml").is_empty());
    }

    #[test]
    fn an_existing_folder_opens_as_a_folder() {
        let temp = std::env::temp_dir();
        let found = matches(&temp.display().to_string(), &MatchConfig::default());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, MatchKind::Path);
        assert_eq!(found[0].description, "打开文件夹");
        assert!(matches!(found[0].action, MatchAction::Open(_)));
    }

    #[test]
    fn an_existing_file_opens_its_folder() {
        let exe = std::env::current_exe().expect("测试程序自己的路径");
        let found = matches(&exe.display().to_string(), &MatchConfig::default());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, MatchKind::Path);
        assert_eq!(found[0].description, "打开所在文件夹");
    }

    #[test]
    fn a_missing_absolute_path_is_reported() {
        let bogus = if cfg!(windows) {
            r"Z:\definitely-missing-xyz"
        } else {
            "/definitely-missing-xyz"
        };
        let found = matches(bogus, &MatchConfig::default());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].description, "文件或文件夹不存在");
        assert_eq!(found[0].action, MatchAction::None);
        assert!(!Path::new(bogus).exists());
    }

    #[test]
    fn disabled_matchers_stay_quiet() {
        let config = MatchConfig {
            calculator: false,
            ..MatchConfig::default()
        };
        assert!(matches("1+2", &config).is_empty());
    }
}
