//! 每个命令对应的动作。
//!
//! 这里是「命令声明」里 `action` 字段指向的那批函数：命令表只声明 id、名字和动作，
//! 具体做什么全在这个文件里，不掺 UI 逻辑。

use std::path::PathBuf;

use gpui_kit::App;
use gpui_tools::settings::Settings;
use gpui_tools::shell;

use crate::app::{release_singleton, remember_window_position};

/// 项目主页。
pub const REPOSITORY_URL: &str = "https://github.com/cz-97/gpui-tools";

/// 启动器自己所在的目录。
pub fn program_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(std::path::Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// 打开程序所在文件夹。
pub fn open_program_folder(_cx: &mut App) {
    open_folder(&program_dir());
}

/// 在默认浏览器里打开项目主页。
pub fn open_repository(_cx: &mut App) {
    shell::open(REPOSITORY_URL);
}

/// 打开书签页。
pub fn open_bookmarks(cx: &mut App) {
    crate::app::open_list(crate::ui::launcher::ListKind::Bookmarks, cx);
}

/// 打开历史页。
pub fn open_history(cx: &mut App) {
    crate::app::open_list(crate::ui::launcher::ListKind::History, cx);
}

/// 打开个人项目页。
pub fn open_github_projects(cx: &mut App) {
    crate::app::open_list(crate::ui::launcher::ListKind::Projects, cx);
}

/// 打开星标页。
pub fn open_github_stars(cx: &mut App) {
    crate::app::open_list(crate::ui::launcher::ListKind::Stars, cx);
}

/// 打开查询仓库页。
pub fn open_github_search(cx: &mut App) {
    crate::app::open_list(crate::ui::launcher::ListKind::Search, cx);
}

/// 打开存设置的那个文件夹。
pub fn open_settings_folder(_cx: &mut App) {
    let Some(dir) = Settings::directory() else {
        eprintln!("gpui-tools: 找不到配置目录");
        return;
    };
    if !dir.is_dir() {
        eprintln!("gpui-tools: {} 还没被创建", dir.display());
        return;
    }
    open_folder(&dir);
}

/// 退出启动器。
pub fn quit_launcher(cx: &mut App) {
    // 面板还在，这是最后一次记下它停在哪。
    remember_window_position(cx);
    cx.quit();
}

/// 重启：先拉起新进程，再让当前这个退出。
///
/// 程序是全局单例，所以**先把自己那份单例身份交出去**再拉新的：新的启动时就能直接
/// 抢到位置，不必等在单例上——那种跨进程的等待又慢又难判断卡在哪。
/// 新的照旧靠 `shortcuts::install` 的重试去拿热键，旧的没退干净也只是晚几百毫秒。
pub fn restart_launcher(cx: &mut App) {
    remember_window_position(cx);
    release_singleton(cx);
    match std::env::current_exe() {
        Ok(exe) => {
            if let Err(error) = std::process::Command::new(exe).spawn() {
                eprintln!("gpui-tools: 重启失败：{error}");
            }
        }
        Err(error) => eprintln!("gpui-tools: 找不到自身路径，无法重启：{error}"),
    }
    cx.quit();
}

/// 交给系统默认的文件管理器打开一个目录。
///
/// 关联由 shell 解析，这里写死 Explorer 之类的名字就等于无视用户的选择。
fn open_folder(dir: &std::path::Path) {
    shell::open(dir);
}
