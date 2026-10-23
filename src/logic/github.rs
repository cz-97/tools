//! GitHub 仓库查询：个人项目、星标、按关键词搜。
//!
//! 三个接口都走 `api.github.com`，都只读：设置里填了令牌就带上（额度从 60 次/小时
//! 提到 5000 次/小时），没填也能用，只是容易撞限流。
//!
//! HTTP 走 `platform/http.rs`（WinHTTP），是**阻塞**的：调用方要放到后台线程里跑
//! （`cx.background_executor().spawn`），不要在 UI 线程上直接调。

use serde_json::{Value, json};

use crate::http;
/// 每页取多少条（GitHub 上限就是 100）。
const PER_PAGE: usize = 100;
/// 星标最多翻这么多页，也就是 1000 个仓库。
const STARS_PAGES: usize = 10;
/// 个人项目最多翻这么多页。
const PROJECT_PAGES: usize = 3;
/// 搜索只取第一页，按星数降序。
const SEARCH_LIMIT: usize = 50;

/// 一个仓库，只留列表里要显示的那几项。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repo {
    /// `owner/name`。
    pub full_name: String,
    pub description: String,
    pub url: String,
    /// 主语言，没识别出来时是空的。
    pub language: String,
    pub stars: u64,
}

/// 某个用户的个人项目（他自己名下的仓库），按最近更新排。
pub fn owned_repos(user: &str, token: &str) -> Result<Vec<Repo>, String> {
    let user = user.trim();
    if user.is_empty() {
        return Err("先在设置里填 GitHub 用户名".to_string());
    }
    let base = format!(
        "https://api.github.com/users/{}/repos?per_page={PER_PAGE}&sort=updated&type=owner",
        encode(user)
    );
    let mut repos = Vec::new();
    for page in 1..=PROJECT_PAGES {
        let url = format!("{base}&page={page}");
        let items = get_array(&url, token)?;
        let count = items.len();
        repos.extend(items.iter().filter_map(repo_from_json));
        if count < PER_PAGE {
            break;
        }
    }
    Ok(repos)
}

/// 某个用户的星标仓库。
pub fn starred(user: &str, token: &str) -> Result<Vec<Repo>, String> {
    let user = user.trim();
    if user.is_empty() {
        return Err("先在设置里填 GitHub 用户名".to_string());
    }
    let base = format!(
        "https://api.github.com/users/{}/starred?per_page={PER_PAGE}",
        encode(user)
    );
    let mut repos = Vec::new();
    for page in 1..=STARS_PAGES {
        let url = format!("{base}&page={page}");
        let items = get_array(&url, token)?;
        let count = items.len();
        repos.extend(items.iter().filter_map(repo_from_json));
        if count < PER_PAGE {
            break;
        }
    }
    Ok(repos)
}

/// 按关键词搜仓库，按星数降序取前 [`SEARCH_LIMIT`] 个。
pub fn search(keyword: &str, token: &str) -> Result<Vec<Repo>, String> {
    let keyword = keyword.trim();
    if keyword.is_empty() {
        return Err("先输入要搜的关键词".to_string());
    }
    let url = format!(
        "https://api.github.com/search/repositories?q={}&sort=stars&order=desc&per_page={SEARCH_LIMIT}",
        encode(keyword)
    );
    let value = get_json(&url, token)?;
    let items = value
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| "GitHub 返回的内容看不懂".to_string())?;
    Ok(items.iter().filter_map(repo_from_json).collect())
}

/// 把一批仓库写成缓存里的 JSON。
pub fn to_json(repos: &[Repo]) -> String {
    let items = repos
        .iter()
        .map(|repo| {
            json!({
                "full_name": repo.full_name,
                "description": repo.description,
                "url": repo.url,
                "language": repo.language,
                "stars": repo.stars,
            })
        })
        .collect::<Vec<_>>();
    json!({ "updated": now(), "repos": items }).to_string()
}

/// 从缓存里读回来；缓存坏了就当没有。
pub fn from_json(text: &str) -> Option<Vec<Repo>> {
    let value: Value = serde_json::from_str(text).ok()?;
    let items = value.get("repos")?.as_array()?;
    Some(items.iter().filter_map(repo_from_json).collect())
}

