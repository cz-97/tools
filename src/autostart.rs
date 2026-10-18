//! The login startup entry.
//!
//! Windows starts an application at login by way of a value under
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, which needs no
//! elevation and is visible to the user in Task Manager's startup tab. The
//! registry is the source of truth: [`enabled`] reads it back, so an entry the
//! user removed elsewhere is not silently re-created.
//!
//! On other platforms every call is a no-op that reports failure, so callers
//! stay platform-agnostic.

/// The name the startup entry is filed under, which is what the user sees in
/// Task Manager.
const ENTRY_NAME: &str = "GPUI Tools";

/// Whether an entry that starts this executable is in place.
pub fn enabled() -> bool {
    sys::enabled(ENTRY_NAME)
}

/// Add or remove the startup entry, reporting whether it took effect.
pub fn set_enabled(enabled: bool) -> bool {
    sys::set_enabled(ENTRY_NAME, enabled)
}

#[cfg(windows)]
mod sys {
    use std::ptr;
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ, RegCloseKey,
        RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    };

    /// The key Windows reads startup entries from.
    const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

    /// The command line a startup entry should run.
    fn command() -> String {
        let exe = std::env::current_exe().unwrap_or_default();
        // Quoted, so a path containing spaces still runs as one argument.
        format!("\"{}\"", exe.display())
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn open(access: u32) -> Option<HKEY> {
        let key = wide(RUN_KEY);
        let mut handle: HKEY = ptr::null_mut();
        let status =
            unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, key.as_ptr(), 0, access, &mut handle) };
        (status == 0).then_some(handle)
    }

    pub fn enabled(name: &str) -> bool {
        let Some(key) = open(KEY_QUERY_VALUE) else {
            return false;
        };
        let name = wide(name);
        let mut kind = 0u32;
        let mut bytes = 0u32;
        // Asking for the size first keeps the read independent of how long the
        // command line of some other installation might be.
        let status = unsafe {
            RegQueryValueExW(
                key,
                name.as_ptr(),
                ptr::null(),
                &mut kind,
                ptr::null_mut(),
                &mut bytes,
            )
        };
        unsafe { RegCloseKey(key) };
        status == 0 && kind == REG_SZ
    }

    pub fn set_enabled(name: &str, enabled: bool) -> bool {
        let Some(key) = open(KEY_SET_VALUE) else {
            return false;
        };
        let name = wide(name);
        let status = if enabled {
            let value = wide(&command());
            let bytes = (value.len() * std::mem::size_of::<u16>()) as u32;
            unsafe {
                RegSetValueExW(
                    key,
                    name.as_ptr(),
                    0,
                    REG_SZ,
                    value.as_ptr().cast::<u8>(),
                    bytes,
                )
            }
        } else {
            unsafe { RegDeleteValueW(key, name.as_ptr()) }
        };
        unsafe { RegCloseKey(key) };
        // Removing an entry that was never there is a success, not a failure.
        status == 0 || (!enabled && status == 2)
    }

    #[cfg(test)]
    mod tests {
        use super::command;

        #[test]
        fn the_startup_command_quotes_the_executable() {
            let command = command();
            assert!(command.starts_with('"'), "{command}");
            assert!(command.ends_with('"'), "{command}");
        }
    }
}

#[cfg(not(windows))]
mod sys {
    pub fn enabled(_name: &str) -> bool {
        false
    }

    pub fn set_enabled(_name: &str, _enabled: bool) -> bool {
        false
    }
}
