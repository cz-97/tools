//! 亚克力面板的配色——目标是「烟熏玻璃」，不是「把主题色泡进壁纸里」。
//!
//! 只有三条规矩：
//!
//! 1. **面板永远从同一个底色出发**：暗色主题是一点冷调的近黑（`#1f1f1f`），浅色主题
//!    是近白。透明度滑块只调这一层的浓度，而且**下限由桌面亮度现算**（[`min_frost`]）：
//!    桌面白就自动留够，桌面暗就真的全透。面板明度被钉在「字看得清」的那一侧，
//!    文字颜色就敢写死，不用每次渲染去算对比度。
//! 2. **块和线都是纱，不是实心色**：暗色主题叠白纱、浅色主题叠黑纱，浓度直接写在
//!    [`Palette`] 的 rgba 里。纱让亚克力从块里透上来，块才像玻璃上的浮层，
//!    而不是贴上去的黑补丁。
//!
//! 两层结构：`apply` 把基础色写进当前主题（开关、滑动条这些组件读 `Theme`），
//! [`UiColors`] 在基础色之上加面板浓度给自绘视图用。绘制代码只从 `UiColors` 取色，
//! 不直接读主题、也不写透明度字面量。

use gpui_kit::component::{ActiveTheme as _, Theme};
use gpui_kit::{App, Hsla, Rgba, rgba};
use std::sync::atomic::{AtomicU8, Ordering};
use tools::settings::Settings;
use tools::window::NativeWindow;

// ---------------------------------------------------------------------------
// 1. 基础色：要换观感，改这一段
// ---------------------------------------------------------------------------

/// 面板最低浓度的**保底**部分：滑块 0% 时面板还剩多浓，由桌面亮度现算。
///
/// 半透明面板是叠在桌面上的，合成大致是字节线性：`C = base * a + desk * (1 - a)`。
/// 桌面白（`desk = 255`）、`a = 0.30` 时面板合成到 `#bcbcbc`，白字只剩 1.9:1——
/// 这就是「透明度拉到底就发虚」的全部原因，不是渲染糊，是对比度没了。
///
/// 单靠一个固定下限救不了：桌面是黑的时 0.30 就白瞎（本来可以全透），桌面是白的时
/// 0.30 又远远不够。所以下限改成**按量到的桌面亮度解**，见 [`min_frost`]：
/// 桌面越亮，0% 自动留得越厚；桌面够暗，0% 就是真的全透。
///
/// 这两个数是「白/黑字还能看清」的分界灰阶：
///
/// | 主题 | 字色 | 面板合成必须 | 分界 |
/// | --- | --- | --- | --- |
/// | 深色 | 近白 `#f6f8fc` | 不能亮过 `#70` | 4.6:1 |
/// | 浅色 | 近黑 `#12151b` | 不能暗过 `#7d` | 4.6:1 |
///
/// 取 4.5:1（正文标准）而不是 3:1：面板上不止有大字，说明文字、元信息都在里面。
const DARK_INK_LIMIT: f32 = 112.;
const LIGHT_INK_LIMIT: f32 = 125.;

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

/// 最近一次量到的桌面亮度（`0..=255`）。
///
/// 这是**环境读数**，不是页面状态：面板画在桌面上，桌面多亮决定了面板还能不能更薄，
/// 而它和「当前在哪一页」没有关系，所以放在这里跟调色板做邻居，而不是塞进 `Launcher`。
///
/// 刷新点只有一个：`show_launcher` 每次唤出时量一次（那时窗口还没显示，量到的是干净的
/// 桌面）。渲染路径里不再抓屏——每帧一次 `BitBlt` 是不可接受的。
static BACKDROP: AtomicU8 = AtomicU8::new(255);

/// 量一次面板背后的桌面亮度。抓不到就按「纯白桌面」算，也就是最厚的保底。
///
/// 宁可白留一层，也不要在白桌面上把字糊掉：两个方向的错误代价不对称。
pub(crate) fn measure_backdrop(window: NativeWindow) {
    let lightness = tools::desktop::backdrop_lightness(window).unwrap_or(255.0);
    BACKDROP.store(lightness.round().clamp(0.0, 255.0) as u8, Ordering::Relaxed);
}

/// 当前认定的桌面亮度。
fn backdrop() -> f32 {
    BACKDROP.load(Ordering::Relaxed) as f32
}

