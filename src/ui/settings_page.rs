//! 设置页：左边是子模块，右边是选中那一页的内容。

use gpui_kit::component::slider::Slider;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, IntoElement, SharedString, Window, div, prelude::*, px,
};
use gpui_tools::settings::{Settings, Theme as ColorScheme};

use crate::actions;
use crate::app::stored_settings;
use crate::strings;
use crate::ui::colors::UiColors;
use crate::ui::commands_page;
use crate::ui::launcher::{Launcher, SIDEBAR_WIDTH, SettingsTab};

/// 画出整个设置页。
pub(crate) fn render(
    launcher: &Launcher,
    tab: SettingsTab,
    cx: &Context<Launcher>,
) -> impl IntoElement {
    let colors = UiColors::current(cx);
    let settings = stored_settings(cx);
    let border = cx.theme().border;

    let sections = v_flex()
        .w(px(SIDEBAR_WIDTH))
        .flex_shrink_0()
        .p_2()
        .gap_1()
        .border_r_1()
        .border_color(border)
        .children(SettingsTab::ALL.into_iter().enumerate().map(|(ix, item)| {
            let selected = item == tab;
            h_flex()
                .id(SharedString::from(format!("section-tab-{ix}")))
                .gap_2()
                .px_3()
                .py_2()
                .rounded(px(6.))
                .text_sm()
                .text_color(if selected {
                    colors.accent_foreground
                } else {
                    colors.foreground
                })
                .cursor_pointer()
                .when(selected, move |this| this.bg(colors.accent))
                .when(!selected, move |this| {
                    this.hover(move |style| style.bg(colors.hover))
                })
                .on_click(cx.listener(move |launcher, _, _, cx| launcher.select_tab(item, cx)))
                .child(Icon::new(item.icon()).size_4().text_color(if selected {
                    colors.accent_foreground
                } else {
                    colors.muted_foreground
                }))
                .child(div().child(item.label()))
        }));

    let content = match tab {
        SettingsTab::System => system_page(settings, colors, cx),
        SettingsTab::Commands => commands_page::render(launcher, colors, cx).into_any_element(),
        SettingsTab::Appearance => appearance_page(launcher, settings, colors, cx),
        SettingsTab::About => about_page(colors, cx),
    };

    // `h_flex` 默认在交叉轴居中，这样会把整页浮在面板中间；这里要顶对齐。
    h_flex()
        .flex_1()
        .min_w_0()
        .items_stretch()
        .overflow_hidden()
        .child(sections)
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .px_4()
                .py_4()
                .child(content),
        )
}

/// 系统页。
fn system_page(settings: Settings, colors: UiColors, cx: &Context<Launcher>) -> AnyElement {
    v_flex()
        .gap_1()
        .child(settings_row(
            strings::settings_page::HOTKEY_NAME,
            strings::settings_page::HOTKEY_DESCRIPTION,
            colors,
            value_label(crate::shortcuts::SUMMON_LABEL, colors),
        ))
        .child(settings_row(
            strings::settings_page::AUTOSTART_NAME,
            strings::settings_page::AUTOSTART_DESCRIPTION,
            colors,
            Switch::new("launch-at-login")
                .checked(settings.launch_at_login)
                .on_change(cx.listener(|launcher, checked: &bool, _window, cx| {
                    launcher.set_launch_at_login(*checked, cx)
                })),
        ))
        .child(settings_row(
            strings::settings_page::RESUME_NAME,
            strings::settings_page::RESUME_DESCRIPTION,
            colors,
            segmented(
                "resume",
                Settings::RESUME_CHOICES
                    .into_iter()
                    .map(|seconds| (seconds, strings::settings_page::resume_label(seconds))),
                settings.resume_seconds,
                colors,
                cx,
                |launcher, seconds, cx| launcher.set_resume_seconds(seconds, cx),
            ),
        ))
        .child(action_row(
            "open-config",
            strings::settings_page::CONFIG_NAME,
            crate::app::about_directory(),
            strings::settings_page::CONFIG_OPEN,
            colors,
            cx.listener(|_, _, _, cx| actions::open_settings_folder(cx)),
        ))
        .child(action_row(
            "quit",
            strings::settings_page::QUIT_NAME,
            strings::settings_page::QUIT_DESCRIPTION,
            strings::settings_page::QUIT_ACTION,
            colors,
            cx.listener(|_, _, _, cx| actions::quit_launcher(cx)),
        ))
        .into_any_element()
}

