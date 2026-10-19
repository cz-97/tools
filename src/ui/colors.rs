//! 界面用到的所有颜色。
//!
//! 和文案一样集中一处：绘制代码只从 `UiColors` 取色，不直接读主题、也不写透明度
//! 字面量。要调色只动这个文件。

use gpui_kit::component::{ActiveTheme, Theme};
use gpui_kit::{App, Hsla};

/// 禁用态控件的透明度。不是颜色，但和颜色一样只该有一份。
pub(crate) const DISABLED_OPACITY: f32 = 0.3;

/// 面板自定义视图共用的调色板。
#[derive(Clone, Copy)]
pub(crate) struct UiColors {
    /// 行的底色（命令页的卡片）。
    pub(crate) surface: Hsla,
    /// 面板自己的底色：半透明时留出空隙给亚克力。
    pub(crate) panel: Hsla,
    /// 行内控件的底色，让它和整行读成一体。
    pub(crate) control: Hsla,
    pub(crate) border: Hsla,
    pub(crate) foreground: Hsla,
    pub(crate) muted_foreground: Hsla,
    pub(crate) accent: Hsla,
    pub(crate) accent_foreground: Hsla,
    /// 悬停的淡色叠加。刻意比选中的高亮弱一档。
    pub(crate) hover: Hsla,
    /// 选中行里图标小块的底色。
    pub(crate) accent_tile: Hsla,
    /// 选中行上的次要文字（比 `accent_foreground` 弱一档）。
    pub(crate) on_accent_muted: Hsla,
    /// 页脚按键提示里按键小块上的文字。
    pub(crate) footer_key: Hsla,
    /// 比次要文字再弱一档：占位性的字（比如「无快捷键」）。
    pub(crate) faint: Hsla,
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
            accent_tile: theme.accent_foreground.opacity(0.14),
            on_accent_muted: theme.accent_foreground.opacity(0.7),
            footer_key: theme.foreground.opacity(0.85),
            faint: theme.muted_foreground.opacity(0.7),
        }
    }
}
