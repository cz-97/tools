//! Native window chrome for host-owned windows.
//!
//! GPUI exposes neither a frameless frame, an exact client size, nor window
//! placement, so the host does that work itself through Win32. Every top-level
//! window the host owns — the launcher panel and the settings window — places
//! itself through these methods instead of writing Win32 in its own binary.
//!
//! Every method is safe to call with a stale handle: it reports `false` (or an
//! empty size) instead of panicking. On non-Windows targets the whole module
//! degrades to no-ops, so callers stay platform-agnostic.

/// An opaque top-level window handle.
///
/// Kept as an integer rather than the platform pointer so it can be stored in
/// GPUI globals and passed between modules without carrying a lifetime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowHandle(usize);

impl WindowHandle {
    /// A handle that never refers to a window.
    pub const INVALID: Self = Self(0);

    /// Whether this handle was ever produced by a real window.
    ///
    /// This is a cheap check: it does not ask the window manager whether the
    /// window still exists. Use [`NativeWindow::is_open`] for that.
    pub fn is_valid(self) -> bool {
        self != Self::INVALID
    }
}

/// A host-owned top-level window, addressed by handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeWindow {
    handle: WindowHandle,
}

impl NativeWindow {
    /// Look up the top-level window whose title matches exactly.
    ///
    /// Titles are how the host re-acquires its own windows: GPUI does not hand
    /// out the native handle, but it does let the host set the title.
    pub fn find(title: &str) -> Option<Self> {
        sys::find(title).map(|handle| Self { handle })
    }

    /// Wrap a handle, returning `None` unless the window still exists.
    pub fn from_handle(handle: WindowHandle) -> Option<Self> {
        (handle.is_valid() && sys::is_open(handle)).then_some(Self { handle })
    }

    /// The raw handle, for handing to other Win32 code.
    pub fn handle(self) -> WindowHandle {
        self.handle
    }

    /// Whether the window still exists.
    pub fn is_open(self) -> bool {
        sys::is_open(self.handle)
    }

    /// Whether the window is currently shown.
    pub fn is_visible(self) -> bool {
        sys::is_visible(self.handle)
    }

    /// Show the window. Returns `false` if it no longer exists.
    pub fn show(self) -> bool {
        sys::show(self.handle)
    }

    /// Hide the window without destroying it. Returns `false` if it no longer exists.
    pub fn hide(self) -> bool {
        sys::hide(self.handle)
    }

    /// Raise the window and give it keyboard focus.
    ///
    /// Windows may refuse the foreground change when another application owns
    /// it; callers should treat `false` as "raise was ignored", not as an error.
    pub fn focus(self) -> bool {
        sys::focus(self.handle)
    }

    /// Whether this window is the one the user is working in.
    ///
    /// A launcher hides itself when another window takes over, and GPUI reports
    /// no activation event, so the host asks the window manager instead.
    pub fn is_foreground(self) -> bool {
        sys::is_foreground(self.handle)
    }

    /// Physical pixels per logical UI unit on the monitor this window is on.
    pub fn scale_factor(self) -> f32 {
        sys::scale_factor(self.handle)
    }

    /// The client area in physical pixels.
    pub fn client_size(self) -> (i32, i32) {
        sys::client_size(self.handle)
    }

    /// The outer window rect in physical pixels, title bar and border included.
    pub fn outer_size(self) -> (i32, i32) {
        sys::outer_size(self.handle)
    }

    /// The window's top-left corner in physical pixels.
    pub fn position(self) -> (i32, i32) {
        sys::position(self.handle)
    }

    /// Remove the native title bar and frame, or restore them.
    ///
    /// Removing the chrome also removes the caption the user would drag to move
    /// the window and the border they would drag to resize it, so the window
    /// becomes a fixed panel. The host is expected to place it explicitly
    /// (see [`Self::apply_panel_chrome`]) and to offer its own way out.
    pub fn set_borderless(self, borderless: bool) -> bool {
        sys::set_borderless(self.handle, borderless)
    }

    /// Resize so the client area measures `width` x `height` logical units.
    ///
    /// Logical units are what the UI is authored in; the conversion to physical
    /// pixels uses this window's own scale factor, so the panel keeps its
    /// apparent size when it opens on a different monitor.
    pub fn set_client_size(self, width: f32, height: f32) -> bool {
        sys::set_client_size(self.handle, width, height)
    }

