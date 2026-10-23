//! 面板的整体绘制：页头、页脚，以及三个页面的骨架。
//!
//! 从 launcher 的行为里拆出来：这里只管长什么样，页面切换和按键逻辑都在上级模块。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::list::List;
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, h_flex};
use gpui_kit::{
    AnyElement, Context, Font, FontWeight, IntoElement, Pixels, Render, TextRun, Window,
    WindowControlArea, div, prelude::*, px,
};
use gpui_tools::settings::Settings;

use crate::app::stored_settings;
use crate::strings;
use crate::ui::calculator_page;
use crate::ui::colors::UiColors;
use crate::ui::settings_page;

use super::{
    CORNER_RADIUS, HEADER_MIN_HEIGHT, HEADER_SLACK, Launcher, Page, SEARCH_FIELD_CARET,
    SEARCH_FIELD_MAX, SEARCH_FIELD_PADDING, SEARCH_FIELD_SLACK, SEARCH_TEXT_SIZE,
};

impl Launcher {
    /// 页头，同时也是拖动面板的手柄。
    ///
    /// 命令页里放搜索框和进入设置的按钮；设置页里换成返回按钮。两种情况下都只把
    /// 真正需要鼠标的控件排除在拖动区之外：输入框正好和文字一样宽，空着的整条
    /// 搜索栏（连提示一起）都能拖。
    fn header(&self, window: &Window, cx: &Context<Self>) -> AnyElement {
        let colors = UiColors::current(cx);
        let muted_fg = colors.muted_foreground;
        // 按输入框里的文字量宽度，而不是已提交的搜索词：输入法组词时缓冲里已经有字，
        // 按搜索词量的话，那串字会被滚到左边看不见。
        let field_text = self.search.read(cx).value().to_string();
        let settings = stored_settings(cx);
        let field_height = px(settings.search_height as f32);

        let bar = div()
            .id("panel-bar")
            .window_control_area(WindowControlArea::Drag)
            .flex()
            .items_center()
            .gap_2()
            .h(header_height(settings))
            .px_3()
            .border_b_1()
            .border_color(colors.border);

        match self.page {
            Page::Commands => bar
                .child(Icon::new(IconName::Search).size_4().text_color(muted_fg))
                .child(
                    div()
                        .id("search-field")
                        .occlude()
                        .flex_shrink_0()
                        .w(search_field_width(window, cx, &field_text))
                        .h(field_height)
                        .child(
                            // 用小号，不用默认中号：中号的上下内边距共 16px，比行框剩下的
                            // 高度还多，每个字的下半截都会被裁掉。
                            Input::new(&self.search)
                                .small()
                                .appearance(false)
                                .cleanable(false),
                        ),
                )
                .when(field_text.is_empty(), |this| {
                    // 提示属于拖动区，不属于输入框：它是让人一边拖一边读的。
                    this.child(
                        div()
                            .text_sm()
                            .text_color(muted_fg)
                            .child(strings::launcher::SEARCH_HINT),
                    )
                })
                .child(div().flex_1())
                .child(
                    div().id("settings-button").occlude().flex_shrink_0().child(
                        Button::new("settings")
                            .ghost()
                            .icon(IconName::Settings)
                            .tooltip(strings::launcher::TOOLTIP_SETTINGS)
                            .on_click(cx.listener(|launcher, _, window, cx| {
                                launcher.open_settings(window, cx)
                            })),
                    ),
                )
                .into_any_element(),
            Page::Settings(_) | Page::Calculator => {
                let is_calculator = matches!(self.page, Page::Calculator);
                bar.child(
                    div().id("page-back").occlude().flex_shrink_0().child(
                        Button::new("back-to-commands")
                            .ghost()
                            .icon(IconName::ArrowLeft)
                            .tooltip(strings::launcher::TOOLTIP_BACK)
                            .on_click(cx.listener(|launcher, _, window, cx| {
                                launcher.back_to_commands(window, cx)
                            })),
                    ),
                )
                .when(is_calculator, |this| {
                    // 计算器图标只在全集里才有，且用 `assets::IconName` 取，组件子集没有。
                    this.child(
                        Icon::new(gpui_kit::assets::IconName::Calculator)
                            .size_4()
                            .text_color(muted_fg),
                    )
                })
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .child(if is_calculator {
                            strings::launcher::CALCULATOR_TITLE
                        } else {
                            strings::launcher::SETTINGS_TITLE
                        }),
                )
                .child(div().flex_1())
                .into_any_element()
            }
        }
    }
}

