//! 一个最小可用的阻塞式 HTTPS 客户端，走 Windows 的 WinINet。
//!
//! 为什么不用现成的 crate：这里只需要「GET 一个 JSON」，为它拖进一整套异步 HTTP 栈
//! 不划算；GPUI 自带的 `http_client` 在桌面端是空实现（`No HttpClient available`）。
//!
//! 为什么是 WinINet 而不是 WinHTTP：WinHTTP 只认 `netsh winhttp` 那套配置，**不认**
//! 系统代理（Clash 这类工具改的是 WinINet 的代理设置）；WinINet 用 `PRECONFIG` 打开
//! 就跟着系统代理和 PAC 走，用户开着代理时才连得上 GitHub。
//!
//! **阻塞**：调用方必须放在后台线程里（`cx.background_executor().spawn`），
//! 绝不能在 UI 线程上直接调。

/// 一次响应。
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

/// 发一个 GET，把状态码和整段响应体拿回来。
pub fn get(url: &str, headers: &[(&str, String)]) -> Result<Response, String> {
    sys::get(url, headers)
}

#[cfg(windows)]
mod sys {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt as _;
    use std::ptr;

    use windows_sys::Win32::Networking::WinInet::{
        HTTP_QUERY_FLAG_NUMBER, HTTP_QUERY_STATUS_CODE, HttpQueryInfoW, INTERNET_FLAG_RELOAD,
        INTERNET_OPEN_TYPE_PRECONFIG, INTERNET_OPTION_CONNECT_TIMEOUT,
        INTERNET_OPTION_RECEIVE_TIMEOUT, INTERNET_OPTION_SEND_TIMEOUT, InternetCloseHandle,
        InternetOpenUrlW, InternetOpenW, InternetReadFile, InternetSetOptionW,
    };

    use super::Response;

    /// WinINet 句柄：出作用域自动关，出错提前返回也不会漏。
    struct Handle(*mut c_void);

    impl Drop for Handle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: 句柄是本模块从 WinINet 拿到的，且只在这里关一次。
                unsafe { InternetCloseHandle(self.0) };
            }
        }
    }

    /// 一次读多少字节。
    const CHUNK: usize = 8 * 1024;

    fn wide(text: &str) -> Vec<u16> {
        std::ffi::OsStr::new(text)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    pub fn get(url: &str, headers: &[(&str, String)]) -> Result<Response, String> {
        let agent = wide("gpui-tools");
        // SAFETY: 每个句柄都判了空，交给 `Handle` 关闭；缓冲区都是本栈上的。
        unsafe {
            let session = Handle(InternetOpenW(
                agent.as_ptr(),
                INTERNET_OPEN_TYPE_PRECONFIG,
                ptr::null(),
                ptr::null(),
                0,
            ));
            if session.0.is_null() {
                return Err(last_error());
            }
            // 解析/连接 10 秒、发送 10 秒、收 20 秒：卡住就报错，不无限等。
            set_timeout(session.0, INTERNET_OPTION_CONNECT_TIMEOUT, 10_000);
            set_timeout(session.0, INTERNET_OPTION_SEND_TIMEOUT, 10_000);
            set_timeout(session.0, INTERNET_OPTION_RECEIVE_TIMEOUT, 20_000);

            // 头拼成一整块，每行 CRLF 结束；长度按字符数给。
            let mut block = String::new();
            for (name, value) in headers {
                block.push_str(name);
                block.push_str(": ");
                block.push_str(value);
                block.push_str("\r\n");
            }
            let block_wide = wide(&block);
            let url_wide = wide(url);
            let request = Handle(InternetOpenUrlW(
                session.0,
                url_wide.as_ptr(),
                if block.is_empty() {
                    ptr::null()
                } else {
                    block_wide.as_ptr()
                },
                block.len() as u32,
                INTERNET_FLAG_RELOAD,
                0,
            ));
            if request.0.is_null() {
                return Err(last_error());
            }

            let mut status: u32 = 0;
            let mut length = std::mem::size_of::<u32>() as u32;
            if HttpQueryInfoW(
                request.0,
                HTTP_QUERY_STATUS_CODE | HTTP_QUERY_FLAG_NUMBER,
                &mut status as *mut u32 as *mut c_void,
                &mut length,
                ptr::null_mut(),
            ) == 0
            {
                return Err(last_error());
            }

            let mut body = Vec::new();
            let mut buffer = vec![0u8; CHUNK];
            loop {
                let mut read: u32 = 0;
                if InternetReadFile(
                    request.0,
                    buffer.as_mut_ptr() as *mut c_void,
                    CHUNK as u32,
                    &mut read,
                ) == 0
                {
                    return Err(last_error());
                }
                if read == 0 {
                    break;
                }
                body.extend_from_slice(&buffer[..read as usize]);
            }

            Ok(Response {
                status: status as u16,
                body,
            })
        }
    }

    /// 给会话设一个超时（单位毫秒）。
    unsafe fn set_timeout(session: *mut c_void, option: u32, milliseconds: u32) {
        // SAFETY: 参数指向本栈上的 u32，长度按 u32 给。
        unsafe {
            InternetSetOptionW(
                session,
                option,
                &milliseconds as *const u32 as *const c_void,
                std::mem::size_of::<u32>() as u32,
            );
        }
    }

    /// 把 `GetLastError` 翻成一句能贴进界面的错误。
    ///
    /// 常见的那几个码基本只有一个意思：网络或代理不通。
    fn last_error() -> String {
        let code = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        let hint = match code {
            12007 => "域名解析不了",
            12029 | 12030 | 12031 => "网络或代理不通",
            12002 => "等太久了，超时",
            12019 => "网络已断开",
            _ => "",
        };
        if hint.is_empty() {
            format!("请求失败（错误码 {code}）")
        } else {
            format!("连不上服务器：{hint}（错误码 {code}）")
        }
    }
}

#[cfg(not(windows))]
mod sys {
    use super::Response;

    pub fn get(_url: &str, _headers: &[(&str, String)]) -> Result<Response, String> {
        Err("这个平台还没有实现 HTTP 请求".to_string())
    }
}

#[cfg(test)]
mod tests {
    /// 连不上的地址要报错，不能 panic、也不能当成空响应。
    #[test]
    fn an_unreachable_target_reports_an_error() {
        let result = super::get("https://127.0.0.1:1/nothing", &[]);
        assert!(result.is_err());
    }
}
