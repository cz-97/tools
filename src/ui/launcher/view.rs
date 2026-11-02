//! 面板的整体绘制：页头、页脚，以及三个页面的骨架。
//!
//! 从 launcher 的行为里拆出来：这里只管长什么样，页面切换和按键逻辑都在上级模块。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::list::List;
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, Context, Font, FontWeight, Hsla, IntoElement, Pixels, Render, SharedString,
    TextRun, Window, WindowControlArea, div, prelude::*, px,
};
use tools::settings::Settings;

use crate::app::stored_settings;
use crate::strings;
use crate::ui::calculator_page;
use crate::ui::colors::UiColors;
use crate::ui::commands_list;
use crate::ui::lists;
use crate::ui::pages::user_tools as user_tools_page;
use crate::ui::settings_page::{self, small_button};

use super::{
    CORNER_RADIUS, HEADER_MIN_HEIGHT, HEADER_SLACK, Launcher, ListKind, Page, SEARCH_FIELD_CARET,
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
            Page::List(kind) => self.list_header(bar, kind, window, cx),
            Page::UserTools => self.user_tools_header(bar, window, cx),
        }
    }

    /// 「自定义条目」页的页头：标题、搜索框、三个类型筛选、新增。
    ///
    /// 和书签、历史同一副骨架：左边标题，中间搜索，右边是这一页能做的事。
    fn user_tools_header(
        &self,
        bar: gpui_kit::Stateful<gpui_kit::Div>,
        _window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let colors = UiColors::current(cx);
        let muted_fg = colors.muted_foreground;
        let kind = self.user_kind;
        let count = self.user_list.read(cx).delegate().rows(cx).len();

        bar.child(Icon::new(IconName::FileText).size_4().text_color(muted_fg))
            .child(
                div()
                    .flex_shrink_0()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(strings::tools_page::USER_ITEMS),
            )
            .child(
                div()
                    .id("tools-search")
                    .occlude()
                    .flex_shrink_0()
                    .ml_2()
                    .w(px(180.))
                    .child(
                        Input::new(&self.tools_search)
                            .small()
                            .appearance(false)
                            .cleanable(false),
                    ),
            )
            .child(div().flex_1())
            .child(
                div().occlude().child(crate::ui::settings_page::segmented(
                    "user-tools-kind",
                    tools::settings::ToolKind::USER
                        .into_iter()
                        .map(|item| (item, strings::tools_page::kind_label(item))),
                    kind,
                    colors,
                    cx,
                    |launcher, choice, cx| launcher.set_user_kind(choice, cx),
                )),
            )
            .child(div().occlude().child(small_button(
                SharedString::from("user-tools-add"),
                strings::tools_page::add_label(kind).into(),
                colors,
                true,
                cx.listener(move |launcher, _, window, cx| {
                    launcher.add_user_tool(kind, window, cx)
                }),
            )))
            .child(
                div()
                    .text_color(muted_fg)
                    .child(strings::lists::count(count)),
            )
            .into_any_element()
    }

    /// 工具列表页的页头。
    ///
    /// 左边是这一页的图标和标题，跟着一个能立刻过滤的搜索框；**右上角**是这一页的
    /// 按钮（仓库两页是「重新去网上取」，查询仓库是「再搜一次」，书签和历史不给按钮）。
    fn list_header(
        &self,
        bar: gpui_kit::Stateful<gpui_kit::Div>,
        kind: ListKind,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let colors = UiColors::current(cx);
        let muted_fg = colors.muted_foreground;
        let settings = stored_settings(cx);
        let loading = self.entries.read(cx).delegate().is_loading();
        let text = self.list_search.read(cx).value().to_string();
        let hint = list_hint(kind);

        // 左上角只留这一页的图标和标题：返回交给 `Esc`，搜索框前面也不挂放大镜。
        let bar = bar
            .child(Icon::new(kind.icon()).size_4().text_color(muted_fg))
            .child(
                div()
                    .flex_shrink_0()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(kind.title()),
            )
            .child(
                div()
                    .id("list-search")
                    .occlude()
                    .flex_shrink_0()
                    .ml_2()
                    .w(search_field_width_at_least(window, cx, &text, hint))
                    .h(px(settings.search_height as f32))
                    .child(
                        Input::new(&self.list_search)
                            .small()
                            .appearance(false)
                            .cleanable(false),
                    ),
            )
            .when(text.is_empty(), |this| {
                // 提示画在拖动区里，跟着输入框走，不在框内。
                this.child(
                    div()
                        .flex_shrink_0()
                        .text_sm()
                        .text_color(muted_fg)
                        .child(hint),
                )
            })
            .child(div().flex_1());

        // 右上角按钮：只有「要手动去取」的页面才给。
        // 书签和历史每次进页面都重读库，天生就是最新的，不再放刷新。
        if !kind.needs_action() {
            return bar.into_any_element();
        }
        let (icon, tip) = if kind == ListKind::Search {
            (
                gpui_kit::assets::IconName::Search,
                strings::lists::SEARCH_AGAIN,
            )
        } else {
            (
                gpui_kit::assets::IconName::RefreshCw,
                strings::lists::REFRESH,
            )
        };

        bar.child(
            div().id("list-action").occlude().flex_shrink_0().child(
                Button::new("list-action-button")
                    .ghost()
                    .icon(icon)
                    .tooltip(if loading {
                        strings::lists::LOADING
                    } else {
                        tip
                    })
                    .on_click(cx.listener(move |launcher, _, window, cx| {
                        launcher.activate_list_action(kind, cx);
                        launcher.focus_list_search(window, cx);
                    })),
            ),
        )
        .into_any_element()
    }
}

