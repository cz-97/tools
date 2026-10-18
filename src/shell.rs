//! Opening folders and links the way the user's own settings say to.
//!
//! Naming a program — `explorer.exe`, a browser — would pin the host to one
//! choice the user can change at any time. `ShellExecuteW` asks the shell to
//! resolve the association instead, so a folder opens in whatever file manager
//! is registered as the default and a link opens in the default browser.
//!
//! The call happens on a thread of its own. `ShellExecuteW` runs the shell's
//! message pump, and starting that pump while GPUI holds its application borrow
//! lets the shell deliver events straight back into the launcher, which panics
//! with "RefCell already borrowed" and aborts the process. The call also blocks
//! until the handler takes over. A short-lived thread with COM initialised keeps
//! both off the UI thread.

use std::ffi::OsStr;

/// Ask the shell to open `target` with its default handler.
///
/// Returns immediately; a failure is reported on standard error from the thread
/// that attempted the open.
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

    /// `ShellExecuteW` reports success as anything above 32; the smaller values
    /// are its own error codes.
    const SHELL_SUCCESS: isize = 32;

    /// Give this thread the apartment the shell expects.
    ///
    /// A failure is not fatal: the open still runs, it only loses the shell
    /// extensions that need COM.
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
