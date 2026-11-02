//! 主页的命令列表：数据、每一行长什么样、以及过滤和执行。

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use gpui_kit::component::list::{ListDelegate, ListState};
use gpui_kit::component::{Icon, IconName, IndexPath, Selectable, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, Context, FontWeight, IntoElement, RenderOnce, SharedString, Task, WeakEntity,
    Window, div, prelude::*, px,
};
use tools::matcher::{Match, MatchAction, MatchConfig, MatchKind};
use tools::settings::{Settings, ToolKind, UserTool};

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
pub(crate) const COPY_FEEDBACK: Duration = Duration::from_millis(1200);

/// 主页列表里的一项：命中的指令匹配，一条静态命令，或者用户自己加的一个工具。
#[derive(Clone)]
pub(crate) enum HomeItem {
    Match(Match),
    Command(&'static catalogue::Command),
    Tool(UserTool),
}

impl HomeItem {
    /// 排序时先按这个分组，再按 id。
    ///
    /// 静态命令用它所属的子模块键，工具用 `tool-*`：两边的键空间不相交，
    /// 所以组内比较永远发生在同一种东西之间。
    fn group(&self) -> (&'static str, i64, &'static str) {
        match self {
            // 指令匹配永远排在命令前面，不进这张表，这里只是给个占位值。
            Self::Match(_) => ("", 0, ""),
            Self::Command(command) => (command.section.key(), 0, command.id),
            Self::Tool(tool) => (tool.kind.section_key(), tool.id, ""),
        }
    }
}

/// 一条用户工具在列表里怎么画。
///
/// 静态命令那张行 [`ToolRow`] 用的是 `&'static Command`，工具的名字和路径都在
/// 数据库里，所以另起一个：字段是旧的，数据是这次渲染现拷贝的一份。
#[derive(IntoElement)]
pub(crate) struct ToolItemRow {
    pub(crate) tool: UserTool,
    pub(crate) selected: bool,
    /// 行号，只用来给命中区一个稳定的 id。
    pub(crate) index: usize,
}

impl Selectable for ToolItemRow {
    fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    fn is_selected(&self) -> bool {
        self.selected
    }
}

impl RenderOnce for ToolItemRow {
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
        let fallback = match self.tool.kind {
            ToolKind::File => gpui_kit::assets::IconName::FileText,
            ToolKind::Folder => gpui_kit::assets::IconName::Folder,
            ToolKind::Terminal => gpui_kit::assets::IconName::SquareTerminal,
        };
        // 有系统图标就用真的：资源管理器里那个 exe 长什么样，这里就什么样。
        let real_icon = crate::ui::icons::for_tool(self.tool.kind, &self.tool.target);
        let has_real_icon = real_icon.is_some();
        let icon = match real_icon {
            Some(image) => gpui_kit::img(image)
                .w(px(16.))
                .h(px(16.))
                .into_any_element(),
            None => Icon::new(fallback)
                .size_4()
                .text_color(if selected {
                    colors.accent_foreground
                } else {
                    colors.foreground
                })
                .into_any_element(),
        };
        let shortcut = match self.tool.shortcut.clone() {
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

        div().w_full().py(px(1.)).child(
            h_flex()
                .id(SharedString::from(format!("tool-row-{}", self.index)))
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
                        // 有真图标的条目不再垫底色：图标自己带形状和留白，再画一块
                        // 方块就成了「图标有背景」。（内置矢量图标还是要这块底，
                        // 否则细线条直接浮在面板上看不清。）
                        .when(has_real_icon == false, move |this| this.bg(tile_bg))
                        .child(icon),
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
                                .text_color(name_color)
                                .child(self.tool.name.clone()),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .overflow_hidden()
                                .text_ellipsis()
                                .text_xs()
                                .text_color(detail_color)
                                .child(self.tool.target.clone()),
                        ),
                )
                .child(
                    h_flex()
                        .flex_shrink_0()
                        .gap_2()
                        .child(
                            div()
                                .text_xs()
                                .text_color(detail_color)
                                .child(strings::tools_page::kind_label(self.tool.kind)),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(detail_color)
                                .child(strings::common::uses(self.tool.uses)),
                        )
                        .child(shortcut),
                ),
        )
    }
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