/// 这一页搜索框里的提示；`Tab` 那枚按钮的悬浮提示也用得上。
fn list_hint(kind: ListKind) -> &'static str {
    match kind {
        ListKind::Bookmarks => strings::lists::SEARCH_BOOKMARKS,
        ListKind::History => strings::lists::SEARCH_HISTORY,
        ListKind::Projects => strings::lists::SEARCH_PROJECTS,
        ListKind::Stars => strings::lists::SEARCH_STARS,
        ListKind::Search => strings::lists::SEARCH_REPOS,
    }
}

/// 页脚那一行的高度（逻辑像素）。正文高度按它扣，所以它必须是定值。
const FOOTER_HEIGHT: f32 = 34.;

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
    px(
        (text_width(window, cx, query) + SEARCH_FIELD_PADDING + SEARCH_FIELD_SLACK)
            .min(SEARCH_FIELD_MAX),
    )
}

/// 列表页的输入框：空着的时候也得放得下那句提示，不能只剩半个字。
fn search_field_width_at_least(
    window: &Window,
    cx: &Context<Launcher>,
    query: &str,
    hint: &str,
) -> Pixels {
    let measured = if query.is_empty() {
        0.
    } else {
        text_width(window, cx, query)
    };
    let floor = text_width(window, cx, hint);
    px((measured.max(floor) + SEARCH_FIELD_PADDING + SEARCH_FIELD_SLACK).min(SEARCH_FIELD_MAX))
}

/// 一段文字排版之后有多宽。
fn text_width(window: &Window, cx: &Context<Launcher>, text: &str) -> f32 {
    shaped_width(window, cx, text, SEARCH_TEXT_SIZE)
}

