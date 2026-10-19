//! 自家窗口的原生外壳。
//!
//! GPUI 既不给无边框，也不给精确的客户区尺寸，更不负责窗口位置，所以这些活由宿主
//! 自己用 Win32 干。宿主持有的每个顶层窗口都通过这些方法摆放自己，不在业务代码里
//! 直接写 Win32。
//!
//! 每个方法拿一个失效句柄调用都是安全的：返回 `false`（或空尺寸），不会 panic。
//! 非 Windows 目标上整个模块退化成空操作，调用方不必分平台写代码。

/// 顶层窗口的不透明句柄。
///
/// 存成整数而不是平台指针，这样能塞进 GPUI 的全局值、在模块之间传递，也不带生命周期。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowHandle(usize);

impl WindowHandle {
    /// 永远不指向任何窗口的句柄。
    pub const INVALID: Self = Self(0);

    /// 这个句柄是不是由真实窗口产生过。
    ///
    /// 只是便宜的判断：不会去问窗口管理器窗口还在不在，那个用
    /// [`NativeWindow::is_open`]。
    pub fn is_valid(self) -> bool {
        self != Self::INVALID
    }
}

/// 宿主持有的顶层窗口，按句柄寻址。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeWindow {
    handle: WindowHandle,
}

impl NativeWindow {
    /// 按标题精确查找顶层窗口。
    ///
    /// 宿主就是这样重新拿到自己的窗口的：GPUI 不把原生句柄交出来，但允许设标题。
    pub fn find(title: &str) -> Option<Self> {
        sys::find(title).map(|handle| Self { handle })
    }

    /// 包一个句柄，窗口已经不在就返回 `None`。
    pub fn from_handle(handle: WindowHandle) -> Option<Self> {
        (handle.is_valid() && sys::is_open(handle)).then_some(Self { handle })
    }

    /// 原始句柄，转交给别的 Win32 代码用。
    pub fn handle(self) -> WindowHandle {
        self.handle
    }

    /// 窗口还在不在。
    pub fn is_open(self) -> bool {
        sys::is_open(self.handle)
    }

    /// 窗口当前是不是显示着。
    pub fn is_visible(self) -> bool {
        sys::is_visible(self.handle)
    }

    /// 显示窗口；窗口不在了返回 `false`。
    pub fn show(self) -> bool {
        sys::show(self.handle)
    }

    /// 收起窗口但不销毁；窗口不在了返回 `false`。
    pub fn hide(self) -> bool {
        sys::hide(self.handle)
    }

    /// 把窗口抬到前面并给它键盘焦点。
    ///
    /// 别的程序占着前台时 Windows 可能拒绝这次切换；调用方应把 `false` 当成
    /// 「抬到前面这件事被忽略了」，而不是错误。
    pub fn focus(self) -> bool {
        sys::focus(self.handle)
    }

    /// 这个窗口是不是用户正在用的那个。
    ///
    /// 启动器在别的窗口抢走前台时收起，而 GPUI 不报激活事件，所以直接问窗口管理器。
    pub fn is_foreground(self) -> bool {
        sys::is_foreground(self.handle)
    }

    /// 窗口所在显示器上，一个逻辑单位等于多少物理像素。
    pub fn scale_factor(self) -> f32 {
        sys::scale_factor(self.handle)
    }

    /// 客户区尺寸，物理像素。
    pub fn client_size(self) -> (i32, i32) {
        sys::client_size(self.handle)
    }

    /// 窗口外框尺寸，物理像素，含标题栏和边框。
    pub fn outer_size(self) -> (i32, i32) {
        sys::outer_size(self.handle)
    }

    /// 窗口左上角位置，物理像素。
    pub fn position(self) -> (i32, i32) {
        sys::position(self.handle)
    }

