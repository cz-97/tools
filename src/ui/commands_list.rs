//! 主页的命令列表：数据、每一行长什么样、以及过滤和执行。

use std::collections::BTreeMap;

use gpui_kit::component::list::{ListDelegate, ListState};
use gpui_kit::component::{Icon, IconName, IndexPath, Selectable, h_flex, v_flex};
use gpui_kit::{
    App, Context, FontWeight, IntoElement, RenderOnce, SharedString, Task, WeakEntity, Window, div,
    prelude::*, px,
};
use gpui_tools::settings::Settings;

use crate::app::run_command;
use crate::catalogue;
use crate::shortcuts::shortcut_label;
use crate::strings;
use crate::ui::colors::UiColors;
use crate::ui::launcher::Launcher;

/// 列表需要知道的、关于一条命令的状态，从设置镜像过来。
///
/// 复制一份是为了让过滤和绘制不必每行都去读全局值。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CommandState {
    /// 这条命令要不要出现在主页。
    pub(crate) enabled: bool,
    pub(crate) uses: u64,
    /// 全局快捷键，形如 `ctrl+alt+KeyP`。
    pub(crate) shortcut: Option<String>,
}

impl Default for CommandState {
    fn default() -> Self {
        Self {
            enabled: true,
            uses: 0,
            shortcut: None,
        }
    }
}

/// 所有命令的状态，键是（子模块, 命令 id）。
pub(crate) type CommandStates = BTreeMap<(&'static str, &'static str), CommandState>;

/// 一次读完所有命令的状态。
pub(crate) fn command_states(settings: &Settings) -> CommandStates {
    catalogue::COMMANDS
        .iter()
        .map(|command| {
            let (section, id) = (command.section.key(), command.id);
            (
                (section, id),
                CommandState {
                    enabled: settings.command_enabled(section, id),
                    uses: settings.command_uses(section, id),
                    shortcut: settings.command_shortcut(section, id).map(str::to_string),
                },
            )
        })
        .collect()
}

/// 主页的一行：图标、名称和描述，右边是设置里那几项信息。
#[derive(IntoElement)]
pub(crate) struct ToolRow {
    pub(crate) command: &'static catalogue::Command,
    pub(crate) state: CommandState,
    pub(crate) selected: bool,
    /// 行号，只用来给命中区一个稳定的 id。
    pub(crate) index: usize,
}

impl Selectable for ToolRow {
    fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    fn is_selected(&self) -> bool {
        self.selected
    }
}

impl RenderOnce for ToolRow {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = UiColors::current(cx);
        let selected = self.selected;

        let name_color = if selected {
            colors.accent_foreground
        } else {
            colors.foreground
        };
        let detail_color = if selected {
            colors.on_accent_muted
        } else {
            colors.muted_foreground
        };
        // 快捷键小块有自己的底色：选中时是高亮上的浮层，未选中时是不透明的
        // `surface`。文字跟着底色走，不跟整行（整行未选中时直接透在面板上）。
        let chip_color = if selected {
            colors.on_accent_muted
        } else {
            colors.on_surface_muted
        };
        let tile_bg = if selected {
            colors.accent_tile
        } else {
            colors.surface
        };

        // 用户能改的那几项一直显示在行尾：属于哪个子模块、跑过几次、哪个键能唤起。
        // 「一次没跑过」「没绑键」也照实写，行不会因为计数从此有了值就换一副样子。
        let section = div()
            .text_xs()
            .text_color(detail_color)
            .child(self.command.section.label());
        let uses = div()
            .text_xs()
            .text_color(detail_color)
            .child(strings::launcher::uses(self.state.uses));
        let shortcut = match self.state.shortcut.clone() {
            Some(chord) => div()
                .px_1p5()
                .py_0p5()
                .rounded(px(4.))
                .bg(tile_bg)
                .text_xs()
                .text_color(chip_color)
                .child(shortcut_label(&chord))
                .into_any_element(),
            None => div()
                .text_xs()
                .text_color(colors.faint)
                .child(strings::launcher::NO_SHORTCUT)
                .into_any_element(),
        };

        // 外层留 1px 上下留白，高亮块因此彼此分开：相邻两行的圆角不再压在一起。
        div().w_full().py(px(1.)).child(
            h_flex()
                .id(SharedString::from(format!("command-row-{}", self.index)))
                .w_full()
                .h(px(50.))
                .px_3()
                .gap_3()
                .rounded(px(8.))
                .when(selected, move |this| this.bg(colors.accent))
                .when(!selected, move |this| {
                    this.hover(move |style| style.bg(colors.hover))
                })
                .child(
                    div()
                        .flex_shrink_0()
                        .size_8()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(8.))
                        .bg(tile_bg)
                        .child(Icon::new(self.command.icon.clone()).size_4().text_color(
                            if selected {
                                colors.accent_foreground
                            } else {
                                colors.foreground
                            },
                        )),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_0p5()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(name_color)
                                .child(self.command.name),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .overflow_hidden()
                                .text_ellipsis()
                                .text_xs()
                                .text_color(detail_color)
                                .child(self.command.description),
                        ),
                )
                .child(
                    h_flex()
                        .flex_shrink_0()
                        .gap_2()
                        .child(section)
                        .child(uses)
                        .child(shortcut),
                ),
        )
    }
}