/// 外观页。
fn appearance_page(
    launcher: &Launcher,
    settings: Settings,
    colors: UiColors,
    cx: &Context<Launcher>,
) -> AnyElement {
    v_flex()
        .gap_1()
        .child(settings_row(
            strings::settings_page::THEME_NAME,
            strings::settings_page::THEME_DESCRIPTION,
            colors,
            segmented(
                "theme",
                ColorScheme::ALL
                    .into_iter()
                    .map(|theme| (theme, theme_label(theme))),
                settings.theme,
                colors,
                cx,
                |launcher, choice, cx| launcher.choose_theme(choice, None, cx),
            ),
        ))
        .child(settings_row(
            strings::settings_page::TRANSLUCENT_NAME,
            strings::settings_page::TRANSLUCENT_DESCRIPTION,
            colors,
            Switch::new("translucent-background")
                .checked(settings.translucent)
                .on_change(cx.listener(|launcher, checked: &bool, _, cx| {
                    launcher.set_translucent(*checked, cx)
                })),
        ))
        .child(settings_row(
            strings::settings_page::OPACITY_NAME,
            strings::settings_page::OPACITY_DESCRIPTION,
            colors,
            h_flex()
                .flex_shrink_0()
                .items_center()
                .gap_3()
                .child(
                    // 滑动条的状态挂在面板上，所以拖动的每一帧都直接变成面板的染色深浅。
                    Slider::new(&launcher.opacity).w(px(160.)).flex_shrink_0(),
                )
                .child(
                    div()
                        .w(px(48.))
                        .text_center()
                        .text_sm()
                        .text_color(colors.foreground)
                        .child(strings::settings_page::opacity_value(
                            settings.panel_opacity,
                        )),
                ),
        ))
        .child(settings_row(
            strings::settings_page::HEIGHT_NAME,
            strings::settings_page::HEIGHT_DESCRIPTION,
            colors,
            stepper(
                "search-height",
                settings.search_height,
                StepperRange {
                    lowest: Settings::SEARCH_HEIGHT_MIN,
                    highest: Settings::SEARCH_HEIGHT_MAX,
                    step: Settings::SEARCH_HEIGHT_STEP,
                    label: strings::settings_page::height_value(settings.search_height).into(),
                },
                colors,
                cx,
                |launcher, height, cx| launcher.set_search_height(height, cx),
            ),
        ))
        .into_any_element()
}

/// 关于页。
fn about_page(colors: UiColors, cx: &Context<Launcher>) -> AnyElement {
    v_flex()
        .gap_1()
        .child(settings_row(
            strings::settings_page::VERSION_NAME,
            strings::settings_page::VERSION_DESCRIPTION,
            colors,
            value_label(env!("CARGO_PKG_VERSION"), colors),
        ))
        .child(action_row(
            "open-repository",
            strings::settings_page::REPOSITORY_NAME,
            actions::REPOSITORY_URL,
            strings::settings_page::REPOSITORY_OPEN,
            colors,
            cx.listener(|_, _, _, cx| actions::open_repository(cx)),
        ))
        .child(action_row(
            "open-folder",
            strings::settings_page::CONFIG_NAME,
            actions::program_dir().display().to_string(),
            strings::settings_page::FOLDER_OPEN,
            colors,
            cx.listener(|_, _, _, cx| actions::open_program_folder(cx)),
        ))
        .into_any_element()
}

/// 主题选项的名字。
fn theme_label(theme: ColorScheme) -> &'static str {
    match theme {
        ColorScheme::System => strings::settings_page::THEME_SYSTEM,
        ColorScheme::Light => strings::settings_page::THEME_LIGHT,
        ColorScheme::Dark => strings::settings_page::THEME_DARK,
    }
}

/// 一行设置：左边是名字和说明，右边是控件。
///
/// 每个设置项都是这副骨架，控件换成什么都行，名字和说明的位置永远一致。
///
/// 子项目基础布局；具体可点击区域由各控件自身负责。
pub(crate) fn settings_row(
    name: &'static str,
    description: impl Into<SharedString>,
    colors: UiColors,
    control: impl IntoElement,
) -> gpui_kit::Div {
    h_flex()
        .w_full()
        .min_w_0()
        .justify_between()
        .items_center()
        .gap_4()
        .px_3()
        .py_2()
        .rounded(px(6.))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .child(div().text_sm().text_color(colors.foreground).child(name))
                .child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .text_ellipsis()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(description.into()),
                ),
        )
        .child(div().flex_shrink_0().child(control))
}

