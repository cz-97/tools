//! 文件 / 命令的**系统图标**。
//!
//! 用的是资源管理器同一套东西（`SHGetFileInfoW` + `SHGFI_ICON`），所以「本来该长什么样」
//! 这件事不用我们操心：
//!
//! - `.exe` → 它自己的图标（内嵌的优先，没有就用默认的 exe 图标）
//! - `.msc` / `.cpl` / `.pdf` 之类 → 关联程序登记的图标（任务计划程序、控制面板……）
//! - 没关联的扩展名 → 系统的默认文件图标
//! - 文件夹 → 文件夹图标
//! - 快捷终端 → 把命令的第一个词解析成程序路径，再按上面那套取
//!
//! 这些图标不落盘、不随包发布，全是当场问系统要的，所以用户装了新程序、换了关联，
//! 下次打开就和资源管理器一致了。
//!
//! 取一次要几毫秒（shell 查询 + 位图转换），所以按「种类 + 目标」缓存；同一份图标给
//! 每次重绘复用同一个 `Arc<RenderImage>`。

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use gpui_kit::RenderImage;
use tools_core::settings::ToolKind;

/// 取过的图标。`None` 表示「问过了，系统也给不出来」，不再重复问。
fn cache() -> &'static Mutex<HashMap<String, Option<Arc<RenderImage>>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<Arc<RenderImage>>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 某个条目该显示的系统图标；取不到返回 `None`，调用方退回内置图标。
pub fn for_tool(kind: ToolKind, target: &str) -> Option<Arc<RenderImage>> {
    let path = target_for(kind, target)?;
    let key = path.to_string_lossy().to_lowercase();
    if let Some(cached) = cache().lock().expect("图标缓存不会中毒").get(&key).cloned() {
        return cached;
    }
    let icon = sys::load(&path);
    cache()
        .lock()
        .expect("图标缓存不会中毒")
        .insert(key, icon.clone());
    icon
}

/// 命令的第一个词如果是个 `System32` 里的文件（`.msc`、`.cpl`、`.exe`），返回它的路径。
///
/// 命令行能直接敲 `services.msc`，靠的就是这个目录在搜索路径里。
fn system_file(command: &str) -> Option<std::path::PathBuf> {
    let token = command.split_whitespace().next()?.trim_matches('"');
    if token.is_empty() || token.contains('/') || token.contains('\\') {
        return None;
    }
    let root = std::env::var_os("SystemRoot")?;
    let candidate = std::path::Path::new(&root).join("System32").join(token);
    candidate.is_file().then_some(candidate)
}

/// 这个条目对应磁盘上的哪个路径。
fn target_for(kind: ToolKind, target: &str) -> Option<std::path::PathBuf> {
    let target = target.trim();
    if target.is_empty() {
        return None;
    }
    match kind {
        // 文件和文件夹：目标本身就是路径。
        ToolKind::File | ToolKind::Folder => {
            let path = std::path::PathBuf::from(target);
            path.exists().then_some(path)
        }
        // 快捷终端：命令的第一个词大概率是个程序。
        //
        // 解析不出来也要再试一步：`services.msc`、`appwiz.cpl` 这种既不在 PATH 上、也不是
        // exe，但系统在 `System32` 里找得到（命令行敲它们能开，就是靠这个目录）。少了这
        // 一步，一票系统工具都只能显示通用图标。
        ToolKind::Terminal => {
            tools_platform::shell::program_of(target).or_else(|| system_file(target))
        }
    }
}

#[cfg(windows)]
mod sys {
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::path::Path;
    use std::sync::Arc;

