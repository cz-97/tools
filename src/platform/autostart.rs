//! 开机启动项。
//!
//! Windows 靠 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` 下的一个值
//! 在登录时启动程序：不需要提权，用户在任务管理器的启动页里也看得见。**注册表才是
//! 事实**：[`enabled`] 每次都去读它，所以用户在别处删掉的那一项不会被悄悄重建。
//!
//! 但「Run 项在」不等于「会启动」：任务管理器的启动页把每一项的启用状态另存一份在
//! `Explorer\StartupApproved\Run`，那里标了「已禁用」的话，登录时根本不跑。读的时候
//! 两边都要看，写的时候两边都要写——只写 Run 项就是在骗自己。
//!
//! 别的平台上每个调用都是「失败的空操作」，调用方因此不必关心平台差异。

/// 启动项的名字，也是用户在任务管理器里看到的那一行。
const ENTRY_NAME: &str = "Tools";

/// 有没有一项在登录时启动这个可执行文件。
pub fn enabled() -> bool {
    sys::enabled(ENTRY_NAME)
}

/// 加上或删掉启动项，返回是否生效。
pub fn set_enabled(enabled: bool) -> bool {
    sys::set_enabled(ENTRY_NAME, enabled)
}

/// 清掉改名前留下的死启动项（指向的文件已经不存在了）。
///
/// 只删「命令指向的 exe 不存在」的那些，名字写死在 `LEGACY_ENTRIES` 里。启动时顺手调一次：
/// 用户会在任务管理器的启动页看到一条 `GPUI Tools` → 一年前就不存在的路径，除了碍眼
/// 没有任何作用，而它跟现在这个程序的关系从名字上根本看不出来。
pub fn drop_dead_entries() {
    sys::drop_dead_entries();
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
    /// 任务管理器的「启动」页把每一项的启用/禁用状态存在这里。**它优先于 Run 项**：
    /// 用户在那儿关掉之后，Run 项还在，但登录时不会跑。
    const APPROVAL_KEY: &str =
        "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run";
    /// 改名前（`gpui-tools` 那阵）留下的启动项名字。它指向的路径早就不存在了，
    /// 用户也没法从名字上看出这俩是同一个程序，所以重新开启时顺手清掉。
    const LEGACY_ENTRIES: [&str; 1] = ["GPUI Tools"];

    /// 启动项该跑的命令行。
    fn command() -> String {
        let exe = std::env::current_exe().unwrap_or_default();
        // 带引号，路径里有空格时也能当成一个参数跑起来。
        format!("\"{}\"", exe.display())
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn open_path(path: &str, access: u32) -> Option<HKEY> {
        let path = wide(path);
        let mut handle: HKEY = ptr::null_mut();
        let status =
            unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, path.as_ptr(), 0, access, &mut handle) };
        (status == 0).then_some(handle)
    }

    /// 打开 `RUN_KEY`，读或写。
    fn open(access: u32) -> Option<HKEY> {
        open_path(RUN_KEY, access)
    }

    pub fn enabled(name: &str) -> bool {
        // 先看任务管理器留下的「已禁用」记录：有它的话 Run 项在也不跑。
        if approval_disabled(name) {
            return false;
        }
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
        let name_wide = wide(name);
        let status = if enabled {
            let value = wide(&command());
            let bytes = (value.len() * std::mem::size_of::<u16>()) as u32;
            unsafe {
                RegSetValueExW(
                    key,
                    name_wide.as_ptr(),
                    0,
                    REG_SZ,
                    value.as_ptr().cast::<u8>(),
                    bytes,
                )
            }
        } else {
            unsafe { RegDeleteValueW(key, name_wide.as_ptr()) }
        };
        unsafe { RegCloseKey(key) };

        if enabled && status == 0 {
            // 光写 Run 项不够：用户在任务管理器里关掉的启动项，Windows 会在
            // StartupApproved 里记一个「已禁用」，那条记录优先于 Run 项。
            set_approval(name, true);
            // 顺手把改名前留下的旧项清掉（旧项指向的路径早就不存在了）。
            for legacy in LEGACY_ENTRIES {
                remove_entry(legacy);
            }
        }
        if !enabled {
            set_approval(name, false);
        }
        // 删一个本来就不存在的项算成功，不算失败。
        status == 0 || (!enabled && status == 2)
    }

    /// `StartupApproved\Run` 里这个名字现在是不是「已禁用」。
    fn approval_disabled(name: &str) -> bool {
        use windows_sys::Win32::System::Registry::REG_BINARY;

        let Some(key) = open_path(APPROVAL_KEY, KEY_QUERY_VALUE) else {
            return false;
        };
        let name = wide(name);
        let mut kind = 0u32;
        let mut data = [0u8; 16];
        let mut bytes = data.len() as u32;
        let status = unsafe {
            RegQueryValueExW(
                key,
                name.as_ptr(),
                ptr::null(),
                &mut kind,
                data.as_mut_ptr(),
                &mut bytes,
            )
        };
        unsafe { RegCloseKey(key) };
        // 第一个字节：0x02 = 启用，0x03 = 用户禁用了。
        status == 0 && kind == REG_BINARY && bytes > 0 && data[0] == 0x03
    }

    /// 写 / 删 `StartupApproved\Run` 里那 12 个字节。
    ///
    /// 启用时写成 `02` + 11 个 0：这是 Windows 自己的「已启用」写法，写进去相当于
    /// 在任务管理器的启动页里把它重新打开。
    fn set_approval(name: &str, enabled: bool) {
        use windows_sys::Win32::System::Registry::REG_BINARY;

        let Some(key) = create_path(APPROVAL_KEY) else {
            return;
        };
        let name = wide(name);
        if enabled {
            let data = [0x02u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
            unsafe {
                RegSetValueExW(
                    key,
                    name.as_ptr(),
                    0,
                    REG_BINARY,
                    data.as_ptr(),
                    data.len() as u32,
                );
            }
        } else {
            unsafe { RegDeleteValueW(key, name.as_ptr()) };
        }
        unsafe { RegCloseKey(key) };
    }

    /// 打开（必要时创建）一个键，只要写权限。
    fn create_path(path: &str) -> Option<HKEY> {
        use windows_sys::Win32::System::Registry::{KEY_SET_VALUE as SET, RegCreateKeyExW};

        let path = wide(path);
        let mut handle: HKEY = ptr::null_mut();
        let mut disposition = 0u32;
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                path.as_ptr(),
                0,
                ptr::null_mut(),
                0,
                SET,
                ptr::null(),
                &mut handle,
                &mut disposition,
            )
        };
        (status == 0).then_some(handle)
    }

    /// 把某个名字的启动项（Run 项 + 它的启用状态）一起删掉。
    fn remove_entry(name: &str) {
        if let Some(key) = open(KEY_SET_VALUE) {
            let name_wide = wide(name);
            unsafe { RegDeleteValueW(key, name_wide.as_ptr()) };
            unsafe { RegCloseKey(key) };
        }
        set_approval(name, false);
    }

    /// 读某个启动项的命令行，读不到返回 `None`。
    fn command_of(name: &str) -> Option<String> {
        let key = open(KEY_QUERY_VALUE)?;
        let name = wide(name);
        let mut kind = 0u32;
        let mut bytes = 0u32;
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
        if status != 0 || kind != REG_SZ || bytes < 2 {
            unsafe { RegCloseKey(key) };
            return None;
        }
        let mut buffer = vec![0u16; (bytes as usize / 2) + 1];
        let mut filled = bytes;
        let status = unsafe {
            RegQueryValueExW(
                key,
                name.as_ptr(),
                ptr::null(),
                &mut kind,
                buffer.as_mut_ptr().cast::<u8>(),
                &mut filled,
            )
        };
        unsafe { RegCloseKey(key) };
        if status != 0 {
            return None;
        }
        let end = buffer.iter().position(|unit| *unit == 0).unwrap_or(0);
        Some(String::from_utf16_lossy(&buffer[..end]))
    }

    pub fn drop_dead_entries() {
        for legacy in LEGACY_ENTRIES {
            let Some(command) = command_of(legacy) else {
                continue;
            };
            // 命令行形如 `"C:\path\app.exe" --args`：取第一对引号里的路径，
            // 没有引号就取到第一个空格。
            let path = match command.strip_prefix('"') {
                Some(rest) => rest.split('"').next().unwrap_or_default().to_string(),
                None => command
                    .split_whitespace()
                    .next()
                    .unwrap_or_default()
                    .to_string(),
            };
            if !path.is_empty() && !std::path::Path::new(&path).exists() {
                remove_entry(legacy);
            }
        }
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

    pub fn drop_dead_entries() {}
}