    /// 去掉原生标题栏和边框，或者恢复它们。
    ///
    /// 去掉之后用户没有可拖动的标题栏，也没有可拉伸的边框，窗口就变成一块固定面板。
    /// 宿主需要自己摆放它（见 [`Self::apply_panel_chrome`]），并自己给出关闭的方式。
    pub fn set_borderless(self, borderless: bool) -> bool {
        sys::set_borderless(self.handle, borderless)
    }

    /// 调整尺寸，让客户区是 `width` x `height` 个逻辑单位。
    ///
    /// 界面是按逻辑单位写的；换算成物理像素用的是这个窗口自己的缩放比例，所以换一台
    /// 显示器打开时面板看起来还是同样大。
    pub fn set_client_size(self, width: f32, height: f32) -> bool {
        sys::set_client_size(self.handle, width, height)
    }

    /// 居中到当前所在显示器，按工作区算，面板因此不会跑到任务栏底下。
    pub fn center_on_monitor(self) -> bool {
        sys::center_on_monitor(self.handle)
    }

    /// 把窗口顶边放到物理像素 `y`，横向在所在显示器上居中。
    ///
    /// `y` 会被夹进显示器工作区：任务栏挪了位置、或者当初量它的那块屏已经不在了，
    /// 记住的位置也仍然落在看得见的地方。
    pub fn center_x_at(self, y: i32) -> bool {
        sys::center_x_at(self.handle, y)
    }

    /// 采用可执行文件里 id 为 `id` 的图标资源。
    ///
    /// 任务栏和 Alt-Tab 显示的图标来自窗口；GPUI 不设，只能宿主来。没有这个资源
    /// 的构建返回 `false`，不会失败。
    pub fn set_icon(self, id: u16) -> bool {
        sys::set_icon(self.handle, id)
    }

    /// 让 DWM 按 Windows 11 的默认圆角裁窗口。
    ///
    /// 去掉边框之后窗口就是方的，系统默认值本来是不用自己配的：Windows 11 上
    /// `DWMWCP_ROUND` 就是设置里那个「圆角」档的半径（100% 缩放下 8px）。更早的
    /// Windows 不认这个属性，函数返回 `false`，窗口保持方角。
    pub fn round_corners(self) -> bool {
        sys::round_corners(self.handle)
    }

    /// 每块面板都想要的外壳：无边框、精确客户区、居中。
    ///
    /// 返回每一步是不是都成了；只成一部分的面板照样能用，所以调用方可以记一笔日志
    /// 继续往下走。
    pub fn apply_panel_chrome(self, width: f32, height: f32) -> bool {
        let borderless = self.set_borderless(true);
        let sized = self.set_client_size(width, height);
        let centered = self.center_on_monitor();
        borderless && sized && centered
    }
}