    use gpui_kit::RenderImage;
    use windows_sys::Win32::Graphics::Gdi::{
        BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC,
        DeleteObject, GetDIBits, GetObjectW, HGDIOBJ, SelectObject,
    };
    use windows_sys::Win32::UI::Shell::{SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGetFileInfoW};
    use windows_sys::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};

    pub fn load(path: &Path) -> Option<Arc<RenderImage>> {
        use std::os::windows::ffi::OsStrExt as _;

        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let mut info: SHFILEINFOW = unsafe { std::mem::zeroed() };
        let flags = SHGFI_ICON | SHGFI_LARGEICON;
        let ok = unsafe {
            SHGetFileInfoW(
                wide.as_ptr(),
                0,
                &mut info,
                size_of::<SHFILEINFOW>() as u32,
                flags,
            )
        };
        if ok == 0 || info.hIcon.is_null() {
            return None;
        }
        let image = unsafe { icon_to_image(info.hIcon) };
        unsafe { DestroyIcon(info.hIcon) };
        image
    }

    /// `HICON` → RGBA 位图。
    ///
    /// 图标的实际像素在 `hbmColor` 里（32 位带 alpha）；`hbmMask` 是老式图标的透明掩码，
    /// 只有 alpha 全 0 的那类图标才用得上。
    unsafe fn icon_to_image(icon: HICON) -> Option<Arc<RenderImage>> {
        let mut info: ICONINFO = unsafe { std::mem::zeroed() };
        if unsafe { GetIconInfo(icon, &mut info) } == 0 {
            return None;
        }
        let image = if info.hbmColor.is_null() {
            None
        } else {
            unsafe { bitmap_to_image(info.hbmColor, info.hbmMask) }
        };
        if !info.hbmColor.is_null() {
            unsafe { DeleteObject(info.hbmColor as HGDIOBJ) };
        }
        if !info.hbmMask.is_null() {
            unsafe { DeleteObject(info.hbmMask as HGDIOBJ) };
        }
        image
    }

    unsafe fn bitmap_to_image(color: HGDIOBJ, mask: HGDIOBJ) -> Option<Arc<RenderImage>> {
        let mut bitmap: BITMAP = unsafe { std::mem::zeroed() };
        let got = unsafe {
            GetObjectW(
                color,
                size_of::<BITMAP>() as i32,
                &mut bitmap as *mut BITMAP as *mut c_void,
            )
        };
        if got == 0 || bitmap.bmWidth <= 0 || bitmap.bmHeight <= 0 {
            return None;
        }
        let (width, height) = (bitmap.bmWidth, bitmap.bmHeight);

        let mut header = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                // 负高度 = 自上而下，省掉一次行翻转。
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..unsafe { std::mem::zeroed() }
            },
            ..unsafe { std::mem::zeroed() }
        };

        let mut buffer = vec![0u8; (width * height * 4) as usize];
        let device = unsafe { CreateCompatibleDC(std::ptr::null_mut()) };
        if device.is_null() {
            return None;
        }
        let previous = unsafe { SelectObject(device, color) };
        let lines = unsafe {
            GetDIBits(
                device,
                color,
                0,
                height as u32,
                buffer.as_mut_ptr().cast::<c_void>(),
                &mut header,
                DIB_RGB_COLORS,
            )
        };
        unsafe {
            SelectObject(device, previous);
            DeleteDC(device);
        }
        if lines == 0 {
            return None;
        }

        // **不要换成 RGBA**：`GetDIBits` 给的是 BGRA，而 gpui 的 `RenderImage` 也按
        // BGRA 解释（它自己的文档写着 "in BGRA format"）。之前在这儿交换了红蓝，结果
        // 蓝色块的图标全变成黄色。
        //
        // **透明的像素必须留着透明**。上一版把 `alpha == 0` 的像素一律改成 255（本意是
        // 照顾没有 alpha 通道的老图标），结果现代图标的透明边角全变成黑方块。判断「有没有
        // alpha 通道」要看整张图，不能看单个像素。
        let alpha_missing = buffer.chunks_exact(4).all(|pixel| pixel[3] == 0);
        if alpha_missing {
            // 老图标：1 位掩码里为 1 的像素是透明的，拿它补 alpha；掩码也拿不到就整张不透明。
            let filled = unsafe { apply_mask(&mut buffer, width, height, mask) };
            if !filled {
                for pixel in buffer.chunks_exact_mut(4) {
                    pixel[3] = 255;
                }
            }
        }

        let frame = image::ImageBuffer::<image::Rgba<u8>, Vec<u8>>::from_raw(
            width as u32,
            height as u32,
            buffer,
        )?;
        Some(Arc::new(RenderImage::new(vec![image::Frame::new(frame)])))
    }

    /// 用 1 位掩码给老式图标补 alpha。
    unsafe fn apply_mask(buffer: &mut [u8], width: i32, height: i32, mask: HGDIOBJ) -> bool {
        if mask.is_null() {
            return false;
        }
        let mut bitmap: BITMAP = unsafe { std::mem::zeroed() };
        let got = unsafe {
            GetObjectW(
                mask,
                size_of::<BITMAP>() as i32,
                &mut bitmap as *mut BITMAP as *mut c_void,
            )
        };
        if got == 0 {
            return false;
        }
        // 掩码是 1 位/像素，行按 4 字节对齐（32 位）。
        let stride = ((width + 31) / 32 * 4) as usize;
        let mut bits = vec![0u8; stride * height as usize];
        let mut header = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 1,
                biCompression: BI_RGB,
                ..unsafe { std::mem::zeroed() }
            },
            ..unsafe { std::mem::zeroed() }
        };
        let device = unsafe { CreateCompatibleDC(std::ptr::null_mut()) };
        if device.is_null() {
            return false;
        }
        let previous = unsafe { SelectObject(device, mask) };
        let lines = unsafe {
            GetDIBits(
                device,
                mask,
                0,
                height as u32,
                bits.as_mut_ptr().cast::<c_void>(),
                &mut header,
                DIB_RGB_COLORS,
            )
        };
        unsafe {
            SelectObject(device, previous);
            DeleteDC(device);
        }
        if lines == 0 {
            return false;
        }
        for y in 0..height as usize {
            for x in 0..width as usize {
                let bit = bits[y * stride + x / 8] >> (7 - (x % 8)) & 1;
                let alpha = if bit == 1 { 0 } else { 255 };
                let index = (y * width as usize + x) * 4 + 3;
                if let Some(slot) = buffer.get_mut(index) {
                    *slot = alpha;
                }
            }
        }
        true
    }
}

