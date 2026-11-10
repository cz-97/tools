//! 面板背后那片桌面有多亮。
//!
//! 半透明面板的底色是**叠在桌面上**的：桌面白，面板合成出来就浅；浅到一定程度，
//! 白字掉到 4.5:1 以下，于是「透明度拉到底就发虚」。要保证不论桌面多亮字都清楚，
//! 就得先知道桌面有多亮——这个模块负责量它。
//!
//! 做法是抓窗口外一圈屏幕像素（面板自己不在这一圈里），取平均。取平均而不是取最亮，
//! 是因为亚克力本来就把背后的画面糊开了：视觉上的「底色」就是邻近像素的平均值。

use super::window::NativeWindow;

/// 采样环的宽度（物理像素）：够宽才不被窗口阴影和圆角吃掉，又不至于读整块屏幕。
const RING: i32 = 96;

/// 面板背后那片桌面的平均亮度，`0..=255`（三通道均值，即灰阶当量）。
///
/// 传正在显示（或即将显示）的面板窗口：它用来把面板自己从那圈像素里挖掉。
/// 抓不到（没有桌面、GDI 失败、非 Windows）就返回 `None`，调用方按最坏情况处理。
pub fn backdrop_lightness(window: NativeWindow) -> Option<f32> {
    let (x, y) = window.position();
    let (width, height) = window.outer_size();
    around((x, y, width, height))
}

/// 给定面板矩形 `(x, y, 宽, 高)`，量它四周一圈桌面的平均亮度。
///
/// 和 [`backdrop_lightness`] 是同一件事，只是不需要真窗口——探针和测试走这条。
#[cfg(windows)]
pub fn around(rect: (i32, i32, i32, i32)) -> Option<f32> {
    use windows_sys::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC,
        DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, SRCCOPY, SelectObject,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN,
    };

    let (x, y, width, height) = rect;
    if width <= 0 || height <= 0 {
        return None;
    }

    // SAFETY: 全是只读的 GDI 调用；每张拿到的 DC / 位图都在下面配平释放，
    // 缓冲区长度按 `width * height * 4` 给足，`GetDIBits` 不会写越界。
    unsafe {
        let (vx, vy) = (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
        );
        let (vw, vh) = (
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        );
        if vw <= 0 || vh <= 0 {
            return None;
        }

        // 面板四周那一圈，夹在虚拟屏幕里（多屏时负坐标也成立）。
        let left = (x - RING).max(vx);
        let top = (y - RING).max(vy);
        let right = (x + width + RING).min(vx + vw);
        let bottom = (y + height + RING).min(vy + vh);
        let (grab_width, grab_height) = (right - left, bottom - top);
        if grab_width <= 0 || grab_height <= 0 {
            return None;
        }

        let screen = GetDC(std::ptr::null_mut());
        if screen.is_null() {
            return None;
        }
        let memory = CreateCompatibleDC(screen);
        if memory.is_null() {
            ReleaseDC(std::ptr::null_mut(), screen);
            return None;
        }
        let bitmap = CreateCompatibleBitmap(screen, grab_width, grab_height);
        if bitmap.is_null() {
            DeleteDC(memory);
            ReleaseDC(std::ptr::null_mut(), screen);
            return None;
        }
        let previous = SelectObject(memory, bitmap);
        let copied =
            BitBlt(memory, 0, 0, grab_width, grab_height, screen, left, top, SRCCOPY) != 0;

        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = grab_width;
        // 负高度 = 自上而下，第 0 行就是屏幕最上面一行，省得算翻转。
        info.bmiHeader.biHeight = -grab_height;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;

        let mut pixels = vec![0u8; (grab_width as usize) * (grab_height as usize) * 4];
        let rows = if copied {
            GetDIBits(
                memory,
                bitmap,
                0,
                grab_height as u32,
                pixels.as_mut_ptr().cast(),
                &mut info,
                DIB_RGB_COLORS,
            )
        } else {
            0
        };

        SelectObject(memory, previous);
        DeleteObject(bitmap);
        DeleteDC(memory);
        ReleaseDC(std::ptr::null_mut(), screen);

        if rows == 0 {
            return None;
        }

        Some(average_brightness(
            &pixels,
            grab_width,
            grab_height,
            rect,
            (left, top),
        ))
    }
}