/// 同 [`text_width`]，但字号自己给。
///
/// 量的时候用的字号必须和画的时候一致，否则算出来的折行位置和真正画出来差一截。
fn shaped_width(window: &Window, cx: &Context<Launcher>, text: &str, size: f32) -> f32 {
    let theme = cx.theme();
    let colors = UiColors::current(cx);
    let run = TextRun {
        len: text.len(),
        font: Font {
            family: theme.font_family.clone(),
            ..Default::default()
        },
        color: colors.foreground,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let shaped = window
        .text_system()
        .shape_line(text.to_string().into(), px(size), &[run], None);
    f32::from(shaped.width())
}

/// 方格名字的字号。**必须和 `grid_cell` 里那个 `text_size` 是同一个数。**
const GRID_NAME_TEXT_SIZE: f32 = 12.0;

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
            Page::UserTools => (
                vec![
                    ("\u{2191}\u{2193}", strings::launcher::FOOTER_NAVIGATE),
                    ("\u{2190}\u{2192}", strings::launcher::FOOTER_SECTIONS),
                    ("Enter", strings::launcher::FOOTER_OPEN),
                    ("Esc", strings::launcher::FOOTER_BACK),
                ],
                String::new(),
            ),
            Page::List(kind) => {
                let enter = if kind == ListKind::Search {
                    strings::lists::FOOTER_SEARCH
                } else {
                    strings::lists::FOOTER_OPEN
                };
                let rows = self.entries.read(cx).delegate().len();
                (
                    vec![
                        ("\u{2191}\u{2193}", strings::launcher::FOOTER_NAVIGATE),
                        ("Enter", enter),
                        ("Esc", strings::launcher::FOOTER_BACK),
                    ],
                    strings::lists::count(rows),
                )
            }
        };

        let footer = h_flex()
            .flex_shrink_0()
            .h(px(FOOTER_HEIGHT))
            .items_center()
            .px_3()
            .gap_4()
            .border_t_1()
            .border_color(border)
            .text_xs()
            .children(hints.into_iter().map(|(key, label)| chip(key, label)))
            .child(div().flex_1())
            .when(!trailing.is_empty(), move |this| {
                this.child(div().text_color(muted_fg).child(trailing))
            });

        // 正文高度直接算出来，不指望 flex 收缩：`List`（gpui-component 的）内部是
        // `size_full()`，外层拿不到确定高度时它会按内容高度排版，多出来的部分直接盖住
        // 页脚——面板能拖小之后这就是「底部提示不见了」的来源。
        let header_px = header_height(stored_settings(cx));
        let body_height = px((f32::from(window.viewport_size().height)
            - f32::from(header_px)
            - FOOTER_HEIGHT)
            .max(0.));

        let page = match self.page {
            Page::Commands => match stored_settings(cx).home_layout {
                tools::settings::HomeLayout::List => {
                    List::new(&self.list).px_1().py_1().into_any_element()
                }
                tools::settings::HomeLayout::Grid => home_grid(self, window, cx),
            },
            Page::Settings(tab) => settings_page::render(self, tab, cx).into_any_element(),
            Page::Calculator => calculator_page::render(self, colors, cx).into_any_element(),
            Page::List(_) => lists::render(&self.entries),
            Page::UserTools => user_tools_page::render(&self.user_list),
        };
        let body = div().h(body_height).min_w_0().overflow_hidden().child(page);

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
            // 提示框盖在整页上，占用的是同一块地方，不改变版面。
            .child(self.prompt_layer(colors, cx))
            .child(self.editor_layer(colors, cx))
    }
}

impl Launcher {
    /// 「文件不在了」那一层：暗底 + 一张小卡片，问用户是删掉还是换个文件。
    ///
    /// 盖住整页是必须的，不然用户会以为后面那个列表还能点；`occlude` 让点击落在
    /// 这一层上而不是穿透到列表去。
    /// 新增 / 编辑用户条目的弹窗：编辑区从列表里挪出来，列表短了、编辑也不怕被滚走。
    fn editor_layer(&self, colors: UiColors, cx: &Context<Self>) -> AnyElement {
        if self.tool_editor.is_none() {
            return div().into_any_element();
        }
        let backdrop = cx.theme().background;
        let card = v_flex()
            .w(px(420.))
            .gap_2()
            .p_4()
            .rounded(px(10.))
            .border_1()
            .border_color(colors.border)
            .bg(backdrop)
            .child(crate::ui::settings_page::tool_editor_block(
                self, colors, cx,
            ));

        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .bg(backdrop.opacity(0.72))
            .child(card)
            .into_any_element()
    }

    fn prompt_layer(&self, colors: UiColors, cx: &Context<Self>) -> AnyElement {
        let Some(prompt) = self.prompt.as_ref() else {
            return div().into_any_element();
        };
        let backdrop = cx.theme().background;
        let card = v_flex()
            .w(px(360.))
            .gap_2()
            .p_4()
            .rounded(px(10.))
            .border_1()
            .border_color(colors.border)
            .bg(backdrop)
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(colors.foreground)
                    .child(strings::tools_page::MISSING_TITLE),
            )
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .text_sm()
                    .text_color(colors.foreground)
                    .child(prompt.name.clone()),
            )
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .text_xs()
                    .text_color(colors.on_surface_muted)
                    .child(prompt.target.clone()),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(strings::tools_page::MISSING_HINT),
            )
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .mt_1()
                    .child(small_button(
                        "prompt-relocate".into(),
                        strings::tools_page::MISSING_RELOCATE.into(),
                        colors,
                        true,
                        cx.listener(|launcher, _, _, cx| launcher.prompt_relocate(cx)),
                    ))
                    .child(small_button(
                        "prompt-delete".into(),
                        strings::tools_page::MISSING_DELETE.into(),
                        colors,
                        false,
                        cx.listener(|launcher, _, window, cx| launcher.prompt_delete(window, cx)),
                    ))
                    .child(div().flex_1())
                    .child(small_button(
                        "prompt-cancel".into(),
                        strings::tools_page::MISSING_CANCEL.into(),
                        colors,
                        false,
                        cx.listener(|launcher, _, window, cx| launcher.dismiss_prompt(window, cx)),
                    )),
            );

        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .bg(backdrop.opacity(0.72))
            .child(card)
            .into_any_element()
    }
}

