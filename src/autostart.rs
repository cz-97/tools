//! 开机启动项。
//!
//! Windows 靠 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` 下的一个值
//! 在登录时启动程序：不需要提权，用户在任务管理器的启动页里也看得见。**注册表才是
//! 事实**：[`enabled`] 每次都去读它，所以用户在别处删掉的那一项不会被悄悄重建。
//!
//! 别的平台上每个调用都是「失败的空操作」，调用方因此不必关心平台差异。

/// 启动项的名字，也是用户在任务管理器里看到的那一行。
const ENTRY_NAME: &str = "GPUI Tools";

/// 有没有一项在登录时启动这个可执行文件。
pub fn enabled() -> bool {
    sys::enabled(ENTRY_NAME)
}

/// 加上或删掉启动项，返回是否生效。
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

    /// Windows 读启动项的那个键。
    const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

    /// 启动项该跑的命令行。
    fn command() -> String {
        let exe = std::env::current_exe().unwrap_or_default();
        // 带引号，路径里有空格时也能当成一个参数跑起来。
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
        // 先问长度再读，读出来的东西就不受别的安装写多长的命令行影响。
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
        // 删一个本来就不存在的项算成功，不算失败。
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
