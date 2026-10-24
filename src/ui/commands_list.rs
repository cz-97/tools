//! 主页的命令列表：数据、每一行长什么样、以及过滤和执行。

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use gpui_kit::component::list::{ListDelegate, ListState};
use gpui_kit::component::{Icon, IconName, IndexPath, Selectable, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, Context, FontWeight, IntoElement, RenderOnce, SharedString, Task, WeakEntity,
    Window, div, prelude::*, px,
};
use gpui_tools::matcher::{Match, MatchAction, MatchConfig, MatchKind};
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
            .child(strings::common::uses(self.state.uses));
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

/// 命中行复制成功后，「已复制」亮多久。
const COPY_FEEDBACK: Duration = Duration::from_millis(1200);

/// 主页列表里的一项：命中的指令匹配，或一条命令。
pub(crate) enum HomeItem {
    Match(Match),
    Command(&'static catalogue::Command),
}

/// 一条指令匹配在列表里怎么画。
#[derive(IntoElement)]
pub(crate) struct MatchRow {
    pub(crate) item: Match,
    pub(crate) selected: bool,
    /// 行号，只用来给命中区一个稳定的 id。
    pub(crate) index: usize,
    /// 刚复制过：副行短暂换成「已复制」。
    pub(crate) copied: bool,
}

impl RenderOnce for MatchRow {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = UiColors::current(cx);
        let selected = self.selected;
        let actionable = !matches!(&self.item.action, MatchAction::None);

        let title_color = if selected {
            colors.accent_foreground
        } else if actionable {
            colors.foreground
        } else {
            colors.muted_foreground
        };
        let detail_color = if selected {
            colors.on_accent_muted
        } else {
            colors.muted_foreground
        };
        let tile_bg = if selected {
            colors.accent_tile
        } else {
            colors.surface
        };
        let icon = match self.item.kind {
            MatchKind::Calculator => gpui_kit::assets::IconName::Calculator,
            MatchKind::Amount => gpui_kit::assets::IconName::Banknote,
            MatchKind::Url => gpui_kit::assets::IconName::Link,
            MatchKind::Path => gpui_kit::assets::IconName::FolderOpen,
        };
        let hint = match &self.item.action {
            MatchAction::Copy(_) => strings::matcher::HINT_COPY,
            MatchAction::Open(_) => strings::matcher::HINT_OPEN,
            MatchAction::None => "",
        };
        let detail: SharedString = if self.copied {
            strings::matcher::COPIED.into()
        } else {
            self.item.description.clone().into()
        };

        div().w_full().py(px(1.)).child(
            h_flex()
                .id(SharedString::from(format!("match-row-{}", self.index)))
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
                        .child(Icon::new(icon).size_4().text_color(if selected {
                            colors.accent_foreground
                        } else {
                            colors.foreground
                        })),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_0p5()
                        .child(
                            div()
                                .min_w_0()
                                .overflow_hidden()
                                .text_ellipsis()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(title_color)
                                .child(self.item.title),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .overflow_hidden()
                                .text_ellipsis()
                                .text_xs()
                                .text_color(detail_color)
                                .child(detail),
                        ),
                )
                .when(!hint.is_empty(), move |this| {
                    this.child(
                        div()
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(detail_color)
                            .child(hint),
                    )
                }),
        )
    }
}

/// 主页列表的一行：既可能是指令匹配，也可能是命令。
pub(crate) enum HomeRow {
    Match(MatchRow),
    Command(ToolRow),
}

impl Selectable for HomeRow {
    fn selected(mut self, selected: bool) -> Self {
        match &mut self {
            HomeRow::Match(row) => row.selected = selected,
            HomeRow::Command(row) => row.selected = selected,
        }
        self
    }

    fn is_selected(&self) -> bool {
        match self {
            HomeRow::Match(row) => row.selected,
            HomeRow::Command(row) => row.selected,
        }
    }
}

impl IntoElement for HomeRow {
    type Element = AnyElement;

    fn into_element(self) -> AnyElement {
        match self {
            HomeRow::Match(row) => row.into_any_element(),
            HomeRow::Command(row) => row.into_any_element(),
        }
    }
}