/// 主页的方格布局。
///
/// 命中区（指令匹配）仍然是行、钉在最上面；下面是自绘的方格。**不复用 `List`**：
/// 它要求所有行同高（还要逐项测量），而命中行和方格行天然不一样高，硬凑只会把命中行
/// 撑成方格的高度。方格的数量、位置、滚动和高亮全是这里算的。
fn home_grid(launcher: &Launcher, window: &mut Window, cx: &mut Context<Launcher>) -> AnyElement {
    let colors = UiColors::current(cx);
    let metrics = crate::ui::grid::metrics_for(f32::from(window.viewport_size().width));
    let gap = crate::ui::grid::CELL_GAP;

    let delegate = launcher.list.read(cx).delegate();
    let matches_len = delegate.match_count();
    let selected = delegate.selected;
    let copied_at = delegate.copied.clone();
    let matches: Vec<tools::matcher::Match> = delegate.matches().to_vec();
    let items: Vec<commands_list::HomeItem> = delegate.command_items().to_vec();

    // 命中区：和列表布局长得一样，只是不参与滚动。
    let match_rows: Vec<AnyElement> = matches
        .iter()
        .enumerate()
        .map(|(index, matched)| {
            let copied = copied_at.as_ref().is_some_and(|(title, at)| {
                *title == matched.title && at.elapsed() < commands_list::COPY_FEEDBACK
            });
            commands_list::MatchRow {
                item: matched.clone(),
                selected: selected == index,
                index,
                copied,
            }
            .into_any_element()
        })
        .collect();

    let rows: Vec<AnyElement> = items
        .chunks(metrics.columns)
        .enumerate()
        .map(|(row, chunk)| {
            h_flex()
                .gap(px(gap))
                .children(chunk.iter().enumerate().map(|(column, item)| {
                    let index = matches_len + row * metrics.columns + column;
                    grid_cell(
                        item,
                        index,
                        metrics.cell,
                        selected == index,
                        colors,
                        window,
                        cx,
                    )
                }))
                .into_any_element()
        })
        .collect();

    // 悬停时那句描述：贴在被压住的那格下面，右边不越界。
    let tooltip = launcher.hovered_cell.and_then(|hovered| {
        let item = items.get(hovered.checked_sub(matches_len)?)?;
        let column = (hovered - matches_len) % metrics.columns;
        let row = (hovered - matches_len) / metrics.columns;
        let (x, y) = metrics.corner(row, column, gap);
        let offset = f32::from(launcher.grid_scroll.offset().y);
        let width = f32::from(window.viewport_size().width) - crate::ui::grid::CELL_PADDING * 2.0;
        let left = x.min((width - TOOLTIP_WIDTH).max(0.0));
        Some(
            div()
                .absolute()
                .left(px(left))
                .top(px(y + metrics.cell + 4.0 - offset))
                .w(px(TOOLTIP_WIDTH))
                .px_2()
                .py_1()
                .rounded(px(6.))
                .bg(colors.control)
                .border_1()
                .border_color(colors.surface)
                .text_xs()
                .text_color(colors.foreground)
                .child(describe(item))
                .into_any_element(),
        )
    });

    let grid = div()
        .flex()
        .flex_col()
        .gap(px(gap))
        .w_full()
        .relative()
        .children(rows)
        .children(tooltip);

    // 一条都没有：和列表布局一样给句话，别留一块空白。
    let empty = (items.is_empty() && matches.is_empty()).then(|| {
        div()
            .text_sm()
            .text_color(colors.muted_foreground)
            .child(strings::launcher::EMPTY)
            .into_any_element()
    });

    v_flex()
        .size_full()
        .p(px(crate::ui::grid::CELL_PADDING))
        .gap_1()
        .children(match_rows)
        .children(empty)
        .child(
            div()
                .id("home-grid-scroll")
                .flex_1()
                .min_h_0()
                .w_full()
                .overflow_y_scroll()
                .track_scroll(&launcher.grid_scroll)
                .child(grid),
        )
        .into_any_element()
}

/// 提示框最大宽度。
const TOOLTIP_WIDTH: f32 = 220.0;