/// 主页列表的一行：指令匹配、一条命令，或者一条用户工具。
pub(crate) enum HomeRow {
    Match(MatchRow),
    Command(ToolRow),
    Tool(ToolItemRow),
}

impl Selectable for HomeRow {
    fn selected(mut self, selected: bool) -> Self {
        match &mut self {
            HomeRow::Match(row) => row.selected = selected,
            HomeRow::Command(row) => row.selected = selected,
            HomeRow::Tool(row) => row.selected = selected,
        }
        self
    }

    fn is_selected(&self) -> bool {
        match self {
            HomeRow::Match(row) => row.selected,
            HomeRow::Command(row) => row.selected,
            HomeRow::Tool(row) => row.selected,
        }
    }
}

impl IntoElement for HomeRow {
    type Element = AnyElement;

    fn into_element(self) -> AnyElement {
        match self {
            HomeRow::Match(row) => row.into_any_element(),
            HomeRow::Command(row) => row.into_any_element(),
            HomeRow::Tool(row) => row.into_any_element(),
        }
    }
}

/// 主页列表，数据来自命令目录、用户自己加的工具，以及当前命中的指令匹配。
pub(crate) struct ToolDelegate {
    pub(crate) query: String,
    pub(crate) selected: usize,
    /// 设置里每条命令的状态，过滤和绘制都看它，不碰全局值。
    pub(crate) states: CommandStates,
    /// 用户自己加的工具。它们和静态命令一起排序、一起过滤。
    pub(crate) tools: Vec<UserTool>,
    /// 哪些指令匹配开着，外加算式精度。
    pub(crate) config: MatchConfig,
    /// 当前搜索词命中的指令匹配，排在命令前面。
    pub(crate) matches: Vec<Match>,
    /// 当前能看到的可执行项（命令 + 工具），已经按次数排好。
    ///
    /// 缓存着不让 `item_count` / `item_at` / `render_item` 各自再算一遍：
    /// 一次渲染里这几个会被问好几轮，现算就是每轮都排一次序。
    rows: Vec<HomeItem>,
    /// 刚复制过的命中（标题 + 时间），用来亮一小会儿「已复制」。
    pub(crate) copied: Option<(String, Instant)>,
    /// 搜索词一变就通知它，页头那点信息要跟着动。
    pub(crate) view: Option<WeakEntity<Launcher>>,
}

impl ToolDelegate {
    pub(crate) fn new(states: CommandStates, config: MatchConfig) -> Self {
        let mut delegate = Self {
            query: String::new(),
            selected: 0,
            states,
            tools: Vec::new(),
            config,
            matches: Vec::new(),
            rows: Vec::new(),
            copied: None,
            view: None,
        };
        delegate.rebuild();
        delegate
    }

    /// 命令状态和用户工具一起换掉（设置页改完东西之后）。
    pub(crate) fn set_catalogue(&mut self, states: CommandStates, tools: Vec<UserTool>) {
        self.states = states;
        self.tools = tools;
        self.rebuild();
    }

    /// 换了匹配开关：重算当前命中的匹配，并把高亮挪回第一项。
    pub(crate) fn set_config(&mut self, config: MatchConfig) {
        if self.config == config {
            return;
        }
        self.config = config;
        self.rebuild();
        self.selected = 0;
    }