/// 缓存里的时间戳，用来说明「这是什么时候抓的」。
pub fn cached_at(text: &str) -> Option<i64> {
    serde_json::from_str::<Value>(text)
        .ok()?
        .get("updated")?
        .as_i64()
}

/// 发一个 GET，把整段响应体当 JSON 拿回来。
fn get_json(url: &str, token: &str) -> Result<Value, String> {
    let mut headers = vec![
        ("Accept", "application/vnd.github+json".to_string()),
        ("X-GitHub-Api-Version", "2022-11-28".to_string()),
        ("User-Agent", "gpui-tools".to_string()),
    ];
    if !token.trim().is_empty() {
        headers.push(("Authorization", format!("Bearer {}", token.trim())));
    }
    let response = http::get(url, &headers)?;
    let status = response.status;

    match status {
        200..=299 => {}
        404 => return Err("GitHub 上没有这个人或这个仓库".to_string()),
        403 | 429 => return Err("GitHub 接口限流了，等一会儿或填个令牌".to_string()),
        422 => return Err("这个关键词 GitHub 不认".to_string()),
        _ => return Err(format!("GitHub 返回 {status}")),
    }
    serde_json::from_slice(&response.body)
        .map_err(|error| format!("GitHub 的回复解析不了：{error}"))
}

/// 发一个 GET，要求响应是一个数组。
fn get_array(url: &str, token: &str) -> Result<Vec<Value>, String> {
    let value = get_json(url, token)?;
    match value {
        Value::Array(items) => Ok(items),
        _ => Err("GitHub 返回的内容看不懂".to_string()),
    }
}

/// 一个仓库 JSON 到 [`Repo`]；缺关键字段的直接跳过。
///
/// 网页地址在两处的键不一样：接口叫 `html_url`，自己写的缓存叫 `url`。
fn repo_from_json(value: &Value) -> Option<Repo> {
    let url = value
        .get("html_url")
        .or_else(|| value.get("url"))
        .and_then(Value::as_str)?;
    Some(Repo {
        full_name: value.get("full_name")?.as_str()?.to_string(),
        description: value
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        url: url.to_string(),
        language: value
            .get("language")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        stars: value
            .get("stargazers_count")
            .or_else(|| value.get("stars"))
            .and_then(Value::as_u64)
            .unwrap_or(0),
    })
}

/// 用户名和关键词都要转义：中文用户名合法，直接拼进 URL 会坏。
fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(*byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// 现在这一刻，单位秒。
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use super::{Repo, encode, from_json, repo_from_json, to_json};
    use serde_json::json;

    #[test]
    fn a_repo_keeps_only_what_the_list_shows() {
        let repo = repo_from_json(&json!({
            "full_name": "cz-97/gpui-tools",
            "description": null,
            "html_url": "https://github.com/cz-97/gpui-tools",
            "language": "Rust",
            "stargazers_count": 12,
            "owner": { "login": "cz-97" }
        }))
        .expect("有仓库");
        assert_eq!(repo.full_name, "cz-97/gpui-tools");
        assert_eq!(repo.description, "");
        assert_eq!(repo.language, "Rust");
        assert_eq!(repo.stars, 12);
    }

    #[test]
    fn a_cache_round_trip_keeps_every_repo() {
        let repos = vec![Repo {
            full_name: "cz-97/gpui-tools".into(),
            description: "启动器".into(),
            url: "https://github.com/cz-97/gpui-tools".into(),
            language: "Rust".into(),
            stars: 1,
        }];
        let text = to_json(&repos);
        assert_eq!(from_json(&text).expect("读得回来"), repos);
        assert!(from_json("不是 JSON").is_none());
    }

    #[test]
    fn a_query_is_percent_encoded() {
        assert_eq!(encode("cz-97/gpui-tools"), "cz-97%2Fgpui-tools");
        assert_eq!(encode("中文"), "%E4%B8%AD%E6%96%87");
        assert_eq!(encode("a b"), "a%20b");
    }
}
