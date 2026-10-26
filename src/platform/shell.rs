//! 按用户自己的设置去打开文件夹和链接。
//!
//! 点名某个程序——`explorer.exe`、某个浏览器——等于把用户随时能改的选择钉死。
//! 这里改用 `ShellExecuteW` 让 shell 去解析关联：文件夹用系统登记的文件管理器打开，
//! 链接用默认浏览器打开。
//!
//! 这一步放在单独的线程里。`ShellExecuteW` 会跑 shell 自己的消息泵，而这个泵在 GPUI
//! 还持着 App 借用的时候把事件送回面板，就会 `RefCell already borrowed`，进而
//! `panic in a function that cannot unwind`，进程直接 abort。这个调用还会一直阻塞到
//! 处理器接手。开一个初始化过 COM 的短命线程，两件事都离开了 UI 线程。

use std::ffi::OsStr;
use std::path::Path;

/// 让 shell 用默认处理器打开 `target`。
///
/// 立刻返回；失败由那个尝试打开的线程写到标准错误。
pub fn open(target: impl AsRef<OsStr>) {
    let target = target.as_ref().to_owned();
    std::thread::spawn(move || {
        sys::prepare();
        if !sys::open(&target) {
            eprintln!(
                "tools: could not open {}",
                std::path::Path::new(&target).display()
            );
        }
    });
}

/// 打开 `path` 的「位置」：是文件夹就打开它，是文件就打开它所在的文件夹。
///
/// 同样交给 shell 解析关联，用户换过文件管理器也照样是他选的那个。
pub fn open_location(path: impl AsRef<Path>) {
    let path = path.as_ref().to_path_buf();
    std::thread::spawn(move || {
        sys::prepare();
        let Some(folder) = sys::location(&path) else {
            eprintln!("tools: {} 打不开，路径已经不在", path.display());
            return;
        };
        if !sys::open(folder.as_os_str()) {
            eprintln!("tools: could not open {}", folder.display());
        }
    });
}

/// 在系统默认终端里跑一条命令，返回控制台窗口有没有被拉起来。
///
/// 用的是 `cmd.exe /K`：命令跑完窗口留着，用户还能接着看输出、接着敲。
/// 开新控制台交给系统，Windows 11 设的默认终端（Windows Terminal 之类）会自己接手，
/// 这里不点名任何一个终端程序。
///
/// `dir` 是终端的工作目录；给了空值就落在**用户文件夹**（`%USERPROFILE%`）——
/// 从启动器直接敲命令的人，八成是想在自己的地盘上干活，而不是程序所在目录。
pub fn run_in_terminal(command: &str, dir: Option<&Path>) -> bool {
    let command = command.trim();
    if command.is_empty() {
        return false;
    }
    let dir = dir
        .map(Path::to_path_buf)
        .filter(|path| path.is_dir())
        .or_else(|| std::env::var_os("USERPROFILE").map(std::path::PathBuf::from));
    match sys::spawn_console("cmd", command, dir.as_deref()) {
        Ok(()) => true,
        Err(error) => {
            eprintln!("tools: 终端没能打开（{error}）");
            false
        }
    }
}

#[cfg(windows)]
mod sys {
    use std::ffi::OsStr;
    use std::path::{Path, PathBuf};
    use std::ptr;
    use windows_sys::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    /// `ShellExecuteW` 用「大于 32」表示成功；更小的值是它自己的错误码。
    const SHELL_SUCCESS: isize = 32;
    /// `CreateProcess` 的「开一个新的控制台窗口」标志。
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;

    /// 把这个线程设成 shell 期待的单线程套间。
    ///
    /// 失败也不致命：照样能打开，只是少了需要 COM 的那些 shell 扩展。
    pub fn prepare() {
        unsafe {
            CoInitializeEx(ptr::null(), COINIT_APARTMENTTHREADED as u32);
        }
    }

    fn wide(text: &OsStr) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt as _;
        text.encode_wide().chain(std::iter::once(0)).collect()
    }

    pub fn open(target: &OsStr) -> bool {
        let verb = wide(OsStr::new("open"));
        let file = wide(target);
        let result = unsafe {
            ShellExecuteW(
                ptr::null_mut(),
                verb.as_ptr(),
                file.as_ptr(),
                ptr::null(),
                ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        result as isize > SHELL_SUCCESS
    }

    /// 该打开哪个文件夹：文件夹返回它自己，文件返回它的上级目录。
    ///
    /// 路径不在了返回 `None`——这种情况由界面上那条提示去问用户删还是改。
    pub fn location(path: &Path) -> Option<PathBuf> {
        let metadata = std::fs::metadata(path).ok()?;
        if metadata.is_dir() {
            return Some(path.to_path_buf());
        }
        path.parent().map(Path::to_path_buf)
    }

    pub fn spawn_console(program: &str, command: &str, dir: Option<&Path>) -> std::io::Result<()> {
        use std::os::windows::process::CommandExt as _;
        let mut builder = std::process::Command::new(program);
        // `raw_arg` 原样拼在命令行后面：`/K` 后面跟的是用户写的那一整条命令，
        // 中间再加一层引号，带空格的路径就会被 cmd 自己吃掉一层。
        builder.arg("/K").raw_arg(command);
        if let Some(dir) = dir {
            builder.current_dir(dir);
        }
        builder.creation_flags(CREATE_NEW_CONSOLE).spawn()?;
        Ok(())
    }
}

#[cfg(not(windows))]
mod sys {
    use std::ffi::OsStr;
    use std::path::{Path, PathBuf};

    pub fn prepare() {}

    pub fn open(_target: &OsStr) -> bool {
        false
    }

    pub fn location(_path: &Path) -> Option<PathBuf> {
        None
    }

    pub fn spawn_console(
        _program: &str,
        _command: &str,
        _dir: Option<&Path>,
    ) -> std::io::Result<()> {
        Err(std::io::Error::other("这个平台上没有终端可开"))
    }
}
