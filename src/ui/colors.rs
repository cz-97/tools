//! 亚克力面板的配色——目标是「烟熏玻璃」，不是「把主题色泡进壁纸里」。
//!
//! 只有三条规矩：
//!
//! 1. **面板永远从同一个底色出发**：暗色主题是一点冷调的近黑（`#0f1116`），浅色主题
//!    是近白。透明度滑块只调这一层的浓度，而且**有下限**（[`FROST_FLOOR`]）：
//!    再透也不会淡到被壁纸冲成中灰。面板明度被钉在窄区间里，文字颜色就敢写死，
//!    不用每次渲染去算对比度。
//! 2. **块和线都是纱，不是实心色**：暗色主题叠白纱、浅色主题叠黑纱，浓度直接写在
//!    [`Palette`] 的 rgba 里。纱让亚克力从块里透上来，块才像玻璃上的浮层，
//!    而不是贴上去的黑补丁。
//! 3. **面板外沿有一根 1px 高光边**（`panel_edge`）。这是「玻璃」最关键的暗示：
//!    没有它，再好的配色也只是一块半透明色块。
//!
//! 两层结构：`apply` 把基础色写进当前主题（开关、滑动条这些组件读 `Theme`），
//! [`UiColors`] 在基础色之上加面板浓度给自绘视图用。绘制代码只从 `UiColors` 取色，
//! 不直接读主题、也不写透明度字面量。

use gpui_kit::component::{ActiveTheme as _, Theme};
use gpui_kit::{App, Hsla, Rgba, rgba};
use gpui_tools::settings::Settings;

// ---------------------------------------------------------------------------
// 1. 基础色：要换观感，改这一段
// ---------------------------------------------------------------------------

/// 面板最低浓度。滑块的 30% 映射到这里，100% 映射到全实。
///
/// 这是整套配色唯一的「物理常数」：低于约 0.72，浅色壁纸会把面板冲成中灰，
/// 白字怎么调都发虚——问题不在字色，在面板不够黑。所以宁可让滑块的下半程
/// 变化小一点，也不允许面板掉出这个区间。
const FROST_FLOOR: f32 = 0.6;

/// 面板外沿那 1px 高光比面板本身亮/暗多少。
const EDGE_MIX: f32 = 0.14;

/// 一套配色的具体值，浅色/深色各一份。颜色都带 alpha（纱就是这样来的）。
struct Palette {
    /// 面板底色，半透明时按滑块浓度铺在亚克力上。
    background: u32,
    foreground: u32,
    /// 次要文字（描述、右侧元信息）。
    muted_foreground: u32,
    /// 画在 `surface` 上的次要文字：底比面板亮一档，字要跟着亮一点。
    on_surface_muted: u32,
    /// 占位性文字（「无快捷键」），比次要文字再弱一档。
    faint: u32,
    /// 分隔线、控件描边。
    border: u32,
    /// 图标小块、卡片、页脚按键块的底色：压在面板上的一层纱。
    surface: u32,
    /// 胶囊选择器、步进器的底座，比 `surface` 再淡一档。
    control: u32,
    /// 选中行 / 选中标签。
    accent: u32,
    /// 选中态上的文字。
    accent_foreground: u32,
    /// 选中行里图标小块的底色。
    accent_tile: u32,
    /// 选中态上的次要文字。
    on_accent_muted: u32,
    /// 悬停的淡色叠加，比选中再弱一档。
    hover: u32,
    /// 页脚按键小块上的字。
    footer_key: u32,
    /// 面板外沿那 1px 高光往哪个方向提：暗色主题提亮、浅色主题压暗。
    edge_tint: u32,
}

/// 深色：冷墨玻璃 + 白纱 + 一点冷蓝的选中。
///
/// 底色不带纯黑：`#0f1116` 有一点点蓝，铺在白壁纸上会透出一层冷雾，比中性灰高级。
const DARK: Palette = Palette {
    background: 0x0f1116ff,
    foreground: 0xf6f8fcff,
    muted_foreground: 0xc9d0dcff,
    on_surface_muted: 0xd6dce6ff,
    faint: 0xa5adbbff,
    border: 0xffffff14,
    surface: 0xffffff12,
    control: 0xffffff0a,
    accent: 0x7aa2f71e,
    accent_foreground: 0xffffffff,
    accent_tile: 0xffffff1f,
    on_accent_muted: 0xffffffb8,
    hover: 0xffffff0d,
    footer_key: 0xfffffff0,
    edge_tint: 0xffffffff,
};

