//! 每个命令对应的动作。
//!
//! 这里是「命令声明」里 `action` 字段指向的那批函数：命令表只声明 id、名字和动作，
//! 具体做什么全在这个文件里，不掺 UI 逻辑。

use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::App;
use tools_core::settings::Settings;
use tools_platform::admin;
use tools_platform::instance;
use tools_platform::shell;

use crate::app::{release_singleton, remember_window_position};

/// 项目主页。
pub const REPOSITORY_URL: &str = env!("CARGO_PKG_REPOSITORY");

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

/// 打开「自定义条目」页：文件 / 文件夹 / 快捷终端都在这儿。
pub fn open_user_tools(cx: &mut App) {
    // 和书签、历史一样：先唤出面板再换页。少了这一步，全局快捷键按下去只是把页面
    // 换掉了，窗口根本没出来——看起来就是「全局没反应」。
    crate::app::show_launcher(cx);
    crate::app::with_launcher(cx, |launcher, window, cx| {
        launcher.open_user_tools(window, cx)
    });
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
        eprintln!("tools: 找不到配置目录");
        return;
    };
    if !dir.is_dir() {
        eprintln!("tools: {} 还没被创建", dir.display());
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

/// 重新拉起自己之后，最多等多久确认它真的起来了。
const RESTART_WAIT: Duration = Duration::from_millis(3000);

/// 重启：先拉起新进程，再让当前这个退出。
///
/// 两条路，按「**当前进程提权没有**」分（不是看设置里那个开关）：
///
/// - **已经提权**：直接 `spawn_self`。子进程继承提权令牌，不弹 UAC，也没有计划任务
///   那一跳的排队和延迟，最快也最可靠。
/// - **没提权、但任务在**：只能让任务去启动（这是唯一不弹 UAC 的提权方式），然后
///   **必须确认新进程真的出现了再退出**。`schtasks /Run` 返回成功只代表任务被接受——
///   任务可能一直排队（电池策略、上一个实例没退干净），拿它当证据的后果就是
///   「退出以后什么都没起来」，实际踩过。
///   等不到就退回 `spawn_self`：宁可给一个普通权限的实例，也不能让用户什么都没有。
///
/// 顺序上**必须先交单例身份**再拉新进程：不然新的那个一眼看到互斥量就自己退了。
pub fn restart_launcher(cx: &mut App) {
    remember_window_position(cx);
    release_singleton(cx);

    if admin::is_elevated() {
        spawn_self();
        admin::log("重启：当前已提权，直接拉子进程（继承提权）");
        cx.quit();
        return;
    }

    if admin::installed() && admin::launch() {
        if instance::wait_for_new(RESTART_WAIT) {
            admin::log("重启：任务已拉起新实例");
            cx.quit();
            return;
        }
        admin::log("重启：任务没在 3 秒内拉起进程（多半是排队了），退回直接拉普通进程");
    }

    spawn_self();
    cx.quit();
}

/// 直接把当前的 exe 再拉起来（普通权限）。
fn spawn_self() {
    match std::env::current_exe() {
        Ok(exe) => {
            if let Err(error) = std::process::Command::new(exe).spawn() {
                eprintln!("tools: 重启失败：{error}");
            }
        }
        Err(error) => eprintln!("tools: 找不到自身路径，无法重启：{error}"),
    }
}

/// 交给系统默认的文件管理器打开一个目录。
///
/// 关联由 shell 解析，这里写死 Explorer 之类的名字就等于无视用户的选择。
fn open_folder(dir: &std::path::Path) {
    shell::open(dir);
}
