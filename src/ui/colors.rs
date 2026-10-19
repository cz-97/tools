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

/// 次要文字的醒目下限（WCAG 对比度）。半透明面板背后的壁纸颜色不可知，
/// 弱文字按纯白、纯黑两种极端底色校验，达不到就调。
const MUTED_MIN_CONTRAST: f32 = 3.0;
/// 占位性文字的醒目下限，比次要文字松一档，保住「更弱一档」的层次。
const FAINT_MIN_CONTRAST: f32 = 2.2;

/// 壁纸的两种极端假设，醒目度校验用。
const WALLPAPER_WHITE: Rgba = Rgba {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 1.0,
};
const WALLPAPER_BLACK: Rgba = Rgba {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};

/// WCAG 相对亮度（sRGB）。
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

/// WCAG 对比度。
fn contrast(a: Rgba, b: Rgba) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// 把「文字叠在面板染色上」的结果折成一个等效色：RGB 是已叠加的前景，
/// 剩下的 alpha 是留给壁纸的缝隙。醒目度校验用的就是它——
/// 壁纸从缝隙里透进来多少，两种极端底色各算一遍。
fn flatten_over_panel(text: Rgba, panel: Rgba) -> Rgba {
    let residual = (1.0 - panel.a) * (1.0 - text.a);
    let a = 1.0 - residual;
    if a <= 0.0 {
        return Rgba {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        };
    }
    Rgba {
        r: (text.r * text.a + panel.r * panel.a * (1.0 - text.a)) / a,
        g: (text.g * text.a + panel.g * panel.a * (1.0 - text.a)) / a,
        b: (text.b * text.a + panel.b * panel.a * (1.0 - text.a)) / a,
        a,
    }
}