#[cfg(windows)]
mod sys {
    use super::WindowHandle;
    use std::mem::size_of;
    use std::ptr;
    use windows_sys::Win32::Foundation::{HWND, RECT};
    use windows_sys::Win32::Graphics::Dwm::{
        DWMNCRP_DISABLED, DWMNCRP_USEWINDOWSTYLE, DWMWA_BORDER_COLOR, DWMWA_COLOR_DEFAULT,
        DWMWA_COLOR_NONE, DWMWA_NCRENDERING_POLICY, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
        DwmSetWindowAttribute,
    };
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, GWL_STYLE, GetClientRect, GetForegroundWindow, GetSystemMetrics,
        GetWindowLongPtrW, GetWindowRect, ICON_BIG, ICON_SMALL, IMAGE_ICON, IsWindow,
        IsWindowVisible, LR_DEFAULTCOLOR, LoadImageW, SM_CXICON, SM_CXSMICON, SW_HIDE, SW_SHOW,
        SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SendMessageW,
        SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, ShowWindow, WM_SETICON, WS_CAPTION,
        WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_SYSMENU, WS_THICKFRAME,
    };

    /// 构成原生标题栏、它那几个按钮和可拉伸边框的样式位。全清掉就只剩客户区。
    const CHROME_STYLES: u32 =
        WS_CAPTION | WS_THICKFRAME | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX;

    /// 不缩放时 Windows 报的 96 DPI 基准。
    const BASE_DPI: f32 = 96.0;

    fn hwnd(handle: WindowHandle) -> HWND {
        handle.0 as HWND
    }

    pub fn find(title: &str) -> Option<WindowHandle> {
        let wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
        let found = unsafe { FindWindowW(ptr::null(), wide.as_ptr()) };
        (!found.is_null()).then_some(WindowHandle(found as usize))
    }

    pub fn is_open(handle: WindowHandle) -> bool {
        handle.is_valid() && unsafe { IsWindow(hwnd(handle)) } != 0
    }

    pub fn is_visible(handle: WindowHandle) -> bool {
        is_open(handle) && unsafe { IsWindowVisible(hwnd(handle)) } != 0
    }

    pub fn show(handle: WindowHandle) -> bool {
        if !is_open(handle) {
            return false;
        }
        unsafe { ShowWindow(hwnd(handle), SW_SHOW) };
        true
    }

    pub fn hide(handle: WindowHandle) -> bool {
        if !is_open(handle) {
            return false;
        }
        unsafe { ShowWindow(hwnd(handle), SW_HIDE) };
        true
    }

    pub fn focus(handle: WindowHandle) -> bool {
        is_open(handle) && unsafe { SetForegroundWindow(hwnd(handle)) } != 0
    }

    pub fn is_foreground(handle: WindowHandle) -> bool {
        is_open(handle) && unsafe { GetForegroundWindow() } == hwnd(handle)
    }

    pub fn scale_factor(handle: WindowHandle) -> f32 {
        if !is_open(handle) {
            return 1.0;
        }
        let dpi = unsafe { GetDpiForWindow(hwnd(handle)) };
        if dpi == 0 { 1.0 } else { dpi as f32 / BASE_DPI }
    }

    fn rect_size(handle: WindowHandle, client: bool) -> (i32, i32) {
        if !is_open(handle) {
            return (0, 0);
        }
        let mut rect = RECT::default();
        let ok = unsafe {
            if client {
                GetClientRect(hwnd(handle), &mut rect)
            } else {
                GetWindowRect(hwnd(handle), &mut rect)
            }
        };
        if ok == 0 {
            (0, 0)
        } else {
            (rect.right - rect.left, rect.bottom - rect.top)
        }
    }

    pub fn client_size(handle: WindowHandle) -> (i32, i32) {
        rect_size(handle, true)
    }

    pub fn outer_size(handle: WindowHandle) -> (i32, i32) {
        rect_size(handle, false)
    }

    pub fn position(handle: WindowHandle) -> (i32, i32) {
        if !is_open(handle) {
            return (0, 0);
        }
        let mut rect = RECT::default();
        if unsafe { GetWindowRect(hwnd(handle), &mut rect) } == 0 {
            (0, 0)
        } else {
            (rect.left, rect.top)
        }
    }

    /// 窗口矩形，连同它所在显示器的工作区。
    ///
    /// 摆放一律相对工作区而不是整块屏，否则居中的窗口会压到任务栏下面。
    fn rect_and_work(handle: WindowHandle) -> Option<(RECT, RECT)> {
        if !is_open(handle) {
            return None;
        }
        let window = hwnd(handle);
        let mut rect = RECT::default();
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        unsafe {
            if GetWindowRect(window, &mut rect) == 0 {
                return None;
            }
            let monitor = MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST);
            if GetMonitorInfoW(monitor, &mut info) == 0 {
                return None;
            }
        }
        Some((rect, info.rcWork))
    }

    pub fn set_borderless(handle: WindowHandle, borderless: bool) -> bool {
        if !is_open(handle) {
            return false;
        }
        let window = hwnd(handle);
        let current = unsafe { GetWindowLongPtrW(window, GWL_STYLE) } as u32;
        let wanted = if borderless {
            current & !CHROME_STYLES
        } else {
            current | CHROME_STYLES
        };
        let resized = if wanted != current {
            unsafe { SetWindowLongPtrW(window, GWL_STYLE, wanted as isize) };

            // 只有样式真的变化时才让系统重算边框。Popup 本身已无边框，
            // 对它额外发送 SWP_FRAMECHANGED 会触发一次多余的 DWM frame 重建。
            unsafe {
                SetWindowPos(
                    window,
                    ptr::null_mut(),
                    0,
                    0,
                    0,
                    0,
                    SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                )
            }
        } else {
            1
        };
        if resized != 0 {
            // 与喝水提醒的透明浮层一致：禁用非客户区绘制并清除系统边框。
            // 整个客户区留给 GPUI 的预乘 alpha 合成，不再通过 ExtendFrame 改写 DWM 表面。
            let policy = if borderless {
                DWMNCRP_DISABLED
            } else {
                DWMNCRP_USEWINDOWSTYLE
            };
            let border_color = if borderless {
                DWMWA_COLOR_NONE
            } else {
                DWMWA_COLOR_DEFAULT
            };
            unsafe {
                let _ = DwmSetWindowAttribute(
                    window,
                    DWMWA_NCRENDERING_POLICY as u32,
                    &policy as *const _ as _,
                    size_of::<i32>() as u32,
                );
                let _ = DwmSetWindowAttribute(
                    window,
                    DWMWA_BORDER_COLOR as u32,
                    &border_color as *const _ as _,
                    size_of::<u32>() as u32,
                );
            }
        }
        resized != 0
    }

    pub fn set_client_size(handle: WindowHandle, width: f32, height: f32) -> bool {
        if !is_open(handle) || width <= 0.0 || height <= 0.0 {
            return false;
        }

        let scale = scale_factor(handle);
        let target_width = (width * scale).round() as i32;
        let target_height = (height * scale).round() as i32;

        // 无边框窗口仍留着一圈看不见的框，它对「外框尺寸」是有贡献的，所以请求的
        // 尺寸要在客户区之上加上这个差值。
        let (outer_width, outer_height) = outer_size(handle);
        let (client_width, client_height) = client_size(handle);
        let frame_width = (outer_width - client_width).max(0);
        let frame_height = (outer_height - client_height).max(0);

        let resized = unsafe {
            SetWindowPos(
                hwnd(handle),
                ptr::null_mut(),
                0,
                0,
                target_width + frame_width,
                target_height + frame_height,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
        };
        resized != 0
    }

    pub fn center_on_monitor(handle: WindowHandle) -> bool {
        let Some((rect, work)) = rect_and_work(handle) else {
            return false;
        };
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        let x = work.left + (work.right - work.left - width) / 2;
        let y = work.top + (work.bottom - work.top - height) / 2;

        unsafe {
            SetWindowPos(
                hwnd(handle),
                ptr::null_mut(),
                x,
                y,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            ) != 0
        }
    }

    pub fn center_x_at(handle: WindowHandle, y: i32) -> bool {
        let Some((rect, work)) = rect_and_work(handle) else {
            return false;
        };
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        let x = work.left + (work.right - work.left - width) / 2;
        // 工作区比窗口还矮时没有可夹的余地，顶边对齐是唯一合理的答案。
        let lowest = (work.bottom - height).max(work.top);
        let y = y.clamp(work.top, lowest);

        unsafe {
            SetWindowPos(
                hwnd(handle),
                ptr::null_mut(),
                x,
                y,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            ) != 0
        }
    }

    pub fn round_corners(handle: WindowHandle) -> bool {
        if !is_open(handle) {
            return false;
        }
        let preference = DWMWCP_ROUND;
        let ok = unsafe {
            DwmSetWindowAttribute(
                hwnd(handle),
                DWMWA_WINDOW_CORNER_PREFERENCE as u32,
                &preference as *const _ as _,
                size_of::<i32>() as u32,
            )
        };
        ok != 0
    }

    pub fn set_icon(handle: WindowHandle, id: u16) -> bool {
        if !is_open(handle) {
            return false;
        }
        let window = hwnd(handle);
        let module = unsafe { GetModuleHandleW(ptr::null()) };
        if module.is_null() {
            return false;
        }
        // 资源 id 以指针宽度的整数传递，就是 MAKEINTRESOURCE 宏拼出来的写法。
        let name = id as usize as *const u16;
        let mut applied = true;
        for (metric, kind) in [(SM_CXICON, ICON_BIG), (SM_CXSMICON, ICON_SMALL)] {
            let edge = unsafe { GetSystemMetrics(metric) };
            let icon = unsafe { LoadImageW(module, name, IMAGE_ICON, edge, edge, LR_DEFAULTCOLOR) };
            if icon.is_null() {
                applied = false;
                continue;
            }
            unsafe { SendMessageW(window, WM_SETICON, kind as usize, icon as isize) };
        }
        applied
    }
}

