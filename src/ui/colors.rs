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
//!
//! 两层结构：`apply` 把基础色写进当前主题（开关、滑动条这些组件读 `Theme`），
//! [`UiColors`] 在基础色之上加面板浓度给自绘视图用。绘制代码只从 `UiColors` 取色，
//! 不直接读主题、也不写透明度字面量。

use gpui_kit::component::{ActiveTheme as _, Theme};
use gpui_kit::{App, Hsla, Rgba, rgba};
use tools::settings::Settings;

// ---------------------------------------------------------------------------
// 1. 基础色：要换观感，改这一段
// ---------------------------------------------------------------------------

/// 面板最低浓度。滑块的 0% 映射到这里，100% 映射到全实。
///
/// 这个值直接决定白壁纸透上来多少：浓度乘在面板色上，剩下的亮度全由壁纸补。
/// 我们面板基色是 `#1f1f1f`，合成到白底上：
///
/// | 浓度 | 白底合成 | 白字对比 | 次要字对比 |
/// | --- | --- | --- | --- |
/// | 0.50 | `#8f8f8f` | 3.2 | 2.1 |
/// | 0.65 | `#6d6d6d` | 5.2 | 3.3 |
///
/// 0.5 那一行就是「发虚」：白字只有 3.2，次要文字掉到 2.1，浅灰字压在浅灰底上。
/// 参照另一款同类软件在白壁纸上的实测值，面板正好是 `#6d6d6d`（白字 5.2、次要字 3.3），
/// 也就是说它的「薄」其实是 65% 浓度，而不是 50%。
///
/// 取 0.30 是让滑块中点落在 0.65（`0.30 + 0.70 * 0.5`）：用户把滑块放在中间，
/// 看到的就是那种「薄但不虚」的面板；再往右只会更实，往左才是真正的极透。
const FROST_FLOOR: f32 = 0.30;

/// 面板外沿那 1px 高光比面板本身亮/暗多少。
///
/// **不能是 0**：0 等于「高光 = 面板色」，那条线画了也看不见，玻璃的顶边就没了。
/// 8% 是暗色提亮、浅色压暗都还能读出「一条细边」的量，再多就变成描边。
const EDGE_MIX: f32 = 0.08;

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

/// 深色：冷墨玻璃 + 白纱 + 中性灰的选中。
///
/// 底色不带纯黑：`#0f1116` 有一点点蓝，铺在白壁纸上会透出一层冷雾，比中性灰高级。
const DARK: Palette = Palette {
    background: 0x1f1f1fff,
    foreground: 0xf6f8fcff,
    muted_foreground: 0xc9d0dcff,
    on_surface_muted: 0xd6dce6ff,
    faint: 0xa5adbbff,
    border: 0xffffff14,
    surface: 0xffffff12,
    control: 0xffffff0a,
    // 选中块是这一屏唯一的重色，浓度按「在 #1f1f1f 上能被一眼认出来、又不至于像按钮」定。
    // 12% 时变化量不到一档灰，光标停在哪一行读不出来。
    //
    // 色调一律走中性灰：整块界面没有第二种色相，选中只靠明度说话。
    accent: 0xffffff33,
    accent_foreground: 0xffffffff,
    accent_tile: 0xffffff1f,
    on_accent_muted: 0xffffffb8,
    // 悬停比选中再弱两档，但得跟「没悬停」分得开：5% 在深色面板上等于没有。
    hover: 0xffffff17,
    footer_key: 0xfffffff0,
    edge_tint: 0xffffffff,
};