/// 主页列表，数据来自命令目录与当前命中的指令匹配。
pub(crate) struct ToolDelegate {
    pub(crate) query: String,
    pub(crate) selected: usize,
    /// 设置里每条命令的状态，过滤和绘制都看它，不碰全局值。
    pub(crate) states: CommandStates,
    /// 哪些指令匹配开着，外加算式精度。
    pub(crate) config: MatchConfig,
    /// 当前搜索词命中的指令匹配，排在命令前面。
    pub(crate) matches: Vec<Match>,
    /// 刚复制过的命中（标题 + 时间），用来亮一小会儿「已复制」。
    pub(crate) copied: Option<(String, Instant)>,
    /// 搜索词一变就通知它，页头那点信息要跟着动。
    pub(crate) view: Option<WeakEntity<Launcher>>,
}

impl ToolDelegate {
    pub(crate) fn new(states: CommandStates, config: MatchConfig) -> Self {
        Self {
            query: String::new(),
            selected: 0,
            states,
            config,
            matches: Vec::new(),
            copied: None,
            view: None,
        }
    }

    /// 换了匹配开关：重算当前命中的匹配，并把高亮挪回第一项。
    pub(crate) fn set_config(&mut self, config: MatchConfig) {
        if self.config == config {
            return;
        }
        self.config = config;
        self.recompute_matches();
        self.selected = 0;
    }

    /// 按当前搜索词和开关重算命中的指令匹配。
    fn recompute_matches(&mut self) {
        self.matches = gpui_tools::matcher::matches(&self.query, &self.config);
    }

    /// 用户留在主页、且搜索词能对上的所有命令。
    fn visible_commands(&self) -> Vec<&'static catalogue::Command> {
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

    /// 列表总行数：命中的指令匹配 + 命令。
    pub(crate) fn item_count(&self) -> usize {
        self.matches.len() + self.visible_commands().len()
    }

    /// 只看命令的行数（页脚「N 条命令」用它）。
    pub(crate) fn command_count(&self) -> usize {
        self.visible_commands().len()
    }

    /// 第 `row` 行是哪一项：先排命中的匹配，再排命令。
    fn item_at(&self, row: usize) -> Option<HomeItem> {
        if row < self.matches.len() {
            self.matches.get(row).cloned().map(HomeItem::Match)
        } else {
            self.visible_commands()
                .get(row - self.matches.len())
                .copied()
                .map(HomeItem::Command)
        }
    }

    /// 当前高亮的那一项。
    pub(crate) fn selected_item(&self) -> Option<HomeItem> {
        self.item_at(self.selected)
    }
}

impl ListDelegate for ToolDelegate {
    type Item = HomeRow;

    fn perform_search(
        &mut self,
        query: &str,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Task<()> {
        self.query = query.trim().to_string();
        self.selected = 0;
        self.recompute_matches();
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
        self.item_count()
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _window: &mut Window,
        _cx: &mut Context<ListState<Self>>,
    ) -> Option<Self::Item> {
        let row = ix.row;
        let item = self.item_at(row)?;
        let selected = row == self.selected;
        Some(match item {
            HomeItem::Match(matched) => {
                let copied = self.copied.as_ref().is_some_and(|(title, at)| {
                    *title == matched.title && at.elapsed() < COPY_FEEDBACK
                });
                HomeRow::Match(MatchRow {
                    item: matched,
                    selected,
                    index: row,
                    copied,
                })
            }
            HomeItem::Command(command) => HomeRow::Command(ToolRow {
                command,
                state: self.state(command),
                selected,
                index: row,
            }),
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
        let Some(item) = self.selected_item() else {
            return;
        };
        match item {
            HomeItem::Command(command) => {
                let app: &mut App = cx;
                run_command(command, app);
            }
            HomeItem::Match(matched) => {
                let app: &mut App = cx;
                crate::app::with_launcher(app, move |launcher, window, cx| {
                    launcher.activate_match(matched, window, cx)
                });
            }
        }
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
    use gpui_tools::matcher::MatchConfig;

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

        let visible = ToolDelegate::new(states, MatchConfig::default()).visible_commands();
        assert_eq!(
            visible.iter().map(|command| command.id).collect::<Vec<_>>(),
            [
                "repository",
                "program-folder",
                "quit",
                "bookmarks",
                "history",
                "open-calculator",
                "github-projects",
                "github-search",
                "github-stars",
                "config-folder",
                "open-settings"
            ]
        );
    }
}