/// 只读行右侧显示的那个值。
pub(crate) fn value_label(value: impl Into<SharedString>, colors: UiColors) -> impl IntoElement {
    div()
        .text_sm()
        .text_color(colors.muted_foreground)
        .child(value.into())
}

/// 点一下就执行的那种设置行。
pub(crate) fn action_row(
    id: &'static str,
    name: &'static str,
    description: impl Into<SharedString>,
    hint: &'static str,
    colors: UiColors,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    settings_row(
        name,
        description,
        colors,
        h_flex()
            .id(id)
            .gap_1()
            .px_2()
            .py_1()
            .rounded(px(6.))
            .cursor_pointer()
            .hover(move |style| style.bg(colors.hover))
            .on_click(on_click)
            .child(
                div()
                    .text_sm()
                    .text_color(colors.muted_foreground)
                    .child(hint),
            )
            .child(
                Icon::new(IconName::ChevronRight)
                    .size_4()
                    .text_color(colors.muted_foreground),
            ),
    )
}

/// 从几个值里挑一个的控件，画成一串胶囊。
pub(crate) fn segmented<T: Copy + PartialEq + 'static>(
    id: &'static str,
    options: impl IntoIterator<Item = (T, &'static str)>,
    selected: T,
    colors: UiColors,
    cx: &Context<Launcher>,
    on_select: impl Fn(&mut Launcher, T, &mut Context<Launcher>) + Copy + 'static,
) -> impl IntoElement {
    h_flex()
        .gap_0p5()
        .p_0p5()
        .rounded(px(8.))
        .bg(colors.control)
        .children(
            options
                .into_iter()
                .enumerate()
                .map(move |(ix, (value, label))| {
                    let current = value == selected;
                    div()
                        .id(SharedString::from(format!("{id}-{ix}")))
                        .px_2()
                        .py_1()
                        .rounded(px(6.))
                        .text_sm()
                        .text_color(if current {
                            colors.accent_foreground
                        } else {
                            colors.muted_foreground
                        })
                        .cursor_pointer()
                        .when(current, move |this| this.bg(colors.accent))
                        .when(!current, move |this| {
                            this.hover(move |style| style.bg(colors.hover))
                        })
                        .on_click(cx.listener(move |launcher, _: &ClickEvent, _window, cx| {
                            on_select(launcher, value, cx)
                        }))
                        .child(label)
                }),
        )
}

/// 数值步进控件：减、值、加。
/// 步进控件的档位：下限、上限、步长，以及中间显示的字。
pub(crate) struct StepperRange {
    pub(crate) lowest: u32,
    pub(crate) highest: u32,
    pub(crate) step: u32,
    pub(crate) label: SharedString,
}

pub(crate) fn stepper(
    id: &'static str,
    value: u32,
    range: StepperRange,
    colors: UiColors,
    cx: &Context<Launcher>,
    on_change: impl Fn(&mut Launcher, u32, &mut Context<Launcher>) + Copy + 'static,
) -> impl IntoElement {
    let StepperRange {
        lowest,
        highest,
        step,
        label,
    } = range;
    let step = step as isize;
    let at_bottom = value <= lowest;
    let at_top = value >= highest;

    let button = move |ix: usize, icon: IconName, disabled: bool| {
        let button = div()
            .id(SharedString::from(format!("{id}-{ix}")))
            .size_6()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(6.))
            .text_color(colors.foreground);
        if disabled {
            return button.opacity(crate::ui::colors::DISABLED_OPACITY);
        }
        let delta = if ix == 0 { -step } else { step };
        let next = (value as isize + delta).clamp(lowest as isize, highest as isize) as u32;
        button
            .cursor_pointer()
            .hover(move |style| style.bg(colors.hover))
            .on_click(cx.listener(move |launcher, _: &ClickEvent, _window, cx| {
                on_change(launcher, next, cx)
            }))
            .child(Icon::new(icon).size_4())
    };

    h_flex()
        .gap_1()
        .p_0p5()
        .rounded(px(8.))
        .bg(colors.control)
        .child(button(0, IconName::Minus, at_bottom))
        .child(
            div()
                .w(px(64.))
                .text_center()
                .text_sm()
                .text_color(colors.foreground)
                .child(label),
        )
        .child(button(1, IconName::Plus, at_top))
}