/// 浅色：冷白玻璃 + 黑纱 + 同一种中性灰的选中。
const LIGHT: Palette = Palette {
    background: 0xf6f8fbff,
    foreground: 0x12151bff,
    muted_foreground: 0x3f464dff,
    on_surface_muted: 0x333a42ff,
    faint: 0x5b6169ff,
    border: 0x00000014,
    surface: 0x0000000d,
    control: 0x00000008,
    accent: 0x0000001f,
    accent_foreground: 0x12151bff,
    accent_tile: 0x00000014,
    on_accent_muted: 0x3f464dff,
    hover: 0x00000012,
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
/// 滑块叫「透明度」，但它真正控制的是玻璃的薄厚：浓度低到一定值以下，白壁纸会
/// 把面板冲成浅灰，字就糊了。所以 0% 落在 [`FROST_FLOOR`]，100% 是全实，中间线性。
/// 滑块中点（50%）正好是 `0.65`——白底合成 `#6d6d6d`，白字对比 5.2。
fn frost(panel_opacity: u32) -> f32 {
    let (low, high) = (Settings::PANEL_OPACITY_MIN, Settings::PANEL_OPACITY_MAX);
    let span = (high - low) as f32;
    let t = (panel_opacity.clamp(low, high) - low) as f32 / span;
    FROST_FLOOR + (1.0 - FROST_FLOOR) * t
}

/// 面板越薄，字色往白里混多少（0..1，0 = 不动）。
///
/// 面板薄了，白壁纸透上来的比例变大，面板本身会变浅；这时还按「深色面板」的字色画，
/// 浅灰字压在浅灰底上就是发虚。浓度越低，次要文字和占位文字越往纯白提。
///
/// 只对深色主题做：浅色主题面板薄了会偏向壁纸色，但字是近黑，对比反而更好。
fn text_lift(dark: bool, translucent: bool, frost: f32) -> f32 {
    if !dark || !translucent {
        return 0.0;
    }
    ((1.0 - frost) / (1.0 - FROST_FLOOR)).clamp(0.0, 1.0)
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
        let frost = frost(panel_opacity);
        let panel = if translucent {
            base.opacity(frost)
        } else {
            base
        };
        // 面板薄的时候，字要跟着提纯，见 `text_lift`。
        let lift = text_lift(theme.is_dark(), translucent, frost);
        let lift_to = |value: u32, amount: f32| {
            mix_toward(color(value), color(0xffffffff), lift * amount)
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
            muted_foreground: lift_to(palette.muted_foreground, 0.45),
            on_surface_muted: lift_to(palette.on_surface_muted, 0.40),
            accent: color(palette.accent),
            accent_foreground: color(palette.accent_foreground),
            hover: color(palette.hover),
            accent_tile: color(palette.accent_tile),
            on_accent_muted: lift_to(palette.on_accent_muted, 0.35),
            footer_key: color(palette.footer_key),
            faint: lift_to(palette.faint, 0.45),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FROST_FLOOR, frost, text_lift};
    use tools::settings::Settings;

    /// 面板叠在纯白壁纸上之后是什么亮度（0..=255）。
    ///
    /// 只看 `#1f1f1f` 这一档基色就够：深浅两个主题里只有深色面板需要关心这件事。
    fn on_white(opacity: u32) -> f32 {
        let alpha = frost(opacity);
        let base = 0x1f as f32;
        base * alpha + 255.0 * (1.0 - alpha)
    }

    #[test]
    fn the_slider_midpoint_lands_on_the_reference_panel() {
        // 参照软件在白壁纸上的面板是 #6d6d6d；滑块放在中间就该是那个观感，
        // 否则白字只剩 3.2 的对比，看着发虚。
        let mid = on_white((Settings::PANEL_OPACITY_MIN + Settings::PANEL_OPACITY_MAX) / 2);
        assert!(
            (mid - 0x6d as f32).abs() <= 6.0,
            "滑块中点合成到 {mid:.0}，期望接近 109(#6d)"
        );
    }

    #[test]
    fn more_transparent_never_makes_the_panel_darker() {
        let mut previous = on_white(Settings::PANEL_OPACITY_MIN);
        for opacity in Settings::PANEL_OPACITY_MIN..=Settings::PANEL_OPACITY_MAX {
            let current = on_white(opacity);
            assert!(
                current <= previous + 0.001,
                "滑块往右调（{opacity}%）反而更亮了：{previous:.1} -> {current:.1}"
            );
            previous = current;
        }
    }

    #[test]
    fn the_floor_is_reachable_and_still_above_zero() {
        // 0% 是「最薄」，但绝不能到全透明：那等于没有面板，也谈不上字。
        assert!(FROST_FLOOR > 0.0);
        assert!((frost(Settings::PANEL_OPACITY_MIN) - FROST_FLOOR).abs() < 1e-6);
        assert!((frost(Settings::PANEL_OPACITY_MAX) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn text_only_lifts_when_the_panel_is_thin() {
        // 全实面板用原来的字色；越薄提得越多，最多到 1。
        assert_eq!(text_lift(true, true, 1.0), 0.0);
        assert!((text_lift(true, true, FROST_FLOOR) - 1.0).abs() < 1e-6);
        // 不半透明、或者浅色主题，都不提。
        assert_eq!(text_lift(true, false, 0.5), 0.0);
        assert_eq!(text_lift(false, true, 0.5), 0.0);
    }
}
