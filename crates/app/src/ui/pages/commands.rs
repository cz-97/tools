//! 命令页：左边是设置子模块，右边是该子模块下的命令。

use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Icon, h_flex, v_flex};
use gpui_kit::{
    App, ClickEvent, Context, IntoElement, MouseButton, MouseDownEvent, Pixels, SharedString,
    Window, div, prelude::*, px,
};
use tools_core::settings::CommandSettings;

use crate::app::stored_settings;
use crate::catalogue;
use crate::shortcuts::shortcut_label;
use crate::strings;
use crate::ui::colors::UiColors;
use crate::ui::launcher::{Launcher, RecordTarget};
use crate::ui::metrics::{RADIUS_CHIP, RADIUS_CONTROL, RADIUS_SURFACE};

/// 设置里「工具」页的分组：命令目录的 `Section` 归成这几组。
///
/// 内容太少的分组不单独占一行：自定义条目、计算器、关于都并进「其他」——
/// 它们各自只有一两条命令，单起一个标题反而更难找。
const GROUPS: [(&str, &[catalogue::Section]); 4] = [
    (strings::sections::SYSTEM, &[catalogue::Section::System]),
    (strings::sections::BROWSER, &[catalogue::Section::Browser]),
    (strings::sections::GITHUB, &[catalogue::Section::Github]),
    (
        strings::sections::OTHER,
        &[
            catalogue::Section::Tools,
            catalogue::Section::Calculator,
            catalogue::Section::About,
        ],
    ),
];

/// 画出整个「工具」页：所有命令按分组排成一列。
///
/// 原来这里是「命令」页，左边一列子模块、右边一列命令，看一眼要换两次选中。
/// 现在改成一张长页面：分组标题（**只写分类名**，不写「系统的命令 · 左键录制…」那种
/// 啰嗦话）+ 一块淡色卡片，滚一遍全看得见。
///
/// 各工具**自己**的设置不在这里：计算精度跟着计算器页、浏览器 profile 跟着书签/历史页、
/// GitHub 用户名和令牌跟着三个仓库页（页头右上角的齿轮，见
/// [`crate::ui::settings_page::tool_settings_block`]）。
pub(crate) fn render(
    launcher: &Launcher,
    colors: UiColors,
    cx: &Context<Launcher>,
) -> impl IntoElement {
    let settings = stored_settings(cx);
    v_flex()
        .id("tools-settings")
        .w_full()
        .min_w_0()
        .gap_4()
        .children(GROUPS.iter().filter_map(|(title, sections)| {
            let rows = sections
                .iter()
                .flat_map(|section| catalogue::commands(*section))
                .map(|command| {
                    let state = settings.command(command.section.key(), command.id);
                    command_row(command, &state, launcher, colors, cx).into_any_element()
                })
                .collect::<Vec<_>>();
            if rows.is_empty() {
                return None;
            }
            Some(
                v_flex()
                    .w_full()
                    .gap_1()
                    .child(
                        div()
                            .px_2()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(*title),
                    )
                    .child(
                        v_flex()
                            .w_full()
                            .gap_0p5()
                            .p_1()
                            .rounded(px(RADIUS_SURFACE))
                            .bg(colors.surface)
                            .children(rows),
                    )
                    .into_any_element(),
            )
        }))
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
        px(240.),
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
        .rounded(px(RADIUS_CONTROL))
        .child(
            div()
                .flex_shrink_0()
                .size_6()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(RADIUS_CHIP))
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
/// 命令页、设置里的系统页、以及用户工具那两页共用这一个：交互必须一模一样，
/// 不然用户得记两套规矩。`max_width` 让窄行（用户工具那种一行塞了五个控件的）
/// 自己决定按钮能占多宽。
pub(crate) fn shortcut_button(
    id: SharedString,
    label: SharedString,
    recording: bool,
    colors: UiColors,
    max_width: Pixels,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_clear: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .px_2()
        .py_1()
        .rounded(px(RADIUS_CONTROL))
        .border_1()
        .border_color(colors.surface)
        .text_xs()
        .max_w(max_width)
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

#[cfg(test)]
mod tests {
    use super::GROUPS;
    use crate::catalogue::{COMMANDS, Section};

    /// 每个分组都得有命令，每条命令也都得落在某个分组里——两边漏一个，
    /// 用户在「工具」页里就找不到那条命令了。
    #[test]
    fn every_section_with_commands_lands_in_exactly_one_group() {
        for section in Section::ALL {
            let has_commands = COMMANDS.iter().any(|command| command.section == section);
            let groups = GROUPS
                .iter()
                .filter(|(_, listed)| listed.contains(&section))
                .count();
            if has_commands {
                assert_eq!(
                    groups,
                    1,
                    "{} 下面有命令，但没归进「工具」页的分组",
                    section.label()
                );
            } else {
                assert_eq!(groups, 0, "{} 下面没有命令，不该占一个分组", section.label());
            }
        }
    }
}