/// 浅色：冷白玻璃 + 黑纱 + 同一种冷蓝的选中。
const LIGHT: Palette = Palette {
    background: 0xf6f8fbff,
    foreground: 0x12151bff,
    muted_foreground: 0x3f464dff,
    on_surface_muted: 0x333a42ff,
    faint: 0x5b6169ff,
    border: 0x00000014,
    surface: 0x0000000d,
    control: 0x00000008,
    accent: 0x1f6feb29,
    accent_foreground: 0x0d2440ff,
    accent_tile: 0x00000014,
    on_accent_muted: 0x0d2440c7,
    hover: 0x0000000a,
    footer_key: 0x12151bf0,
    edge_tint: 0x000000ff,
};

/// 按当前主题的深浅取一套基础色。
fn palette(dark: bool) -> Palette {
    if dark { DARK } else { LIGHT }
}

/// 表里的 `0xRRGGBBAA` 换成绘制用的颜色。
fn color(value: u32) -> Hsla {
    Hsla::from(rgba(value))
}

/// 把 `a` 往 `b` 混 `t`，透明度保留 `a` 的（高光边、选中态都是这么来的）。
fn mix_toward(a: Hsla, b: Hsla, t: f32) -> Hsla {
    let (a, b) = (Rgba::from(a), Rgba::from(b));
    Hsla::from(Rgba {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a,
    })
}

/// 把基础色写进当前主题，让组件和自绘视图用同一份颜色。
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
    theme.background = color(palette.background);
    theme.foreground = color(palette.foreground);
    theme.border = color(palette.border);
    theme.muted = color(palette.surface);
    theme.muted_foreground = color(palette.muted_foreground);
    theme.accent = color(palette.accent);
    theme.accent_foreground = color(palette.accent_foreground);
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
    same(theme.background, color(palette.background))
        && same(theme.foreground, color(palette.foreground))
        && same(theme.border, color(palette.border))
        && same(theme.muted, color(palette.surface))
        && same(theme.muted_foreground, color(palette.muted_foreground))
        && same(theme.accent, color(palette.accent))
        && same(theme.accent_foreground, color(palette.accent_foreground))
}

/// 禁用态控件的透明度。不是颜色，但和颜色一样只该有一份。
pub(crate) const DISABLED_OPACITY: f32 = 0.3;

/// 滑块那一档对应面板多浓。
///
/// 30% 就是 [`FROST_FLOOR`]，100% 是全实；中间线性。滑块叫「透明度」，但它真正
/// 控制的是玻璃的薄厚，底子不会薄到让人看不清字。
fn frost(panel_opacity: u32) -> f32 {
    let (low, high) = (Settings::PANEL_OPACITY_MIN, Settings::PANEL_OPACITY_MAX);
    let span = (high - low) as f32;
    let t = (panel_opacity.clamp(low, high) - low) as f32 / span;
    FROST_FLOOR + (1.0 - FROST_FLOOR) * t
}

// ---------------------------------------------------------------------------
// 2. 调色板：绘制代码唯一该用的东西
// ---------------------------------------------------------------------------

/// 面板自定义视图共用的调色板。
#[derive(Clone, Copy)]
pub(crate) struct UiColors {
    /// 面板底色：半透明时已经带上面板浓度。
    pub(crate) panel: Hsla,
    /// 面板外沿的 1px 高光边。
    pub(crate) panel_edge: Hsla,
    /// 图标小块、卡片、页脚按键块的底色（一层纱）。
    pub(crate) surface: Hsla,
    /// 胶囊选择器、步进器的底座。
    pub(crate) control: Hsla,
    pub(crate) border: Hsla,
    pub(crate) foreground: Hsla,
    pub(crate) muted_foreground: Hsla,
    /// 画在 `surface` 上的次要文字。
    pub(crate) on_surface_muted: Hsla,
    /// 选中行 / 选中标签。
    pub(crate) accent: Hsla,
    pub(crate) accent_foreground: Hsla,
    /// 悬停的淡色叠加。
    pub(crate) hover: Hsla,
    /// 选中行里图标小块的底色。
    pub(crate) accent_tile: Hsla,
    /// 选中行上的次要文字。
    pub(crate) on_accent_muted: Hsla,
    /// 页脚按键提示里按键小块上的文字。
    pub(crate) footer_key: Hsla,
    /// 比次要文字再弱一档：占位性的字。
    pub(crate) faint: Hsla,
}