#[cfg(not(windows))]
mod sys {
    use super::WindowHandle;

    pub fn find(_title: &str) -> Option<WindowHandle> {
        None
    }

    pub fn is_open(_handle: WindowHandle) -> bool {
        false
    }

    pub fn is_visible(_handle: WindowHandle) -> bool {
        false
    }

    pub fn show(_handle: WindowHandle) -> bool {
        false
    }

    pub fn hide(_handle: WindowHandle) -> bool {
        false
    }

    pub fn focus(_handle: WindowHandle) -> bool {
        false
    }

    pub fn is_foreground(_handle: WindowHandle) -> bool {
        false
    }

    pub fn scale_factor(_handle: WindowHandle) -> f32 {
        1.0
    }

    pub fn client_size(_handle: WindowHandle) -> (i32, i32) {
        (0, 0)
    }

    pub fn outer_size(_handle: WindowHandle) -> (i32, i32) {
        (0, 0)
    }

    pub fn position(_handle: WindowHandle) -> (i32, i32) {
        (0, 0)
    }

    pub fn set_borderless(_handle: WindowHandle, _borderless: bool) -> bool {
        false
    }

    pub fn set_client_size(_handle: WindowHandle, _width: f32, _height: f32) -> bool {
        false
    }

    pub fn center_on_monitor(_handle: WindowHandle) -> bool {
        false
    }