/// 面板基色的灰阶当量（`0..=255`）：暗色主题 `#1f1f1f` 是 31，浅色 `#f6f8fb` 是 249。
///
/// 和桌面亮度用同一套口径（三通道均值），合成式才对得上。
fn base_brightness(background: u32) -> f32 {
    let red = (background >> 24) as f32;
    let green = (background >> 16) as f32;
    let blue = (background >> 8) as f32;
    (red + green + blue) / 3.0
}

/// 面板**至少要**多浓，字才不会糊在桌面里（`0..1`）。
///
/// 合成式 `C = base * a + desk * (1 - a)` 对 `a` 是单调的，所以只有两种可能：
/// `a = 0`（全透）本来就看得很清，那就不用保底；否则解出 `C = 分界值` 的那个 `a`。
///
/// - 白桌面 + 深色面板：`a = (112 - 255) / (31 - 255) = 0.64`，合成 `#70`，白字 4.6:1；
/// - 黑桌面 + 深色面板：全透就是黑底白字，保底 0；
/// - 黑桌面 + 浅色面板：`a = (125 - 0) / (249 - 0) = 0.50`，合成 `#7d`，黑字 4.6:1。
fn min_frost(dark: bool, base: f32, desk: f32) -> f32 {
    let limit = if dark { DARK_INK_LIMIT } else { LIGHT_INK_LIMIT };
    let clear_at_zero = if dark { desk <= limit } else { desk >= limit };
    if clear_at_zero {
        return 0.0;
    }
    let denominator = base - desk;
    if denominator.abs() < 1.0 {
        return 1.0;
    }
    ((limit - desk) / denominator).clamp(0.0, 1.0)
}

/// 滑块位置：0% 到 100% 映射成 `0..1`。
fn slider_position(panel_opacity: u32) -> f32 {
    let (low, high) = (Settings::PANEL_OPACITY_MIN, Settings::PANEL_OPACITY_MAX);
    let span = (high - low) as f32;
    if span <= 0.0 {
        return 1.0;
    }
    (panel_opacity.clamp(low, high) - low) as f32 / span
}

/// 滑块那一档对应面板多浓。
///
/// 滑块叫「透明度」，但它真正控制的是玻璃的薄厚，**下限由桌面亮度定**（[`min_frost`]）：
/// 白桌面下 0% 已经是「刚好看得清」的那一档，再薄就是糊字，所以整段行程从保底起步，
/// 100% 是全实。这样滑块的每一格都还有用，不会出现「拖了半天没反应」的死区。
fn frost(dark: bool, base: f32, desk: f32, panel_opacity: u32) -> f32 {
    let floor = min_frost(dark, base, desk);
    floor + (1.0 - floor) * slider_position(panel_opacity)
}