    /// 按当前搜索词、匹配开关和设置重算整张列表。
    fn rebuild(&mut self) {
        self.matches = tools::matcher::matches(&self.query, &self.config);

        let mut rows = Vec::new();
        for command in catalogue::COMMANDS {
            if self.enabled(command) && self.matches_command(command) {
                rows.push(HomeItem::Command(command));
            }
        }
        for tool in &self.tools {
            if tool.enabled && self.matches_tool(tool) {
                rows.push(HomeItem::Tool(tool.clone()));
            }
        }
        // 用得多的排前面；次数一样时按分组和 id 排，保证每次启动顺序都一样。
        rows.sort_by(|left, right| {
            self.uses(right)
                .cmp(&self.uses(left))
                .then_with(|| left.group().0.cmp(right.group().0))
                .then_with(|| left.group().1.cmp(&right.group().1))
                .then_with(|| left.group().2.cmp(right.group().2))
        });
        self.rows = rows;
        self.clamp_selection();
    }

    /// 这一项跑过多少次：命令读设置镜像，工具读它自己那份。
    fn uses(&self, item: &HomeItem) -> u64 {
        match item {
            HomeItem::Match(_) => 0,
            HomeItem::Command(command) => self.state(command).uses,
            HomeItem::Tool(tool) => tool.uses,
        }
    }

