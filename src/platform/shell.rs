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

/// 让 shell 用默认处理器打开 `target`。
///
/// 立刻返回；失败由那个尝试打开的线程写到标准错误。
pub fn open(target: impl AsRef<OsStr>) {
    let target = target.as_ref().to_owned();
    std::thread::spawn(move || {
        sys::prepare();
        if !sys::open(&target) {
            eprintln!(
                "gpui-tools: could not open {}",
                std::path::Path::new(&target).display()
            );
        }
    });
}

#[cfg(windows)]
mod sys {
    use std::ffi::OsStr;
    use std::ptr;
    use windows_sys::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    /// `ShellExecuteW` 用「大于 32」表示成功；更小的值是它自己的错误码。
    const SHELL_SUCCESS: isize = 32;

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
}

#[cfg(not(windows))]
mod sys {
    use std::ffi::OsStr;

    pub fn prepare() {}

    pub fn open(_target: &OsStr) -> bool {
        false
    }
}