impl UiColors {
    pub(crate) fn current(cx: &App) -> Self {
        let theme = cx.theme();
        let settings = crate::app::stored_settings(cx);
        Self::from_theme(theme, settings.translucent, settings.panel_opacity)
    }

    fn from_theme(theme: &Theme, translucent: bool, panel_opacity: u32) -> Self {
        let palette = palette(theme.is_dark());
        let base = color(palette.background);
        let panel = if translucent {
            base.opacity(frost(panel_opacity))
        } else {
            base
        };
        Self {
            // 高光边不能是「半透明的白」：那样透过它看到的是壁纸，不是面板。
            // 得是「面板色往高光方向调一档」，透明度跟面板一致。
            panel_edge: mix_toward(panel, color(palette.edge_tint), EDGE_MIX),
            panel,
            surface: color(palette.surface),
            control: color(palette.control),
            border: color(palette.border),
            foreground: color(palette.foreground),
            muted_foreground: color(palette.muted_foreground),
            on_surface_muted: color(palette.on_surface_muted),
            accent: color(palette.accent),
            accent_foreground: color(palette.accent_foreground),
            hover: color(palette.hover),
            accent_tile: color(palette.accent_tile),
            on_accent_muted: color(palette.on_accent_muted),
            footer_key: color(palette.footer_key),
            faint: color(palette.faint),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 把 `top` 叠在 `bottom` 上（sRGB 空间，够算对比度用了）。
    fn over(top: Rgba, bottom: Rgba) -> Rgba {
        Rgba {
            r: top.r * top.a + bottom.r * (1.0 - top.a),
            g: top.g * top.a + bottom.g * (1.0 - top.a),
            b: top.b * top.a + bottom.b * (1.0 - top.a),
            a: 1.0,
        }
    }

    fn luminance(c: Rgba) -> f32 {
        fn linear(v: f32) -> f32 {
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * linear(c.r) + 0.7152 * linear(c.g) + 0.0722 * linear(c.b)
    }

    fn contrast(a: Rgba, b: Rgba) -> f32 {
        let (la, lb) = (luminance(a), luminance(b));
        let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
        (hi + 0.05) / (lo + 0.05)
    }

    const PAPER: Rgba = Rgba {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    };

    /// 面板在「最薄的玻璃 + 最亮的壁纸」下的样子，是整套配色的最坏情况。
    fn thinnest_panel(palette: &Palette) -> Rgba {
        over(rgba(palette.background).alpha(FROST_FLOOR), PAPER)
    }

    #[test]
    fn the_slider_never_thins_the_glass_past_the_floor() {
        assert!((frost(Settings::PANEL_OPACITY_MIN) - FROST_FLOOR).abs() < 1e-6);
        assert!((frost(Settings::PANEL_OPACITY_MAX) - 1.0).abs() < 1e-6);
        // 单调：滑块往右只会更实。
        for step in Settings::PANEL_OPACITY_MIN..Settings::PANEL_OPACITY_MAX {
            assert!(frost(step + 1) >= frost(step));
        }
    }

    #[test]
    fn text_stays_readable_on_a_white_wallpaper() {
        for palette in [DARK, LIGHT] {
            let panel = thinnest_panel(&palette);
            for (text, min) in [
                (palette.foreground, 4.5),
                (palette.muted_foreground, 4.5),
                (palette.faint, 3.0),
            ] {
                let seen = over(rgba(text), panel);
                let got = contrast(seen, panel);
                assert!(got >= min, "contrast {got:.2} below {min} on {panel:?}");
            }
        }
    }

    #[test]
    fn text_stays_readable_on_a_black_wallpaper() {
        // 壁纸是深色时面板更暗，浅字只会更清楚——下限在这里只是防呆。
        for palette in [DARK, LIGHT] {
            let panel = over(
                rgba(palette.background).alpha(FROST_FLOOR),
                Rgba {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 1.0,
                },
            );
            let seen = over(rgba(palette.muted_foreground), panel);
            assert!(contrast(seen, panel) >= 4.5);
        }
    }

    #[test]
    fn a_veil_lifts_the_surface_off_the_panel_in_the_dark_theme() {
        // 暗色主题的块是白纱：比面板亮一档，才算「浮层」而不是「补丁」。
        let panel = thinnest_panel(&DARK);
        let surface = over(rgba(DARK.surface), panel);
        assert!(luminance(surface) > luminance(panel));
    }
}