    /// 高亮不能停在已经不在列表里的行上。
    fn clamp_selection(&mut self) {
        let count = self.matches.len() + self.rows.len();
        self.selected = self.selected.min(count.saturating_sub(1));
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

    /// 不区分大小写的匹配：搜索词按空白切成若干段，每一段都要在可搜索文本里出现。
    fn matches_command(&self, command: &catalogue::Command) -> bool {
        self.matches_text(&format!(
            "{} {} {}",
            command.name, command.description, command.keyword
        ))
    }

    /// 用户工具的可搜索文本是「名字 + 路径（或命令）」：名字是自己起的，
    /// 路径是当初挑的那个文件，两个都该能搜到。
    fn matches_tool(&self, tool: &UserTool) -> bool {
        self.matches_text(&format!("{} {}", tool.name, tool.target))
    }

    fn matches_text(&self, haystack: &str) -> bool {
        let query = self.query.trim();
        if query.is_empty() {
            return true;
        }
        let haystack = haystack.to_lowercase();
        query
            .split_whitespace()
            .all(|token| haystack.contains(&token.to_lowercase()))
    }

    /// 列表总行数：命中的指令匹配 + 命令 + 用户工具。
    pub(crate) fn item_count(&self) -> usize {
        self.matches.len() + self.rows.len()
    }

    /// 只看可执行项的行数（页脚「N 条命令」用它，不含指令匹配）。
    pub(crate) fn command_count(&self) -> usize {
        self.rows.len()
    }

    /// 第 `row` 行是哪一项：先排命中的匹配，再排命令和工具。
    fn item_at(&self, row: usize) -> Option<HomeItem> {
        if row < self.matches.len() {
            self.matches.get(row).cloned().map(HomeItem::Match)
        } else {
            self.rows.get(row - self.matches.len()).cloned()
        }
    }

    /// 当前高亮的那一项。
    pub(crate) fn selected_item(&self) -> Option<HomeItem> {
        self.item_at(self.selected)
    }

    /// 网格布局要的那一份：只有命令和工具，指令匹配不在里面。
    pub(crate) fn command_items(&self) -> &[HomeItem] {
        &self.rows
    }

    /// 指令匹配占了几条（网格布局里它们仍是行，排在方格上面）。
    pub(crate) fn match_count(&self) -> usize {
        self.matches.len()
    }

    /// 指令匹配本身，网格布局自己画那几行。
    pub(crate) fn matches(&self) -> &[Match] {
        &self.matches
    }

    /// 直接改高亮。
    ///
    /// 网格布局的方向键是面板自己算的（`List` 只管一行一条那套上下），所以得有个
    /// 不经过 `ListState` 的入口。越界一律夹到最后一条。
    pub(crate) fn select(&mut self, index: usize) {
        self.selected = index.min(self.item_count().saturating_sub(1));
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
        self.rebuild();
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
            HomeItem::Tool(tool) => HomeRow::Tool(ToolItemRow {
                tool,
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
            HomeItem::Tool(tool) => {
                let app: &mut App = cx;
                crate::app::run_tool(tool.id, app);
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
    use super::{CommandState, CommandStates, HomeItem, ToolDelegate};
    use tools::matcher::MatchConfig;
    use tools::settings::{ToolKind, UserTool};

    /// 列表里每条命令的 id（工具按 `tool:<id>` 写），按现在的顺序。
    fn order(delegate: &ToolDelegate) -> Vec<String> {
        (0..delegate.command_count())
            .filter_map(|row| match delegate.item_at(delegate.matches.len() + row) {
                Some(HomeItem::Command(command)) => Some(command.id.to_string()),
                Some(HomeItem::Tool(tool)) => Some(format!("tool:{}", tool.id)),
                _ => None,
            })
            .collect()
    }

    fn two_used_commands() -> CommandStates {
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
        states
    }

    #[test]
    fn homepage_orders_by_uses_then_section_and_id() {
        let delegate = ToolDelegate::new(two_used_commands(), MatchConfig::default());
        assert_eq!(
            order(&delegate),
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
                "open-settings",
                "user-tools"
            ]
        );
    }

    #[test]
    fn user_tools_take_part_in_the_same_ranking() {
        let tools = vec![
            UserTool {
                id: 1,
                kind: ToolKind::File,
                name: "账单".to_string(),
                target: r"D:\文档\账单.xlsx".to_string(),
                enabled: true,
                uses: 9,
                shortcut: None,
                open_default: false,
                start_dir: None,
                quiet: false,
            },
            UserTool {
                id: 2,
                kind: ToolKind::Terminal,
                name: "构建".to_string(),
                target: "cargo build".to_string(),
                enabled: true,
                uses: 0,
                shortcut: None,
                open_default: false,
                start_dir: None,
                quiet: false,
            },
        ];
        let mut delegate = ToolDelegate::new(two_used_commands(), MatchConfig::default());
        delegate.set_catalogue(two_used_commands(), tools);

        let ordered = order(&delegate);
        // 用了 9 次的工具排在用了 5 次的命令前面；一次没用过的都落到末尾。
        // （新加的「自定义条目」命令也是 0 次，同样在尾部，所以只看倒数两个。）
        assert_eq!(ordered.first().map(String::as_str), Some("tool:1"));
        assert_eq!(ordered.get(1).map(String::as_str), Some("repository"));
        let tail = &ordered[ordered.len() - 2..];
        assert!(
            tail.iter().any(|id| id == "tool:2"),
            "没用过的工具该在末尾：{ordered:?}"
        );
    }

    #[test]
    fn a_disabled_or_filtered_tool_leaves_the_list() {
        let tools = vec![
            UserTool {
                id: 1,
                kind: ToolKind::File,
                name: "账单".to_string(),
                target: r"D:\文档\账单.xlsx".to_string(),
                enabled: false,
                uses: 9,
                shortcut: None,
                open_default: false,
                start_dir: None,
                quiet: false,
            },
            UserTool {
                id: 2,
                kind: ToolKind::Terminal,
                name: "构建".to_string(),
                target: "cargo build".to_string(),
                enabled: true,
                uses: 0,
                shortcut: None,
                open_default: false,
                start_dir: None,
                quiet: false,
            },
        ];
        let mut delegate = ToolDelegate::new(CommandStates::new(), MatchConfig::default());
        delegate.set_catalogue(CommandStates::new(), tools);
        assert!(!order(&delegate).iter().any(|id| id == "tool:1"));

        // 名字和路径都算搜索词。
        delegate.query = "cargo".to_string();
        delegate.rebuild();
        assert_eq!(order(&delegate), ["tool:2"]);
        delegate.query = "构建".to_string();
        delegate.rebuild();
        assert_eq!(order(&delegate), ["tool:2"]);
        delegate.query = "xlsx".to_string();
        delegate.rebuild();
        assert!(order(&delegate).is_empty());
    }
}