/// 求那圈像素的平均亮度，顺手跳过落在面板里的像素（阴影、圆角会漏进来一点）。
///
/// 每 2 个像素取一个：一圈 96px 宽的边框已经有一万多个采样点，再密就是浪费时间。
#[cfg(windows)]
fn average_brightness(
    pixels: &[u8],
    grab_width: i32,
    grab_height: i32,
    panel: (i32, i32, i32, i32),
    origin: (i32, i32),
) -> f32 {
    let (panel_x, panel_y, panel_width, panel_height) = panel;
    let (origin_x, origin_y) = origin;
    // 面板边缘往里收 4px：外边那几像素可能是窗口自己的阴影。
    let inner_left = panel_x + 4;
    let inner_top = panel_y + 4;
    let inner_right = panel_x + panel_width - 4;
    let inner_bottom = panel_y + panel_height - 4;

    let mut total: u64 = 0;
    let mut count: u64 = 0;
    for row in (0..grab_height).step_by(2) {
        for column in (0..grab_width).step_by(2) {
            let screen_x = origin_x + column;
            let screen_y = origin_y + row;
            if screen_x >= inner_left
                && screen_x < inner_right
                && screen_y >= inner_top
                && screen_y < inner_bottom
            {
                continue;
            }
            let offset = ((row as usize) * (grab_width as usize) + column as usize) * 4;
            // 32bpp BI_RGB 的字节序是 B, G, R, A。
            let blue = pixels[offset] as u32;
            let green = pixels[offset + 1] as u32;
            let red = pixels[offset + 2] as u32;
            total += ((red + green + blue) / 3) as u64;
            count += 1;
        }
    }

    if count == 0 {
        return 0.0;
    }
    total as f32 / count as f32
}

/// 非 Windows：没有可量的桌面，交给调用方按最坏情况处理。
#[cfg(not(windows))]
pub fn around(_rect: (i32, i32, i32, i32)) -> Option<f32> {
    None
}

#[cfg(all(test, windows))]
mod tests {
    use super::average_brightness;

    /// 造一块纯色画布。
    fn solid(value: u8, width: i32, height: i32) -> Vec<u8> {
        let mut pixels = vec![0u8; (width as usize) * (height as usize) * 4];
        for chunk in pixels.chunks_exact_mut(4) {
            chunk[0] = value;
            chunk[1] = value;
            chunk[2] = value;
            chunk[3] = 255;
        }
        pixels
    }

    #[test]
    fn a_white_wallpaper_reads_as_white() {
        let pixels = solid(255, 40, 40);
        let value = average_brightness(&pixels, 40, 40, (0, 0, 0, 0), (0, 0));
        assert!((value - 255.0).abs() < 0.5, "读到 {value}");
    }

    #[test]
    fn the_panel_area_is_skipped() {
        // 面板那块（含 4px 内缩）涂白、四周涂黑：要是没跳过面板，读数会被白块拉高。
        let (width, height) = (40i32, 40i32);
        let mut pixels = vec![0u8; (width as usize) * (height as usize) * 4];
        for row in 0..height {
            for column in 0..width {
                let offset = ((row as usize) * (width as usize) + column as usize) * 4;
                if (14..26).contains(&column) && (14..26).contains(&row) {
                    pixels[offset] = 255;
                    pixels[offset + 1] = 255;
                    pixels[offset + 2] = 255;
                }
                pixels[offset + 3] = 255;
            }
        }
        let value = average_brightness(&pixels, width, height, (10, 10, 20, 20), (0, 0));
        assert_eq!(value, 0.0);
    }

    #[test]
    fn channels_are_averaged_not_just_red() {
        // 纯蓝：三通道均值是 85，只看红色通道会是 0。
        let mut pixels = vec![0u8; 16 * 16 * 4];
        for chunk in pixels.chunks_exact_mut(4) {
            chunk[0] = 255;
        }
        let value = average_brightness(&pixels, 16, 16, (0, 0, 0, 0), (0, 0));
        assert!((value - 85.0).abs() < 0.5, "读到 {value}");
    }
}
