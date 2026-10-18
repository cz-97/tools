//! Application-owned UI colors and interaction states.

use gpui_kit::component::{ActiveTheme, Theme};
use gpui_kit::{App, Hsla};

/// Shared palette for the launcher's custom views.
#[derive(Clone, Copy)]
pub(crate) struct UiColors {
    pub(crate) surface: Hsla,
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
        Self::from_theme(theme, crate::app::stored_settings(cx).translucent)
    }

    fn from_theme(theme: &Theme, translucent: bool) -> Self {
        Self {
            surface: theme.muted,
            control: if translucent {
                theme.background.opacity(0.85)
            } else {
                theme.background
            },
            border: theme.border,
            foreground: theme.foreground,
            muted_foreground: theme.muted_foreground,
            accent: theme.accent,
            accent_foreground: theme.accent_foreground,
            hover: theme.foreground.opacity(0.08),
        }
    }
}