    /// Centre the window on the monitor it currently sits on, respecting the
    /// monitor's work area so the panel never lands under the taskbar.
    pub fn center_on_monitor(self) -> bool {
        sys::center_on_monitor(self.handle)
    }

    /// Put the window's top edge at `y` physical pixels, centred horizontally
    /// on the monitor it currently sits on.
    ///
    /// `y` is clamped into the monitor's work area, so a remembered position
    /// still lands somewhere visible after the taskbar moves or the display
    /// it was measured on is gone.
    pub fn center_x_at(self, y: i32) -> bool {
        sys::center_x_at(self.handle, y)
    }

    /// Adopt the icon the executable carries as resource `id`.
    ///
    /// The icon resource is what the shell shows in the task bar and in
    /// Alt-Tab; GPUI does not set one, so the host has to. A build without the
    /// resource reports `false` rather than failing.
    pub fn set_icon(self, id: u16) -> bool {
        sys::set_icon(self.handle, id)
    }

    /// The chrome every host panel wants: frameless, exact client size, centred.
    ///
    /// Returns whether all three steps succeeded; a partially applied panel is
    /// still usable, so callers may log a failure and continue.
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
    use windows_sys::Win32::Graphics::Dwm::DwmExtendFrameIntoClientArea;
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Controls::MARGINS;
    use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, GWL_STYLE, GetClientRect, GetForegroundWindow, GetSystemMetrics,
        GetWindowLongPtrW, GetWindowRect, ICON_BIG, ICON_SMALL, IMAGE_ICON, IsWindow,
        IsWindowVisible, LR_DEFAULTCOLOR, LoadImageW, SM_CXICON, SM_CXSMICON, SW_HIDE, SW_SHOW,
        SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SendMessageW,
        SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, ShowWindow, WM_SETICON, WS_CAPTION,
        WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_SYSMENU, WS_THICKFRAME,
    };

    /// The styles that make up the native title bar, its buttons and the
    /// resize border. Clearing all of them leaves a bare client area.
    const CHROME_STYLES: u32 =
        WS_CAPTION | WS_THICKFRAME | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX;

    /// The 96 DPI baseline Windows reports for unscaled displays.
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

    /// The window rect paired with the work area of the monitor it sits on.
    ///
    /// Placement is always relative to the work area rather than the full
    /// screen, otherwise a centred window ends up under the taskbar.
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
        if wanted != current {
            unsafe { SetWindowLongPtrW(window, GWL_STYLE, wanted as isize) };
        }

        // Without extending the DWM frame, a caption-less window still draws a
        // 1px system border and square corners over the client area.
        let margins = if borderless {
            MARGINS {
                cxLeftWidth: -1,
                cxRightWidth: -1,
                cyTopHeight: -1,
                cyBottomHeight: -1,
            }
        } else {
            MARGINS::default()
        };
        unsafe { DwmExtendFrameIntoClientArea(window, &margins) };

        // Styles only take effect once the frame is recalculated.
        let resized = unsafe {
            SetWindowPos(
                window,
                ptr::null_mut(),
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
        };
        resized != 0
    }

    pub fn set_client_size(handle: WindowHandle, width: f32, height: f32) -> bool {
        if !is_open(handle) || width <= 0.0 || height <= 0.0 {
            return false;
        }

        let scale = scale_factor(handle);
        let target_width = (width * scale).round() as i32;
        let target_height = (height * scale).round() as i32;

        // A borderless window keeps an invisible frame that still counts towards
        // the outer rect, so request the client size plus the current delta.
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
        // A work area shorter than the window leaves no room to clamp towards,
        // so the top edge is the only sensible answer.
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

    pub fn set_icon(handle: WindowHandle, id: u16) -> bool {
        if !is_open(handle) {
            return false;
        }
        let window = hwnd(handle);
        let module = unsafe { GetModuleHandleW(ptr::null()) };
        if module.is_null() {
            return false;
        }
        // The resource id travels as a pointer-sized integer, the way the
        // MAKEINTRESOURCE macro spells it.
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
        // A handle that is well-formed but refers to nothing must stay inert
        // rather than reaching into the window manager.
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
