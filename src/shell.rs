//! Opening folders and links the way the user's own settings say to.
//!
//! Naming a program — `explorer.exe`, a browser — would pin the host to one
//! choice the user can change at any time. `ShellExecuteW` asks the shell to
//! resolve the association instead, so a folder opens in whatever file manager
//! is registered as the default and a link opens in the default browser.
//!
//! The target is passed as one argument, never through a shell command line, so
//! there is nothing to quote or escape.

/// Ask the shell to open `target` with its default handler.
///
/// Returns whether the shell accepted the request; the handler may still fail
/// afterwards, which is out of the host's hands.
pub fn open(target: impl AsRef<std::ffi::OsStr>) -> bool {
    sys::open(target.as_ref())
}

#[cfg(windows)]
mod sys {
    use std::ffi::OsStr;
    use std::ptr;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    /// `ShellExecuteW` reports success as anything above 32; the smaller values
    /// are its own error codes.
    const SHELL_SUCCESS: isize = 32;

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

    pub fn open(_target: &OsStr) -> bool {
        false
    }
}