/// 页头要多高才能放下一个 `height` 高的输入框，并且上下留出跟默认一样的余量。
fn header_height(settings: Settings) -> Pixels {
    let needed = settings.search_height as f32 + HEADER_SLACK;
    px(needed.max(HEADER_MIN_HEIGHT))
}

/// 输入框要多宽才能不滚动地显示 `query`。
///
/// 宽度按文字算而不是铺满搜索栏：它多占一像素，面板就少一像素可拖。
fn search_field_width(window: &Window, cx: &Context<Launcher>, query: &str) -> Pixels {
    let padding = px(SEARCH_FIELD_PADDING);
    if query.is_empty() {
        return padding + px(SEARCH_FIELD_CARET);
    }

    let theme = cx.theme();
    let colors = UiColors::current(cx);
    let run = TextRun {
        len: query.len(),
        font: Font {
            family: theme.font_family.clone(),
            ..Default::default()
        },
        color: colors.foreground,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let text = window.text_system().shape_line(
        query.to_string().into(),
        px(SEARCH_TEXT_SIZE),
        &[run],
        None,
    );
    (text.width() + padding + px(SEARCH_FIELD_SLACK)).min(px(SEARCH_FIELD_MAX))
}

impl Render for Launcher {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = UiColors::current(cx);
        let (bg, fg, border, muted, muted_fg) = (
            colors.panel,
            colors.foreground,
            colors.border,
            colors.surface,
            colors.muted_foreground,
        );
        let count = self.list.read(cx).delegate().command_count();

        let chip = |key: &'static str, label: &'static str| {
            h_flex()
                .gap_1()
                .child(
                    div()
                        .px_1()
                        .rounded(px(4.))
                        .bg(muted)
                        .text_color(colors.footer_key)
                        .child(key),
                )
                .child(div().text_color(muted_fg).child(label))
        };

        let (hints, trailing): (Vec<(&'static str, &'static str)>, String) = match self.page {
            Page::Commands => (
                vec![
                    ("\u{2191}\u{2193}", strings::launcher::FOOTER_NAVIGATE),
                    ("Enter", strings::launcher::FOOTER_OPEN),
                    ("Esc", strings::launcher::FOOTER_CLOSE),
                ],
                strings::launcher::count(count),
            ),
            Page::Settings(_) => (
                vec![
                    ("\u{2191}\u{2193}", strings::launcher::FOOTER_SECTIONS),
                    ("Esc", strings::launcher::FOOTER_BACK),
                ],
                String::new(),
            ),
            Page::Calculator => (vec![("Esc", strings::launcher::FOOTER_BACK)], String::new()),
        };

        let footer = h_flex()
            .px_3()
            .py_2()
            .gap_4()
            .border_t_1()
            .border_color(border)
            .text_xs()
            .children(hints.into_iter().map(|(key, label)| chip(key, label)))
            .child(div().flex_1())
            .when(!trailing.is_empty(), move |this| {
                this.child(div().text_color(muted_fg).child(trailing))
            });

        let body = match self.page {
            Page::Commands => div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .child(List::new(&self.list).px_1().py_1())
                .into_any_element(),
            Page::Settings(tab) => settings_page::render(self, tab, cx).into_any_element(),
            Page::Calculator => calculator_page::render(self, colors, cx).into_any_element(),
        };

        // 面板自己的圆角必须和系统裁剪的圆角一模一样（8 物理像素，不随 DPI 缩放），
        // 否则面板的圆角和窗口的圆角会差出一个小小的缺口。
        let radius = px(CORNER_RADIUS / window.scale_factor());

        div()
            .flex()
            .flex_col()
            .size_full()
            .rounded(radius)
            // 两端要跟着圆角裁掉，顶边高光才收得干净。
            .overflow_hidden()
            // 半透明时底下的亚克力从这儿透上来；不半透明就是一块实色面板。
            .bg(bg)
            .text_color(fg)
            // 玻璃的顶边高光。**不用 `border_1`**：GPUI 的边框只画四条直边，
            // 圆角那段弧是空的，框线在四个角上会断开（试过，用户看到了）。
            // 一条 1px 的顶线两端被圆角裁掉，正好停在弧线的起点，看起来更像玻璃边缘。
            .child(
                div()
                    .flex_shrink_0()
                    .h(px(1.))
                    .w_full()
                    .bg(colors.panel_edge),
            )
            .child(self.header(window, cx))
            .child(body)
            .child(footer)
    }
}
