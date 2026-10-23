//! 命令页：左边是设置子模块，右边是该子模块下的命令。

use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Icon, h_flex, v_flex};
use gpui_kit::{
    App, ClickEvent, Context, IntoElement, MouseButton, MouseDownEvent, SharedString, Window, div,
    prelude::*, px,
};
use gpui_tools::settings::CommandSettings;

use crate::app::stored_settings;
use crate::catalogue;
use crate::shortcuts::shortcut_label;
use crate::strings;
use crate::ui::colors::UiColors;
use crate::ui::launcher::{Launcher, MODULE_WIDTH, RecordTarget};

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
                launcher,
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
    launcher: &Launcher,
    colors: UiColors,
    cx: &Context<Launcher>,
) -> impl IntoElement {
    let recording = launcher
        .recording_command()
        .is_some_and(|current| std::ptr::eq(current, command));
    let label: SharedString = if recording {
        launcher
            .recording_conflict()
            .map_or_else(
                || strings::commands_page::RECORDING.to_string(),
                str::to_string,
            )
            .into()
    } else {
        match state.shortcut.as_deref() {
            Some(chord) => shortcut_label(chord).into(),
            None => strings::commands_page::SHORTCUT_EMPTY.into(),
        }
    };
    let shortcut = shortcut_button(
        SharedString::from(format!("shortcut-{}", command.id)),
        label,
        recording,
        colors,
        cx.listener(move |launcher, _, _, cx| launcher.toggle_command_recording(command, cx)),
        cx.listener(move |launcher, _: &MouseDownEvent, _, cx| {
            launcher.clear_shortcut(RecordTarget::Command(command), cx)
        }),
    );

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
                        .text_color(colors.on_surface_muted)
                        .child(command.description),
                ),
        )
        .child(
            div()
                .flex_shrink_0()
                .text_xs()
                .text_color(colors.on_surface_muted)
                .child(strings::common::uses(state.uses)),
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

/// 快捷键按钮：左键开始/取消录制，右键清空。
///
/// 命令页和系统页的「全局快捷键」共用这一个：两处交互必须一模一样，
/// 不然用户得记两套规矩。
pub(crate) fn shortcut_button(
    id: SharedString,
    label: SharedString,
    recording: bool,
    colors: UiColors,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_clear: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .px_2()
        .py_1()
        .rounded(px(6.))
        .border_1()
        .border_color(colors.surface)
        .text_xs()
        .max_w(px(240.))
        .overflow_hidden()
        .text_ellipsis()
        .cursor_pointer()
        .text_color(if recording {
            colors.accent_foreground
        } else {
            colors.on_surface_muted
        })
        .when(recording, move |this| this.bg(colors.accent))
        .when(!recording, move |this| {
            this.hover(move |style| style.bg(colors.hover))
        })
        .on_click(on_click)
        .on_mouse_down(MouseButton::Right, on_clear)
        .child(label)
}
