//! 命令页：左边是设置子模块，右边是该子模块下的命令。

use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Icon, h_flex, v_flex};
use gpui_kit::{Context, IntoElement, SharedString, div, prelude::*, px};
use gpui_tools::settings::CommandSettings;

use crate::app::stored_settings;
use crate::catalogue;
use crate::shortcuts::shortcut_label;
use crate::strings;
use crate::ui::colors::UiColors;
use crate::ui::launcher::{Launcher, MODULE_WIDTH};

/// 画出整个命令页。
///
/// 内容全部来自命令目录和每条命令的设置，所以新声明一条命令（挂在某个子模块下）
/// 这里自动就有了，不用改这个函数。
pub(crate) fn render(
    launcher: &Launcher,
    colors: UiColors,
    cx: &Context<Launcher>,
) -> impl IntoElement {
    let settings = stored_settings(cx);
    let selected = launcher.section.min(catalogue::Section::ALL.len() - 1);
    let border = colors.border;

    let sections = v_flex()
        .w(px(MODULE_WIDTH))
        .flex_shrink_0()
        .gap_1()
        .pr_3()
        .border_r_1()
        .border_color(border)
        .children(
            catalogue::Section::ALL
                .iter()
                .enumerate()
                .map(|(ix, section)| {
                    let current = ix == selected;
                    let owned = catalogue::commands(*section).collect::<Vec<_>>();
                    let shown = owned
                        .iter()
                        .filter(|command| {
                            settings.command_enabled(command.section.key(), command.id)
                        })
                        .count();
                    h_flex()
                        .id(SharedString::from(format!("section-{}", section.label())))
                        .gap_2()
                        .px_2()
                        .py_1p5()
                        .rounded(px(6.))
                        .text_sm()
                        .cursor_pointer()
                        .text_color(if current {
                            colors.accent_foreground
                        } else {
                            colors.foreground
                        })
                        .when(current, move |this| this.bg(colors.accent))
                        .when(!current, move |this| {
                            this.hover(move |style| style.bg(colors.hover))
                        })
                        .on_click(
                            cx.listener(move |launcher, _, _, cx| launcher.select_section(ix, cx)),
                        )
                        .child(Icon::new(section.icon()).size_4())
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_ellipsis()
                                .child(section.label()),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(if current {
                                    colors.on_accent_muted
                                } else {
                                    colors.muted_foreground
                                })
                                .child(strings::commands_page::enabled_ratio(shown, owned.len())),
                        )
                }),
        );

    let section = catalogue::Section::ALL[selected];
    let commands = catalogue::commands(section)
        .map(|command| {
            command_row(
                command,
                &settings.command(command.section.key(), command.id),
                launcher
                    .recording()
                    .is_some_and(|current| std::ptr::eq(current, command)),
                colors,
                cx,
            )
            .into_any_element()
        })
        .collect::<Vec<_>>();

    h_flex()
        .size_full()
        .min_w_0()
        .items_stretch()
        .overflow_hidden()
        .child(sections)
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .pl_3()
                .gap_1()
                .child(
                    div()
                        .px_2()
                        .pb_1()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(strings::commands_page::caption(section.label())),
                )
                .when(commands.is_empty(), |this| {
                    this.child(
                        div()
                            .px_2()
                            .py_2()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(strings::commands_page::EMPTY),
                    )
                })
                .children(commands),
        )
}

/// 命令页上的一行：名字、次数、快捷键按钮、启用开关。
fn command_row(
    command: &'static catalogue::Command,
    state: &CommandSettings,
    recording: bool,
    colors: UiColors,
    cx: &Context<Launcher>,
) -> impl IntoElement {
    let shortcut = if recording {
        div()
            .px_2()
            .py_1()
            .rounded(px(6.))
            .bg(colors.accent)
            .text_xs()
            .text_color(colors.accent_foreground)
            .child(strings::commands_page::RECORDING)
            .into_any_element()
    } else {
        let label = match state.shortcut.as_deref() {
            Some(chord) => shortcut_label(chord),
            None => strings::commands_page::SHORTCUT_EMPTY.to_string(),
        };
        div()
            .id(SharedString::from(format!("shortcut-{}", command.id)))
            .px_2()
            .py_1()
            .rounded(px(6.))
            .border_1()
            .border_color(colors.surface)
            .text_xs()
            .cursor_pointer()
            .text_color(colors.muted_foreground)
            .hover(move |style| style.bg(colors.hover))
            .on_click(cx.listener(move |launcher, _, _, cx| launcher.toggle_recording(command, cx)))
            .child(label)
            .into_any_element()
    };

    h_flex()
        .w_full()
        .min_w_0()
        .gap_2()
        .px_2()
        .py_2()
        .rounded(px(6.))
        .bg(colors.surface)
        .child(
            div()
                .flex_shrink_0()
                .size_6()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.))
                .child(
                    Icon::new(command.icon.clone())
                        .size_3()
                        .text_color(colors.foreground),
                ),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .child(
                    div()
                        .text_sm()
                        .text_color(colors.foreground)
                        .child(command.name),
                )
                .child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .text_ellipsis()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(command.description),
                ),
        )
        .child(
            div()
                .flex_shrink_0()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child(strings::commands_page::uses(state.uses)),
        )
        .child(div().flex_shrink_0().child(shortcut))
        .child(
            div().flex_shrink_0().child(
                Switch::new(SharedString::from(format!("enabled-{}", command.id)))
                    .checked(state.enabled)
                    .on_change(cx.listener(move |launcher, checked: &bool, _window, cx| {
                        launcher.set_command_enabled(command, *checked, cx)
                    })),
            ),
        )
}
