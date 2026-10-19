//! Application-owned UI colors and interaction states.

use gpui_kit::component::{ActiveTheme, Theme};
use gpui_kit::{App, Hsla};

/// Shared palette for the launcher's custom views.
#[derive(Clone, Copy)]
pub(crate) struct UiColors {
    pub(crate) surface: Hsla,
    /// 面板自己的底色：半透明时留出空隙给亚克力。
    pub(crate) panel: Hsla,
    pub(crate) control: Hsla,
    pub(crate) border: Hsla,
    pub(crate) foreground: Hsla,
    pub(crate) muted_foreground: Hsla,
    pub(crate) accent: Hsla,
    pub(crate) accent_foreground: Hsla,
    /// Subtle translucent overlay; intentionally weaker than the accent selection fill.
    pub(crate) hover: Hsla,
}

impl UiColors {
    pub(crate) fn current(cx: &App) -> Self {
        let theme = cx.theme();
        let settings = crate::app::stored_settings(cx);
        Self::from_theme(theme, settings.translucent, settings.panel_opacity)
    }

    fn from_theme(theme: &Theme, translucent: bool, panel_opacity: u32) -> Self {
        Self {
            surface: theme.muted,
            // 半透明时这一层只负责染色：模糊交给 DWM 的亚克力，见
            // `app::apply_panel_appearance`。染色深浅就是「透明度」那一档。
            panel: if translucent {
                theme.background.opacity(panel_opacity as f32 / 100.0)
            } else {
                theme.background
            },
            control: theme.background,
            border: theme.border,
            foreground: theme.foreground,
            muted_foreground: theme.muted_foreground,
            accent: theme.accent,
            accent_foreground: theme.accent_foreground,
            hover: theme.foreground.opacity(0.08),
        }
    }
}
