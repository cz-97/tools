//! Best-effort Windows native menu appearance matching the in-app theme.

use gpui_tools::settings::Theme;

/// Set Windows' preferred app-menu appearance when the OS exposes the API.
/// Older Windows builds keep their native system menu appearance.
pub(crate) fn apply(theme: Theme) {
    #[cfg(windows)]
    {
        use std::mem::transmute;

        use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
        use windows_sys::core::w;

        let mode = match theme {
            Theme::System => 1, // AllowDark: let Windows choose based on its appearance.
            Theme::Dark => 2,   // ForceDark.
            Theme::Light => 3,  // ForceLight.
        };
        // Windows exposes SetPreferredAppMode from uxtheme by ordinal 135. tray-icon uses
        // the same process-wide preference for its native popup menus; gpui-tray's HMENU
        // menus use the Windows renderer too, so no tray backend replacement is needed.
        // Resolve dynamically so older Windows versions keep their normal system menus.
        unsafe {
            let module = LoadLibraryW(w!("uxtheme.dll"));
            if module.is_null() {
                return;
            }
            let Some(procedure) = GetProcAddress(module, 135usize as *const u8) else {
                return;
            };
            let set_preferred_app_mode: unsafe extern "system" fn(usize) -> usize =
                transmute(procedure);
            set_preferred_app_mode(mode);
        }
    }

    #[cfg(not(windows))]
    let _ = theme;
}