/// 主页列表，数据来自命令目录。
pub(crate) struct ToolDelegate {
    pub(crate) query: String,
    pub(crate) selected: usize,
    /// 设置里每条命令的状态，过滤和绘制都看它，不碰全局值。
    pub(crate) states: CommandStates,
    /// 搜索词一变就通知它，页头那点信息要跟着动。
    pub(crate) view: Option<WeakEntity<Launcher>>,
}

impl ToolDelegate {
    pub(crate) fn new(states: CommandStates) -> Self {
        Self {
            query: String::new(),
            selected: 0,
            states,
            view: None,
        }
    }

    /// 用户留在主页、且搜索词能对上的所有命令。
    fn visible(&self) -> Vec<&'static catalogue::Command> {
        let mut commands = catalogue::COMMANDS
            .iter()
            .filter(|command| self.enabled(command) && self.matches(command))
            .collect::<Vec<_>>();
        // Most-used commands first; ties are stable and deterministic across runs.
        commands.sort_by(|left, right| {
            self.state(right)
                .uses
                .cmp(&self.state(left).uses)
                .then_with(|| left.section.key().cmp(right.section.key()))
                .then_with(|| left.id.cmp(right.id))
        });
        commands
    }

    /// 这条命令允不允许出现在主页。
    fn enabled(&self, command: &catalogue::Command) -> bool {
        self.state(command).enabled
    }

    /// 这条命令的状态，没有记录就是默认值。
    pub(crate) fn state(&self, command: &catalogue::Command) -> CommandState {
        self.states
            .get(&(command.section.key(), command.id))
            .cloned()
            .unwrap_or_default()
    }

    /// 不区分大小写的匹配：搜索词按空白切成若干段，每一段都要在命令的可搜索文本里出现。
    fn matches(&self, command: &catalogue::Command) -> bool {
        let query = self.query.trim();
        if query.is_empty() {
            return true;
        }
        let haystack = format!(
            "{} {} {}",
            command.name, command.description, command.keyword
        )
        .to_lowercase();
        query
            .split_whitespace()
            .all(|token| haystack.contains(&token.to_lowercase()))
    }

    pub(crate) fn visible_count(&self) -> usize {
        self.visible().len()
    }

    pub(crate) fn selected_command(&self) -> Option<&'static catalogue::Command> {
        self.visible().get(self.selected).copied()
    }
}

impl ListDelegate for ToolDelegate {
    type Item = ToolRow;

    fn perform_search(
        &mut self,
        query: &str,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Task<()> {
        self.query = query.trim().to_string();
        self.selected = 0;
        if let Some(view) = self.view.clone() {
            let app: &mut App = cx;
            // 延后：搜索词变化可能发生在面板自己正被更新的时候（唤出时会清空搜索词），
            // 从那里再更新一次会 panic。
            app.defer(move |cx| {
                view.update(cx, |_, cx| cx.notify()).ok();
            });
        }
        cx.notify();
        Task::ready(())
    }

    fn items_count(&self, _section: usize, _cx: &App) -> usize {
        self.visible_count()
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _window: &mut Window,
        _cx: &mut Context<ListState<Self>>,
    ) -> Option<Self::Item> {
        let row = ix.row;
        let command = *self.visible().get(row)?;
        Some(ToolRow {
            command,
            state: self.state(command),
            selected: row == self.selected,
            index: row,
        })
    }

    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        self.selected = ix.map(|path| path.row).unwrap_or(0);
        cx.notify();
    }

    fn confirm(
        &mut self,
        _secondary: bool,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        let Some(command) = self.selected_command() else {
            return;
        };
        run_command(command, cx);
    }

    fn cancel(&mut self, _window: &mut Window, cx: &mut Context<ListState<Self>>) {
        crate::app::hide_launcher(cx);
    }

    fn render_empty(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        // 空状态直接画在半透明面板上，用校验过醒目度的次要文字。
        let muted = UiColors::current(cx).muted_foreground;
        let label = if self.query.is_empty() {
            strings::launcher::EMPTY.to_string()
        } else {
            strings::launcher::no_match(&self.query)
        };
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_2()
            .child(Icon::new(IconName::Search).size_6().text_color(muted))
            .child(div().text_sm().text_color(muted).child(label))
    }
}

#[cfg(test)]
mod tests {
    use super::{CommandState, CommandStates, ToolDelegate};

    #[test]
    fn homepage_orders_by_uses_then_section_and_id() {
        let mut states = CommandStates::new();
        states.insert(
            ("system", "quit"),
            CommandState {
                uses: 2,
                ..CommandState::default()
            },
        );
        states.insert(
            ("system", "program-folder"),
            CommandState {
                uses: 2,
                ..CommandState::default()
            },
        );
        states.insert(
            ("about", "repository"),
            CommandState {
                uses: 5,
                ..CommandState::default()
            },
        );

        let visible = ToolDelegate::new(states).visible();
        assert_eq!(
            visible.iter().map(|command| command.id).collect::<Vec<_>>(),
            [
                "repository",
                "program-folder",
                "quit",
                "open-calculator",
                "open-settings"
            ]
        );
    }
}