/// 面板越薄，字色往白里混多少（0..1，0 = 不动）。
///
/// 面板薄了，桌面透上来的比例变大，面板本身会变浅；这时还按「深色面板」的字色画，
/// 浅灰字压在浅灰底上就是发虚。浓度越低，次要文字和占位文字越往纯白提。
///
/// 只对深色主题做：浅色主题面板薄了会偏向桌面色，但字是近黑，对比反而更好。
fn text_lift(dark: bool, translucent: bool, frost: f32) -> f32 {
    if !dark || !translucent {
        return 0.0;
    }
    (1.0 - frost).clamp(0.0, 1.0)
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
        let base_level = base_brightness(palette.background);
        let frost = frost(theme.is_dark(), base_level, backdrop(), panel_opacity);
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
    use super::{DARK_INK_LIMIT, LIGHT_INK_LIMIT, frost, min_frost, text_lift};
    use tools::settings::Settings;

    /// 深色主题基色 `#1f1f1f`、浅色主题基色 `#f6f8fb` 的灰阶当量。
    const DARK_BASE: f32 = 0x1f as f32;
    const LIGHT_BASE: f32 = (0xf6 as f32 + 0xf8 as f32 + 0xfb as f32) / 3.0;

    /// `a` 压在 `desk` 上合成出来的灰阶（字节线性，和 GDI 的混合一致）。
    fn composite(base: f32, desk: f32, alpha: f32) -> f32 {
        base * alpha + desk * (1.0 - alpha)
    }

    /// sRGB 灰阶 -> 相对亮度。
    fn luminance(value: f32) -> f32 {
        let c = (value / 255.0).clamp(0.0, 1.0);
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }

    /// 两个灰阶之间的对比度。
    fn contrast(a: f32, b: f32) -> f32 {
        let (first, second) = (luminance(a), luminance(b));
        let (high, low) = if first > second {
            (first, second)
        } else {
            (second, first)
        };
        (high + 0.05) / (low + 0.05)
    }

    #[test]
    fn a_white_desktop_still_keeps_the_panel_dark_enough() {
        // 这就是用户报的那个现象：0% 时面板合成到 #bcbcbc，白字只剩 1.9:1。
        // 现在 0% 会自动留到 #70 一带。
        let alpha = frost(true, DARK_BASE, 255.0, Settings::PANEL_OPACITY_MIN);
        let panel = composite(DARK_BASE, 255.0, alpha);
        assert!(
            contrast(panel, 0xf6 as f32) >= 4.5,
            "0% 时面板合成到 {panel:.0}，白字只有 {:.2}:1",
            contrast(panel, 0xf6 as f32)
        );
        // 也不能厚得离谱：还该看得出是块玻璃。
        assert!(panel >= 0x60 as f32, "0% 时面板 {panel:.0} 太实了");
    }

    #[test]
    fn a_dark_desktop_lets_the_panel_vanish() {
        // 桌面够暗，0% 就是真的全透——这才是「透明度拉到底」该有的样子。
        for desk in [0.0, 20.0, 60.0, 100.0] {
            assert_eq!(min_frost(true, DARK_BASE, desk), 0.0, "桌面 {desk}");
            assert_eq!(frost(true, DARK_BASE, desk, Settings::PANEL_OPACITY_MIN), 0.0);
        }
    }

    #[test]
    fn every_desktop_and_every_notch_stays_readable() {
        // 桌面从纯黑到纯白、滑块从头到尾，两个主题都不能掉出 4.5:1。
        for opacity in Settings::PANEL_OPACITY_MIN..=Settings::PANEL_OPACITY_MAX {
            for desk in (0..=255).step_by(5) {
                let desk = desk as f32;

                let dark_alpha = frost(true, DARK_BASE, desk, opacity);
                let dark_panel = composite(DARK_BASE, desk, dark_alpha);
                let dark_contrast = contrast(dark_panel, 0xf6 as f32);
                assert!(
                    dark_contrast >= 4.4,
                    "深色：桌面 {desk:.0}、滑块 {opacity}% -> 面板 {dark_panel:.0}，白字 {dark_contrast:.2}:1"
                );

                let light_alpha = frost(false, LIGHT_BASE, desk, opacity);
                let light_panel = composite(LIGHT_BASE, desk, light_alpha);
                let light_contrast = contrast(light_panel, 0x12 as f32);
                assert!(
                    light_contrast >= 4.4,
                    "浅色：桌面 {desk:.0}、滑块 {opacity}% -> 面板 {light_panel:.0}，黑字 {light_contrast:.2}:1"
                );
            }
        }
    }

    #[test]
    fn the_floor_never_eats_the_whole_slider() {
        // 有保底也不能让滑块变成一段死区：每一格都得真的更实一点。
        let mut previous = frost(true, DARK_BASE, 255.0, Settings::PANEL_OPACITY_MIN);
        for opacity in Settings::PANEL_OPACITY_MIN..=Settings::PANEL_OPACITY_MAX {
            let current = frost(true, DARK_BASE, 255.0, opacity);
            assert!(current >= previous - 1e-6, "{opacity}% 反而更透了");
            previous = current;
        }
        assert_eq!(frost(true, DARK_BASE, 255.0, Settings::PANEL_OPACITY_MAX), 1.0);
        // 白桌面上保底最厚，暗桌面上没有保底：两端都能到。
        assert!(min_frost(true, DARK_BASE, 255.0) > 0.5);
        assert_eq!(min_frost(true, DARK_BASE, 0.0), 0.0);
    }

    #[test]
    fn the_boundaries_match_the_contrast_targets() {
        // 分界灰阶得真的对应 4.5:1，改常量的人一眼能看出该往哪边调。
        assert!(contrast(DARK_INK_LIMIT, 0xf6 as f32) >= 4.5);
        assert!(contrast(LIGHT_INK_LIMIT, 0x12 as f32) >= 4.5);
    }

    #[test]
    fn text_only_lifts_when_the_panel_is_thin() {
        // 全实面板用原来的字色；越薄提得越多，最多到 1。
        assert_eq!(text_lift(true, true, 1.0), 0.0);
        assert!((text_lift(true, true, 0.0) - 1.0).abs() < 1e-6);
        // 不半透明、或者浅色主题，都不提。
        assert_eq!(text_lift(true, false, 0.5), 0.0);
        assert_eq!(text_lift(false, true, 0.5), 0.0);
    }
}