/// 一个方格：图标 + 名字，选中时整块高亮。
fn grid_cell(
    item: &commands_list::HomeItem,
    index: usize,
    cell: f32,
    selected: bool,
    colors: UiColors,
    window: &Window,
    cx: &Context<Launcher>,
) -> AnyElement {
    let (icon, name) = match item {
        commands_list::HomeItem::Command(command) => {
            (command.icon.clone(), command.name.to_string())
        }
        commands_list::HomeItem::Tool(tool) => {
            (super::user_kind_icon(tool.kind), tool.name.clone())
        }
        // 命中不在方格区里，走到这儿说明状态串了，给个空块撑住布局。
        commands_list::HomeItem::Match(_) => {
            return div().w(px(cell)).h(px(cell)).into_any_element();
        }
    };
    let text_color = if selected {
        colors.accent_foreground
    } else {
        colors.foreground
    };

    div()
        .id(SharedString::from(format!("grid-cell-{index}")))
        .w(px(cell))
        .h(px(cell))
        .flex_shrink_0()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        // 图标和名字贴紧一点：格子里本来就有富余高度，再留 4px 就更散了。
        .gap_0p5()
        .rounded(px(8.))
        .cursor_pointer()
        .when(selected, move |this| this.bg(colors.accent))
        .when(!selected, move |this| {
            this.hover(move |style| style.bg(colors.hover))
        })
        .child(grid_icon(item, Icon::new(icon), text_color))
        .child(grid_name_lines(name, cell, text_color, window, cx))
        .on_hover(cx.listener(move |launcher, hovered: &bool, _window, cx| {
            launcher.set_hovered_cell(index, *hovered, cx)
        }))
        .on_click(
            cx.listener(move |launcher, _: &gpui_kit::ClickEvent, window, cx| {
                launcher.grid_click(index, window, cx)
            }),
        )
        .into_any_element()
}

/// 方格里的图标：用户加的文件/终端用**系统图标**（资源管理器什么样就什么样），
/// 取不到或者内置命令才用编译进来的那套。
fn grid_icon(item: &commands_list::HomeItem, fallback: Icon, color: Hsla) -> AnyElement {
    if let commands_list::HomeItem::Tool(tool) = item
        && let Some(image) = crate::ui::icons::for_tool(tool.kind, &tool.target)
    {
        return gpui_kit::img(image)
            .w(px(GRID_ICON_SIZE))
            .h(px(GRID_ICON_SIZE))
            .into_any_element();
    }
    fallback.size_5().text_color(color).into_any_element()
}

/// 方格图标的边长。
const GRID_ICON_SIZE: f32 = 24.0;

/// 方格里的名字：一行放得下就一行，放不下就按宽度匀着折成两行，各自居中。
///
/// 为什么不能靠 CSS 那套自动换行：格子里只有 68~100px，中文一个就 12px，自动换行会
/// 出现「七字一行、末尾孤零零一个字」的参差；用户要的是 8 个字对半 4+4。所以这里
/// 自己量宽度、自己找断点（`grid::split_balanced`），拿到两段再各画一行。
fn grid_name_lines(
    name: String,
    cell: f32,
    color: Hsla,
    window: &Window,
    cx: &Context<Launcher>,
) -> AnyElement {
    let available = (cell - NAME_HORIZONTAL_PADDING * 2.0).max(1.0);
    let measure = |text: &str| shaped_width(window, cx, text, GRID_NAME_TEXT_SIZE);
    let ellipsis = measure("\u{2026}");
    let (first, second) = crate::ui::grid::split_balanced(&name, available, &measure, ellipsis);

    let line = |text: String| {
        div()
            .w_full()
            .text_center()
            .text_ellipsis()
            .text_size(px(GRID_NAME_TEXT_SIZE))
            .text_color(color)
            .child(text)
    };
    v_flex()
        .w_full()
        .items_center()
        .children(Some(line(first)))
        .children((!second.is_empty()).then(|| line(second)))
        .into_any_element()
}

/// 方格名字左右各留多少（判断可排宽度用，和 `.px_1()` 对齐）。
const NAME_HORIZONTAL_PADDING: f32 = 4.0;

/// 方格悬停时显示的那句话。
fn describe(item: &commands_list::HomeItem) -> SharedString {
    match item {
        commands_list::HomeItem::Command(command) => command.description.into(),
        commands_list::HomeItem::Tool(tool) => tool.target.clone().into(),
        commands_list::HomeItem::Match(_) => SharedString::default(),
    }
}
