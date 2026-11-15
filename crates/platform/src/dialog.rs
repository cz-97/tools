//! 系统的「打开文件 / 选择文件夹」对话框。
//!
//! 用户给文件、文件夹条目挑目标时用的就是它——不自己造一套文件浏览界面，
//! 直接用系统的那个框：盘符、最近访问、网络位置、搜索全都是现成的。
//!
//! [`pick_file`] / [`pick_folder`] 都是**阻塞**的，一直等到用户选完或者取消，所以只能
//! 在后台线程里调。调用方负责把结果送回界面线程。

use std::path::PathBuf;

/// 弹出系统的打开文件框，返回选中的文件；取消返回 `None`。
///
/// `owner` 是对话框的宿主窗口句柄（`0` 表示没有）：给了它，对话框就是那个窗口的
/// 模态子窗，会跟着窗口一起居中、一起最小化。
pub fn pick_file(owner: usize, title: &str) -> Option<PathBuf> {
    sys::pick_file(owner, title)
}

/// 弹出系统的「选择文件夹」框，返回选中的目录；取消返回 `None`。
pub fn pick_folder(owner: usize, title: &str) -> Option<PathBuf> {
    sys::pick_folder(owner, title)
}

#[cfg(windows)]
mod sys {
    use std::path::PathBuf;

    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
    use windows_sys::Win32::UI::Controls::Dialogs::{
        GetOpenFileNameW, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST,
        OPENFILENAMEW,
    };

    /// 缓冲多长。`MAX_PATH` 是 260，给长路径留四倍余量。
    const BUFFER: usize = 1024;

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// 系统的文件框会加载 shell 扩展，这些扩展按单线程套间写，先声明一下。
    fn prepare() {
        unsafe {
            CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32);
        }
    }

    pub fn pick_file(owner: usize, title: &str) -> Option<PathBuf> {
        prepare();

        let mut buffer = vec![0u16; BUFFER];
        // 过滤器串是「说明\0通配符\0说明\0通配符\0\0」。
        let filter: Vec<u16> = "所有文件\0*.*\0\0".encode_utf16().collect();
        let title = wide(title);

        let mut options = OPENFILENAMEW {
            lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
            hwndOwner: owner as HWND,
            lpstrFilter: filter.as_ptr(),
            // 一律不给「记住上次目录」以外的隐式行为：不改当前目录、必须选到真实存在的文件。
            Flags: OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR,
            lpstrFile: buffer.as_mut_ptr(),
            nMaxFile: BUFFER as u32,
            lpstrTitle: title.as_ptr(),
            ..Default::default()
        };

        let confirmed = unsafe { GetOpenFileNameW(&mut options) } != 0;
        if !confirmed {
            return None;
        }
        let end = buffer
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(buffer.len());
        let text = String::from_utf16_lossy(&buffer[..end]);
        (!text.is_empty()).then(|| PathBuf::from(text))
    }

    /// 挑文件夹：**和文件框是同一个对话框**，只是加了 `FOS_PICKFOLDERS`。
    ///
    /// 用的是 Vista 之后的 Common Item Dialog（`IFileOpenDialog`）：左边导航窗格、
    /// 地址栏、搜索框、新建文件夹、拖大拖小全都在。老式的 `SHBrowseForFolderW` 长得
    /// 完全是另一个东西——那个树形小窗口用户一眼就看出「不是文件选择器那种对话框」。
    pub fn pick_folder(owner: usize, title: &str) -> Option<PathBuf> {
        use windows::Win32::Foundation::{ERROR_CANCELLED, HWND};
        use windows::Win32::System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoTaskMemFree,
        };
        use windows::Win32::UI::Shell::{
            FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS, FileOpenDialog,
            IFileOpenDialog, SIGDN_FILESYSPATH,
        };
        use windows::core::HSTRING;

        unsafe {
            // COM 已经在这个线程上初始化过就是 `S_FALSE`，无所谓。
            let _ = windows::Win32::System::Com::CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let dialog: IFileOpenDialog =
                CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
            let mut options = dialog.GetOptions().ok()?;
            // 「只认真实目录」+「只回文件系统路径」：库、网络位置里的虚拟项拿不到路径。
            options |= FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST;
            dialog.SetOptions(options).ok()?;
            if !title.is_empty() {
                dialog.SetTitle(&HSTRING::from(title)).ok()?;
            }

            let owner = HWND(owner as *mut core::ffi::c_void);
            if let Err(error) = dialog.Show(Some(owner)) {
                // 用户点取消不是错误，别当成失败去记日志。
                if error.code() != ERROR_CANCELLED.to_hresult() {
                    eprintln!("tools: 选文件夹对话框没能打开：{error}");
                }
                return None;
            }

            let item = dialog.GetResult().ok()?;
            let path = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
            let text = path.to_string().ok()?;
            // 这串是 shell 分配给我们（CoTaskMemAlloc）的，用完要还。
            CoTaskMemFree(Some(path.0 as *const core::ffi::c_void));
            (!text.is_empty()).then(|| PathBuf::from(text))
        }
    }
}

#[cfg(not(windows))]
mod sys {
    use std::path::PathBuf;

    pub fn pick_file(_owner: usize, _title: &str) -> Option<PathBuf> {
        None
    }

    pub fn pick_folder(_owner: usize, _title: &str) -> Option<PathBuf> {
        None
    }
}