/// 把颜色的相对亮度调到 `target`：等比缩放 RGB，色相不变。
/// 亮度对缩放系数单调但不线性，直接二分（定点迭代会震荡）。
fn with_luminance(c: Rgba, target: f32) -> Rgba {
    let top = 1.0 / c.r.max(c.g).max(c.b).max(1e-6);
    let (mut lo, mut hi) = (0.0f32, top);
    for _ in 0..32 {
        let mid = (lo + hi) / 2.0;
        let probe = Rgba {
            r: c.r * mid,
            g: c.g * mid,
            b: c.b * mid,
            a: c.a,
        };
        if luminance(probe) < target {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let k = (lo + hi) / 2.0;
    Rgba {
        r: c.r * k,
        g: c.g * k,
        b: c.b * k,
        a: c.a,
    }
}

/// 从两种极端底色的对比度要求推出文字亮度的目标值。
/// 文字在每种底色的哪一侧就往哪侧加深/提亮；两侧要求打架时取中点折中。
fn readable_luminance(current: f32, extremes: &[Rgba; 2], min: f32) -> f32 {
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for bg in extremes {
        let lbg = luminance(*bg);
        if current <= lbg {
            // 文字比底深：继续加深，直到对比度够。
            let x = (lbg + 0.05) / min - 0.05;
            if x < 0.0 {
                lo = lo.max(min * (lbg + 0.05) - 0.05);
            } else {
                hi = hi.min(x);
            }
        } else {
            // 文字比底浅：继续提亮，直到对比度够。
            let y = min * (lbg + 0.05) - 0.05;
            if y > 1.0 {
                hi = hi.min((lbg + 0.05) / min - 0.05);
            } else {
                lo = lo.max(y);
            }
        }
    }
    if lo > hi {
        ((lo + hi) / 2.0).clamp(0.0, 1.0)
    } else {
        current.clamp(lo, hi)
    }
}

/// 把画在半透明面板上的弱文字调到够醒目。
///
/// 壁纸颜色不可知，按「染色叠纯白」「染色叠纯黑」两种极端底色校验对比度；
/// 现状两种都过关就原样返回（不透明主题的配色天然协调，不会白改）。
/// 不过关就换成一个不透明的 adjusted 色——文字把面板盖严实之后，
/// 看到的就是它本身，不再受壁纸搅局。两种底色一头偏白一头偏黑时，
/// 调出来的颜色落在两者之间，深壁纸浅壁纸下都读得清。
fn readable_on_panel(text: Hsla, panel: Hsla, extremes: &[Rgba; 2], min: f32) -> Hsla {
    let flattened = flatten_over_panel(Rgba::from(text), Rgba::from(panel));
    if extremes
        .iter()
        .all(|wp| contrast(wp.blend(flattened), *wp) >= min)
    {
        return text;
    }
    let solid = Rgba {
        a: 1.0,
        ..flattened
    };
    let target = readable_luminance(luminance(solid), extremes, min);
    Hsla::from(with_luminance(solid, target))
}

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
    /// 画在 `surface` / `control` 这类底色上的次要文字。
    /// 半透明时卡片本身是深色半透明、再叠一层半透明面板，有效底色比裸面板更暗，
    /// 所以这套文字按「卡片叠面板叠壁纸」的堆叠底色校验，通常不用调亮。
    pub(crate) on_surface_muted: Hsla,
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

    /// 把若干半透明层自底向上叠起来：返回「最上面一层画在前面各层之上」的合成色。
    /// `blend` 的语义是「后者画在前者之上」，所以折叠顺序就是底 → 顶。
    fn stack(bottom: Rgba, layers: &[Rgba]) -> Rgba {
        layers.iter().fold(bottom, |acc, layer| acc.blend(*layer))
    }

    fn from_theme(theme: &Theme, translucent: bool, panel_opacity: u32) -> Self {
        let opacity = panel_opacity as f32 / 100.0;

        // 半透明时面板底色、卡片底色都留出空隙给亚克力，文字醒目度按
        // 「染色叠壁纸极端色」校验；不透明主题底色就是主题底色，天然协调。
        let (muted_foreground, faint, on_surface_muted) = if translucent {
            // 面板染色叠在壁纸上的两种极端底色。
            let tint = Rgba::from(theme.background).alpha(opacity);
            let panel = Hsla::from(tint);
            let panel_extremes = [
                Self::stack(WALLPAPER_WHITE, &[tint]),
                Self::stack(WALLPAPER_BLACK, &[tint]),
            ];

            // 卡片（行底色 / 控件底色）是半透明深色，再叠一层半透明面板，
            // 所以卡片的有效底色 = 卡片色叠在「面板染色叠壁纸」之上。
            let surface = Rgba::from(theme.muted).alpha(opacity);
            let card_extremes = [
                Self::stack(WALLPAPER_WHITE, &[tint, surface]),
                Self::stack(WALLPAPER_BLACK, &[tint, surface]),
            ];

            (
                readable_on_panel(
                    theme.muted_foreground,
                    panel,
                    &panel_extremes,
                    MUTED_MIN_CONTRAST,
                ),
                readable_on_panel(
                    theme.muted_foreground.opacity(FAINT_OPACITY),
                    panel,
                    &panel_extremes,
                    FAINT_MIN_CONTRAST,
                ),
                // 画在卡片上的次要文字：卡片比面板更暗（两层深色叠起来），
                // 反而不容易被壁纸冲淡，所以按卡片自己的堆叠底色校验。
                readable_on_panel(
                    theme.muted_foreground,
                    Hsla::from(surface),
                    &card_extremes,
                    MUTED_MIN_CONTRAST,
                ),
            )
        } else {
            (
                theme.muted_foreground,
                theme.muted_foreground.opacity(FAINT_OPACITY),
                theme.muted_foreground,
            )
        };

        Self {
            // 半透明时卡片也留出空隙给亚克力，和面板一样只是染色的一层。
            surface: if translucent {
                theme.muted.opacity(opacity)
            } else {
                theme.muted
            },
            // 半透明时这一层只负责染色：模糊交给 DWM 的亚克力，见
            // `app::apply_panel_appearance`。染色深浅就是「透明度」那一档。
            panel: if translucent {
                theme.background.opacity(opacity)
            } else {
                theme.background
            },
            // 控件底色同卡片：半透明时一起透出亚克力。
            control: if translucent {
                theme.background.opacity(opacity)
            } else {
                theme.background
            },
            border: theme.border,
            foreground: theme.foreground,
            muted_foreground,
            on_surface_muted,
            accent: theme.accent,
            accent_foreground: theme.accent_foreground,
            hover: theme.foreground.opacity(HOVER_OPACITY),
            accent_tile: theme.accent_foreground.opacity(ACCENT_TILE_OPACITY),
            on_accent_muted: theme.accent_foreground.opacity(ON_ACCENT_MUTED_OPACITY),
            footer_key: theme.foreground.opacity(FOOTER_KEY_OPACITY),
            faint,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 校验 `readable_on_panel` 的产出在 30% 暗色面板 + 两种极端壁纸下都醒目。
    fn assert_readable(text: Hsla, min: f32) {
        let tint = Rgba::from(hex(0x0a0a0a)).alpha(0.3);
        let extremes = [WALLPAPER_WHITE.blend(tint), WALLPAPER_BLACK.blend(tint)];
        let out = readable_on_panel(text, Hsla::from(tint), &extremes, min);
        // 调整过的产出是不透明的，盖住面板后看到的就是它本身；
        // 没调整时原样返回半透明色，要按原路叠出实际看到的颜色。
        let seen = if Rgba::from(out).a >= 1.0 {
            Rgba::from(out)
        } else {
            WALLPAPER_WHITE.blend(flatten_over_panel(Rgba::from(out), tint))
        };
        for wp in extremes {
            assert!(
                contrast(wp.blend(seen), wp) + 1e-3 >= min,
                "contrast {wp:?} vs {seen:?} below {min}"
            );
        }
    }

    #[test]
    fn translucent_dark_theme_strengthens_weak_text() {
        // 30% 暗色面板叠白壁纸：次要文字 #a3a3a3 糊进底色，必须被调到醒目。
        assert_readable(hex(0xa3a3a3), MUTED_MIN_CONTRAST);
        assert_readable(hex(0xa3a3a3).opacity(FAINT_OPACITY), FAINT_MIN_CONTRAST);
    }

    #[test]
    fn opaque_dark_theme_keeps_muted_text() {
        // 不透明深底上的浅字本来就醒目，不该被改动。
        let bg = hex(0x0a0a0a);
        let text = hex(0xa3a3a3);
        let extremes = [Rgba::from(bg), Rgba::from(bg)];
        assert_eq!(
            Rgba::from(readable_on_panel(text, bg, &extremes, MUTED_MIN_CONTRAST)),
            Rgba::from(text)
        );
    }

    #[test]
    fn translucent_dark_card_keeps_muted_text() {
        // 60% 暗色面板：卡片（surface 叠面板）的有效底色比裸面板更暗，
        // 次要文字 #a3a3a3 叠上去对比度本来够，不该被调亮成发白的灰。
        let tint = Rgba::from(hex(0x0a0a0a)).alpha(0.6);
        let surface = Rgba::from(hex(0x262626)).alpha(0.6);
        let card_extremes = [
            UiColors::stack(WALLPAPER_WHITE, &[tint, surface]),
            UiColors::stack(WALLPAPER_BLACK, &[tint, surface]),
        ];
        let out = readable_on_panel(
            hex(0xa3a3a3),
            Hsla::from(surface),
            &card_extremes,
            MUTED_MIN_CONTRAST,
        );
        assert_eq!(Rgba::from(out), Rgba::from(hex(0xa3a3a3)));
    }

    #[test]
    fn with_luminance_hits_target_for_gray() {
        let out = with_luminance(Rgba::from(hex(0xa3a3a3)), 0.1227);
        assert!((luminance(out) - 0.1227).abs() < 1e-3);
    }
}