    pub fn center_x_at(_handle: WindowHandle, _y: i32) -> bool {
        false
    }

    pub fn set_icon(_handle: WindowHandle, _id: u16) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::{NativeWindow, WindowHandle};

    #[test]
    fn invalid_handle_is_not_valid() {
        assert!(!WindowHandle::INVALID.is_valid());
        assert!(WindowHandle(1).is_valid());
    }

    #[test]
    fn from_handle_rejects_handles_that_do_not_refer_to_a_window() {
        assert!(NativeWindow::from_handle(WindowHandle::INVALID).is_none());
        assert!(NativeWindow::from_handle(WindowHandle(0xdead_beef)).is_none());
    }

    #[test]
    fn unknown_title_is_not_found() {
        assert!(NativeWindow::find("gpui-tools no-such-window").is_none());
    }

    #[test]
    fn operations_on_a_stale_window_report_failure() {
        // 格式合法但不指向任何窗口的句柄必须保持安静，不能拿去调窗口管理器。
        let window = NativeWindow {
            handle: WindowHandle(0xdead_beef),
        };
        assert!(!window.is_open());
        assert!(!window.is_visible());
        assert!(!window.show());
        assert!(!window.hide());
        assert!(!window.focus());
        assert!(!window.is_foreground());
        assert!(!window.set_borderless(true));
        assert!(!window.set_client_size(640.0, 480.0));
        assert!(!window.center_on_monitor());
        assert!(!window.center_x_at(240));
        assert!(!window.set_icon(1));
        assert!(!window.apply_panel_chrome(640.0, 480.0));
        assert_eq!(window.client_size(), (0, 0));
        assert_eq!(window.outer_size(), (0, 0));
        assert_eq!(window.position(), (0, 0));
        assert!(window.scale_factor() > 0.0);
    }
}
