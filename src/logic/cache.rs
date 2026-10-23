//! 网络结果的本地缓存：一个键一个 JSON 文件。
//!
//! 为什么不用数据库：这些东西只是「上次抓回来的那一份」，丢了从网上再抓一次就有，
//! 和设置那种丢不起的东西不该共用一张表。

use std::path::PathBuf;

/// 缓存文件放哪儿：`%APPDATA%\gpui-tools\cache`。
fn directory() -> Option<PathBuf> {
    Some(super::settings::Settings::directory()?.join("cache"))
}

/// 读一份缓存；没有、读不出来、或者坏了都当作没有。
pub fn read(key: &str) -> Option<String> {
    std::fs::read_to_string(directory()?.join(format!("{key}.json"))).ok()
}

/// 写一份缓存。写失败只是下次再从网上抓，不打扰用户。
pub fn write(key: &str, contents: &str) {
    let Some(directory) = directory() else {
        return;
    };
    if std::fs::create_dir_all(&directory).is_err() {
        return;
    }
    let path = directory.join(format!("{key}.json"));
    // 先写临时文件再改名：中途崩了也不会留下半截 JSON。
    let temporary = path.with_extension("json.tmp");
    if std::fs::write(&temporary, contents).is_ok() {
        if std::fs::rename(&temporary, &path).is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
    }
}