#[cfg(not(windows))]
mod sys {
    use std::path::Path;
    use std::sync::Arc;

    use gpui_kit::RenderImage;

    pub fn load(_path: &Path) -> Option<Arc<RenderImage>> {
        None
    }
}

#[cfg(all(test, windows))]
mod tests {
    use std::path::Path;

    /// 真去问系统要一次图标：能拿到图、字节数对得上。
    ///
    /// 这条不依赖界面，但会把 `SHGetFileInfoW` → `GetDIBits` → RGBA 整条链路跑一遍，
    /// 是唯一能在无人值守下验证「图标到底取到没有」的办法。
    #[test]
    fn the_shell_gives_us_a_bitmap_for_a_system_exe() {
        let path = Path::new(env!("SystemRoot"))
            .join("System32")
            .join("cmd.exe");
        let icon = super::sys::load(&path).expect("cmd.exe 的图标应该拿得到");
        let bytes = icon.as_bytes(0).expect("渲染图里该有像素");
        // 32 位 RGBA，边长至少 16。
        assert!(bytes.len() >= 16 * 16 * 4, "像素太少：{}", bytes.len());
        assert_eq!(bytes.len() % 4, 0);
        // 不是全透明：真图标总有不透明的地方。
        assert!(
            bytes.chunks_exact(4).any(|pixel| pixel[3] > 0),
            "整张图全透明"
        );
        // **也必须留得住透明**：把 `alpha == 0` 的像素涂成不透明时，图标四周会出现黑方块
        // ——上一版就是这么被用户发现的。
        assert!(
            bytes.chunks_exact(4).any(|pixel| pixel[3] < 255),
            "图标该有透明像素，否则边角会变黑"
        );
    }

    #[test]
    fn a_path_that_does_not_exist_yields_nothing() {
        assert!(
            super::target_for(tools_core::settings::ToolKind::File, "C:/nope/nope.exe").is_none()
        );
    }

    #[test]
    fn a_terminal_command_resolves_to_its_program() {
        let path = super::target_for(tools_core::settings::ToolKind::Terminal, "ipconfig /all")
            .expect("ipconfig 该解析成 System32 里的 exe");
        assert!(
            path.file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("ipconfig.exe")),
            "解析成了 {}",
            path.display()
        );
    }

    #[test]
    fn a_system_tool_resolves_through_system32() {
        // `services.msc` 不在 PATH 上，但命令行能直接敲，靠的是 System32。
        let path = super::target_for(tools_core::settings::ToolKind::Terminal, "services.msc")
            .expect("services.msc 该解析到 System32 里的那个文件");
        assert!(
            path.file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("services.msc")),
            "解析成了 {}",
            path.display()
        );
    }

    #[test]
    fn a_command_that_is_not_a_program_has_no_icon() {
        let resolved = super::target_for(tools_core::settings::ToolKind::Terminal, "setlocal");
        assert!(
            resolved.is_none(),
            "setlocal 是内建命令，不该解析出程序，实际得到 {}",
            resolved
                .map(|path| path.display().to_string())
                .unwrap_or_default()
        );
    }
}
