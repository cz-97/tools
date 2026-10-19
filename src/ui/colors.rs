//! 界面用到的所有颜色——**基础色在这里写值**。
//!
//! 结构分三层：
//!
//! 1. **基础色**（`NEUTRAL_*` + `palette()`）：真正写死的值。默认与 gpui-component 的
//!    shadcn neutral 主题一致，所以改之前和改之后长得一样；想换观感就改这里的值。
//!    [`apply`] 会在每次换主题后把这套值写进当前主题，于是组件（开关、滑动条、按钮）
//!    和自绘视图读到的都是同一份。
//! 2. **派生色**（[`UiColors`]）：在基础色上按固定比例调出来的（悬停、选中行的图标
//!    底、次要文字……），比例就是下面的那些 `*_OPACITY` 常量。
//! 3. **半透明染色**（`panel`）：亚克力之上的那层底色，深浅由外观页的「透明度」档控制。
//!
//! 绘制代码只从 `UiColors` 取色，不直接读主题、也不写透明度字面量。

use gpui_kit::component::{ActiveTheme as _, Theme};
use gpui_kit::{App, Hsla, Rgba, rgb};

// ---------------------------------------------------------------------------
// 1. 基础色：要换观感，改这一段
// ---------------------------------------------------------------------------

/// shadcn neutral 色阶，浅色与深色两套共用（sRGB）。
const NEUTRAL_50: u32 = 0xfafafa;
/// #f5f5f5
const NEUTRAL_100: u32 = 0xf5f5f5;
/// #e5e5e5
const NEUTRAL_200: u32 = 0xe5e5e5;
/// #737373
const NEUTRAL_500: u32 = 0x737373;
/// #a3a3a3
const NEUTRAL_400: u32 = 0xa3a3a3;
/// #262626
const NEUTRAL_800: u32 = 0x262626;
/// #171717
const NEUTRAL_900: u32 = 0x171717;
/// #0a0a0a
const NEUTRAL_950: u32 = 0x0a0a0a;
/// 纯白。
const WHITE: u32 = 0xffffff;

/// 一套配色的具体值，浅色/深色各一份。
struct Palette {
    background: u32,
    foreground: u32,
    border: u32,
    /// 行的底色、选中态的底。
    muted: u32,
    /// 次要文字。
    muted_foreground: u32,
    /// 选中高亮。
    accent: u32,
    /// 选中高亮上的文字。
    accent_foreground: u32,
}

/// 浅色配色。
const LIGHT: Palette = Palette {
    background: WHITE,
    foreground: NEUTRAL_950,
    border: NEUTRAL_200,
    muted: NEUTRAL_100,
    muted_foreground: NEUTRAL_500,
    accent: NEUTRAL_100,
    accent_foreground: NEUTRAL_900,
};

/// 深色配色。
const DARK: Palette = Palette {
    background: NEUTRAL_950,
    foreground: NEUTRAL_50,
    border: NEUTRAL_800,
    muted: NEUTRAL_800,
    muted_foreground: NEUTRAL_400,
    accent: NEUTRAL_800,
    accent_foreground: NEUTRAL_50,
};

fn hex(value: u32) -> Hsla {
    Hsla::from(rgb(value))
}

/// 按当前主题的深浅取一套基础色。
fn palette(dark: bool) -> Palette {
    if dark { DARK } else { LIGHT }
}

/// 把基础色写进当前主题。
///
/// `Theme::change` / `sync_system_appearance` 每次都会按内置主题重建颜色，所以换主题、
/// 跟随系统切换之后都要再调一次（见 `app::apply_theme` 与主题观察者）。
///
/// **已经是这套值时就什么都不写。** `global_mut` 每调用一次都会触发一遍全局观察者，
/// 而主题观察者又会调到这里——不拦住的话就是「观察者 → 写主题 → 观察者」的转圈，
/// 面板直接卡死（0.9.1 踩过）。
pub(crate) fn apply(cx: &mut App) {
    let palette = palette(cx.theme().is_dark());
    if matches(cx.theme(), &palette) {
        return;
    }
    let theme = cx.global_mut::<Theme>();
    theme.background = hex(palette.background);
    theme.foreground = hex(palette.foreground);
    theme.border = hex(palette.border);
    theme.muted = hex(palette.muted);
    theme.muted_foreground = hex(palette.muted_foreground);
    theme.accent = hex(palette.accent);
    theme.accent_foreground = hex(palette.accent_foreground);
}

/// 主题里的颜色是不是已经等于这套调色板。
///
/// HSL 来回换算有浮点误差，按分量比，留一点容差。
fn matches(theme: &Theme, palette: &Palette) -> bool {
    fn same(a: Hsla, b: Hsla) -> bool {
        let (a, b) = (Rgba::from(a), Rgba::from(b));
        (a.r - b.r).abs() < 1e-4
            && (a.g - b.g).abs() < 1e-4
            && (a.b - b.b).abs() < 1e-4
            && (a.a - b.a).abs() < 1e-4
    }
    same(theme.background, hex(palette.background))
        && same(theme.foreground, hex(palette.foreground))
        && same(theme.border, hex(palette.border))
        && same(theme.muted, hex(palette.muted))
        && same(theme.muted_foreground, hex(palette.muted_foreground))
        && same(theme.accent, hex(palette.accent))
        && same(theme.accent_foreground, hex(palette.accent_foreground))
}

// ---------------------------------------------------------------------------
// 2. 派生色：比例常量
// ---------------------------------------------------------------------------

/// 悬停叠加的强度。刻意比选中的高亮弱一档。
const HOVER_OPACITY: f32 = 0.08;
/// 选中行里图标小块底色的强度。
const ACCENT_TILE_OPACITY: f32 = 0.14;
/// 选中行上次要文字的强度。
const ON_ACCENT_MUTED_OPACITY: f32 = 0.7;
/// 页脚按键小块上文字的强度。
const FOOTER_KEY_OPACITY: f32 = 0.85;
/// 占位性文字（「无快捷键」）的强度：比次要文字再弱一档。
const FAINT_OPACITY: f32 = 0.7;
/// 禁用态控件的透明度。不是颜色，但和颜色一样只该有一份。
pub(crate) const DISABLED_OPACITY: f32 = 0.3;

// ---------------------------------------------------------------------------
// 3. 调色板：绘制代码唯一该用的东西
// ---------------------------------------------------------------------------

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
            hover: theme.foreground.opacity(HOVER_OPACITY),
            accent_tile: theme.accent_foreground.opacity(ACCENT_TILE_OPACITY),
            on_accent_muted: theme.accent_foreground.opacity(ON_ACCENT_MUTED_OPACITY),
            footer_key: theme.foreground.opacity(FOOTER_KEY_OPACITY),
            faint: theme.muted_foreground.opacity(FAINT_OPACITY),
        }
    }
}
