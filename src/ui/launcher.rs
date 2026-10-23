//! 面板本体：页头、页脚、命令列表，以及面板内的导航。

use std::time::{Duration, Instant};

use gpui_kit::component::IndexPath;
use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::component::list::ListState;
use gpui_kit::component::slider::{SliderEvent, SliderState};
use gpui_kit::{
    App, ClipboardItem, Context, Entity, Focusable, Subscription, Task, Window, prelude::*,
};
use gpui_tools::autostart;
use gpui_tools::cache;
use gpui_tools::calculator;
use gpui_tools::firefox;
use gpui_tools::github;
use gpui_tools::matcher::{Match, MatchAction, MatchConfig, MatchKind};
use gpui_tools::settings::Settings;
use gpui_tools::shell;

use crate::app::{
    LauncherPanel, apply_theme, hide_launcher, run_command, store_settings, stored_settings,
    with_launcher,
};
use crate::catalogue;
use crate::shortcuts::{
    ShortcutOwner, apply_command_shortcuts, apply_summon_shortcut, chord_from_keystroke,
};
use crate::strings;
use crate::ui::commands_list::{ToolDelegate, command_states};
use crate::ui::lists::{Entry, EntryDelegate, favicon_image};
use gpui_tools::settings::Theme as ColorScheme;

// 绘制拆到 `view.rs`：页头、页脚与整页骨架在那里，这里只管状态和行为。
mod view;

/// 页头最矮也要画这么高，不管搜索框设成多少。
const HEADER_MIN_HEIGHT: f32 = 52.;
/// 页头留给搜索框上下的余量，两个方向合起来算。
const HEADER_SLACK: f32 = 26.;
/// 设置页左侧子模块列的宽度。
pub(crate) const SIDEBAR_WIDTH: f32 = 148.;
/// 命令页里左侧子模块列的宽度。
pub(crate) const MODULE_WIDTH: f32 = 124.;
/// 输入框组件在内部画掉的左右留白（小号是 8 + 8），输入框得自己付这笔账。
const SEARCH_FIELD_PADDING: f32 = 16.;
/// 空输入框给光标留的宽度。再宽就是白占拖动区。
const SEARCH_FIELD_CARET: f32 = 4.;
/// 按量出来的文字宽度再多留一点，免得差一个像素就把字滚出视野。
const SEARCH_FIELD_SLACK: f32 = 14.;
/// 输入框最宽到这儿，剩下的地方留给图标、提示和设置按钮。
const SEARCH_FIELD_MAX: f32 = 560.;
/// 输入框里文字的字号，跟组件的中号输入框一致。
const SEARCH_TEXT_SIZE: f32 = 14.;
/// 面板多久看一眼自己还在不在前台。
const FOCUS_CHECK_INTERVAL: Duration = Duration::from_millis(60);
/// 连续几次没拿到前台才收起。留几次，是为了躲开「刚显示、前台还没转过来」的空档。
const FOCUS_LOST_SAMPLES: u8 = 3;
/// 系统给的窗口圆角半径，**物理像素**。
///
/// Windows 11 的 `DWMWCP_ROUND` 固定 8px、不随 DPI 缩放（从截图里量出来的就是 8）。
/// 面板自己也得按同一个半径圆角，那圈 1px 高光边才会贴着弧线走；
/// 画在方角矩形上就会被窗口的圆角裁断，四个角上各缺一小段。
const CORNER_RADIUS: f32 = 8.;
/// 计算器一行最多长到几行字。再长的公式横着也能看全，竖着会把别的行顶出面板。
const CALCULATOR_MAX_ROWS: usize = 8;
/// 「已复制」这四个字亮多久。
const COPY_FEEDBACK: Duration = Duration::from_millis(1200);

/// 面板当前显示哪一页。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Page {
    /// 命令列表。
    Commands,
    /// 设置，打开在指定的子模块上。
    Settings(SettingsTab),
    /// 计算器：和设置平级的一级页面，从主页「计算器」命令进入。
    Calculator,
    /// 工具列表页：书签、历史、个人项目、星标、查询仓库。
    List(ListKind),
}

/// 工具列表页有哪几页。
///
/// 五个页面共用一张列表（[`EntryDelegate`]），差别只在数据来源和页头那枚按钮。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ListKind {
    /// 火狐系浏览器的书签。
    Bookmarks,
    /// 火狐系浏览器的浏览历史。
    History,
    /// 自己的 GitHub 仓库。
    Projects,
    /// 星标仓库。
    Stars,
    /// 按关键词搜仓库。
    Search,
}

impl ListKind {
    /// 页头标题。
    pub(crate) fn title(self) -> &'static str {
        match self {
            Self::Bookmarks => strings::common::BOOKMARKS,
            Self::History => strings::common::HISTORY,
            Self::Projects => strings::common::PROJECTS,
            Self::Stars => strings::common::STARS,
            Self::Search => strings::common::REPO_SEARCH,
        }
    }

    /// 页头图标。
    pub(crate) fn icon(self) -> gpui_kit::assets::IconName {
        match self {
            Self::Bookmarks => gpui_kit::assets::IconName::Bookmark,
            Self::History => gpui_kit::assets::IconName::Clock,
            Self::Projects => gpui_kit::assets::IconName::FolderGit2,
            Self::Stars => gpui_kit::assets::IconName::Star,
            Self::Search => gpui_kit::assets::IconName::Search,
        }
    }

    /// 这一页的数据是不是 GitHub 仓库（缓存优先，页头有刷新）。
    pub(crate) fn is_repos(self) -> bool {
        matches!(self, Self::Projects | Self::Stars)
    }

    /// 缓存键里的那一段；搜索页不缓存。
    fn cache_name(self) -> &'static str {
        match self {
            Self::Projects => "projects",
            Self::Stars => "stars",
            _ => "none",
        }
    }
}

/// 设置里的子模块，顺序就是左侧列的顺序。
///
/// `Tools` 是一个容器：它本身不含设置项，而是把计算器等工具收在下面的子标签页里
/// （见 `ToolTab`）。其余四项与命令页给命令分组的子模块一一对应。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SettingsTab {
    System,
    Commands,
    Appearance,
    Tools,
    Matchers,
    About,
}

impl SettingsTab {
    /// 全部子模块，按界面上的顺序。
    pub(crate) const ALL: [Self; 6] = [
        Self::System,
        Self::Commands,
        Self::Appearance,
        Self::Tools,
        Self::Matchers,
        Self::About,
    ];

    /// 界面上的名字。
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::System => strings::sections::SYSTEM,
            Self::Commands => strings::sections::COMMANDS,
            Self::Appearance => strings::sections::APPEARANCE,
            Self::Tools => strings::sections::TOOLS,
            Self::Matchers => strings::sections::MATCHERS,
            Self::About => strings::sections::ABOUT,
        }
    }

    /// 界面上的图标。
    pub(crate) fn icon(self) -> gpui_kit::assets::IconName {
        match self {
            Self::System => catalogue::Section::System.icon(),
            Self::Commands => catalogue::Section::Commands.icon(),
            Self::Appearance => catalogue::Section::Appearance.icon(),
            Self::Tools => gpui_kit::assets::IconName::Toolbox,
            Self::Matchers => gpui_kit::assets::IconName::Sparkles,
            Self::About => catalogue::Section::About.icon(),
        }
    }

    /// 往后挪 `delta` 个，到底就停在最后一个。
    fn shifted(self, delta: isize) -> Self {
        let index = Self::ALL
            .iter()
            .position(|tab| *tab == self)
            .unwrap_or_default() as isize;
        Self::ALL[(index + delta).clamp(0, Self::ALL.len() as isize - 1) as usize]
    }
}

/// 设置里「工具」标签下面的子标签页。
///
/// 每个子标签是一个可配置的工具；加工具 = 给这个枚举加一项，右侧内容按它分派，
/// 左侧列表自动列出全部。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ToolTab {
    /// 计算器：和它的设置（比如精度）。
    Calculator,
    /// 火狐系浏览器：profile 目录在哪儿。
    Firefox,
    /// GitHub：用户名和令牌。
    Github,
}

impl ToolTab {
    /// 全部子标签，顺序就是左侧列表的顺序。
    pub(crate) const ALL: [Self; 3] = [Self::Calculator, Self::Firefox, Self::Github];

    /// 界面上的名字。
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Calculator => strings::launcher::CALCULATOR_TITLE,
            Self::Firefox => strings::launcher::FIREFOX_TITLE,
            Self::Github => strings::launcher::GITHUB_TITLE,
        }
    }

    /// 界面上的图标。
    pub(crate) fn icon(self) -> gpui_kit::assets::IconName {
        match self {
            Self::Calculator => gpui_kit::assets::IconName::Calculator,
            Self::Firefox => gpui_kit::assets::IconName::Globe,
            Self::Github => gpui_kit::assets::IconName::Github,
        }
    }
}

/// 录制时按下来的那组键该记到谁头上。
#[derive(Clone, Copy)]
pub(crate) enum RecordTarget {
    /// 唤出面板的全局热键。
    Summon,
    /// 某一条命令。
    Command(&'static catalogue::Command),
}

impl RecordTarget {
    /// 两个录制目标是不是同一项。
    pub(crate) fn same(self, other: Self) -> bool {
        match (self, other) {
            (Self::Summon, Self::Summon) => true,
            (Self::Command(a), Self::Command(b)) => std::ptr::eq(a, b),
            _ => false,
        }
    }

    /// 热键现在归的那个对象，是不是就是录制目标本身。
    fn is_owner(self, owner: ShortcutOwner) -> bool {
        match (self, owner) {
            (Self::Summon, ShortcutOwner::Summon) => true,
            (Self::Command(a), ShortcutOwner::Command(b)) => std::ptr::eq(a, b),
            _ => false,
        }
    }
}

/// 计算器里的一行：一个公式输入框，和它的订阅。
///
/// `id` 是稳定键，不是下标：行会被增删，下标会挪窝，订阅里记的下标会对不上行。
pub(crate) struct CalculatorRow {
    pub(crate) id: usize,
    pub(crate) input: Entity<TextareaState>,
    /// 订阅跟着行一起活；行没了它也没了。
    _events: Subscription,
}

/// 面板视图。
pub struct Launcher {
    pub(crate) list: Entity<ListState<ToolDelegate>>,
    pub(crate) search: Entity<InputState>,
    /// 已提交的搜索词，从输入框同步过来驱动列表。
    ///
    /// 输入法还在组词时这里没有那串字，所以搜索栏是按输入框自己的内容量的
    /// （见 `header`）。
    query: String,
    /// 当前这一页，页头、正文、页脚都跟着它。
    pub(crate) page: Page,
    /// 最后一次离开非命令页（设置或计算器）时停在哪一页、什么时候走的。
    /// 保留期内再唤出就回到那一页。
    last_page: Option<(Instant, Page)>,
    /// 盯着面板有没有失去前台的循环。每次唤起重开，旧的那条已经结束了。
    focus_watch: Option<Task<()>>,
    /// 命令页当前显示第几个子模块，`catalogue::Section::ALL` 的下标。
    pub(crate) section: usize,
    /// 工具标签下当前显示哪个子标签页（`ToolTab`）。
    pub(crate) tool_tab: ToolTab,
    /// 透明度滑动条。滑动的过程只改内存，库在面板收起时统一落盘。
    pub(crate) opacity: Entity<SliderState>,
    /// 计算器的公式行。最后一行永远是空行：它有内容，下面就自动补一个。
    pub(crate) calculator: Vec<CalculatorRow>,
    /// 下一行计算器行的 id。只增不减，行删了也不回收。
    next_row_id: usize,
    /// 「清空本行」清的是哪一行：用户最后一次点进去的那行。
    current_row: Option<usize>,
    /// 哪一行刚被复制过、什么时候复制的；用来亮一小会儿「已复制」。
    copied: Option<(usize, Instant)>,
    /// 正在录制的目标。
    recording: Option<RecordTarget>,
    /// 录制期间被拒绝的原因（那个键已经被谁占了）。按键换一组就清掉。
    conflict: Option<String>,
    /// 工具列表页的列表。五页共用一份，换页时把数据换掉。
    pub(crate) entries: Entity<ListState<EntryDelegate>>,
    /// 查询仓库页页头的搜索框。
    pub(crate) repo_search: Entity<InputState>,
    /// 已经发给 GitHub 的那个关键词：和搜索框内容一样时，回车才是「打开选中的仓库」。
    repo_query: String,
    /// 后台任务（读库、抓网络）。存着它任务才不会被丢掉。
    pending: Option<Task<()>>,
    /// 设置里那三个文本框（火狐目录、GitHub 用户名、令牌）。
    pub(crate) firefox_input: Entity<InputState>,
    pub(crate) github_user_input: Entity<InputState>,
    pub(crate) github_token_input: Entity<InputState>,
    /// 跟着视图活着的订阅，丢了输入框就不再上报用户敲了什么。
    _search_events: Subscription,
    _opacity_events: Subscription,
    _search_observation: Subscription,
    /// 查询仓库页搜索框的订阅。
    _repo_events: Subscription,
    /// 三个设置文本框的订阅（变化通知 + 失焦保存）。
    _settings_events: Vec<Subscription>,
}

impl Launcher {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings = stored_settings(cx);
        let states = command_states(&settings);
        let config = MatchConfig::from_settings(&settings);
        let list = cx.new(|cx| ListState::new(ToolDelegate::new(states, config), window, cx));
        let view = cx.weak_entity();
        list.update(cx, |state, _cx| {
            state.delegate_mut().view = Some(view);
        });

        let search = cx.new(|cx| InputState::new(window, cx));
        // 列表自带的搜索框装不下搜索栏的图标、拖动区和设置按钮，所以面板自己持有
        // 输入框，再把文字推给列表。
        let search_events = cx.subscribe(&search, |launcher, search, event, cx| {
            if matches!(event, InputEvent::Change) {
                let query = search.read(cx).value().to_string();
                launcher.search_changed(query, cx);
            }
        });
        // 输入法还在组词时不会有 Change 事件，但输入框里已经有那串字了，搜索栏得跟着变宽。
        // 状态每次变动都会通知，两个文本不一致就重画。
        let search_observation = cx.observe(&search, |launcher, search, cx| {
            if search.read(cx).value().as_ref() != launcher.query {
                cx.notify();
            }
        });

        let opacity = cx.new(|cx| {
            SliderState::new()
                .min(Settings::PANEL_OPACITY_MIN as f32)
                .max(Settings::PANEL_OPACITY_MAX as f32)
                .step(Settings::PANEL_OPACITY_STEP as f32)
                .default_value(stored_settings(cx).panel_opacity as f32)
        });
        // 拖动时每一帧都会来一次事件：只在内存里改，落盘交给面板收起的那次统一写。
        let opacity_events = cx.subscribe(&opacity, |launcher, _state, event, cx| {
            if matches!(event, SliderEvent::Change(_)) {
                let opacity = launcher.opacity.read(cx).value().start().round() as u32;
                launcher.preview_panel_opacity(opacity, cx);
            }
        });

        // 列表页一上来是空的：数据在 `open_list` 里按页装进来。
        let entries =
            cx.new(|cx| ListState::new(EntryDelegate::new(ListKind::Bookmarks), window, cx));

        // 查询仓库页的搜索框：和主页那个分开，两页的输入互不干扰。
        let repo_search =
            cx.new(|cx| InputState::new(window, cx).placeholder(strings::lists::SEARCH_HINT));
        // 搜索栏的宽度是按输入框里的文字量的，内容一变就得重画。
        let repo_events = cx.subscribe(&repo_search, |_, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });

        let firefox_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(strings::settings_page::FIREFOX_PROFILE_PLACEHOLDER)
                .default_value(settings.firefox_profile.clone().unwrap_or_default())
        });
        let github_user_input =
            cx.new(|cx| InputState::new(window, cx).default_value(settings.github_user.clone()));
        let github_token_input =
            cx.new(|cx| InputState::new(window, cx).default_value(settings.github_token.clone()));
        // 输入框失焦就落盘：用户点走、换标签、收起面板都算「改完了」。
        let settings_events = [&firefox_input, &github_user_input, &github_token_input]
            .into_iter()
            .map(|input| {
                cx.subscribe(input, |launcher, _input, event, cx| {
                    if matches!(event, InputEvent::Blur) {
                        launcher.commit_settings_inputs(cx);
                    }
                })
            })
            .collect::<Vec<_>>();

        let mut launcher = Self {
            list,
            search,
            query: String::new(),
            page: Page::Commands,
            last_page: None,
            focus_watch: None,
            section: 0,
            tool_tab: ToolTab::Calculator,
            opacity,
            calculator: Vec::new(),
            next_row_id: 0,
            current_row: None,
            copied: None,
            recording: None,
            conflict: None,
            entries,
            repo_search,
            repo_query: String::new(),
            pending: None,
            firefox_input,
            github_user_input,
            github_token_input,
            _search_events: search_events,
            _opacity_events: opacity_events,
            _search_observation: search_observation,
            _repo_events: repo_events,
            _settings_events: settings_events,
        };
        // 一开始就有一行空行占着：用户进来直接就能打公式。
        launcher.append_calculator_row(window, cx);
        // GPUI 默认不给焦点，用户点一下输入框之前，面板会把所有按键都吞掉。
        launcher.focus_search(window, cx);
        launcher.select_row(0, window, cx);
        launcher
    }

    /// 正在录制的目标。
    pub(crate) fn recording(&self) -> Option<RecordTarget> {
        self.recording
    }

    /// 正在录制的命令；录的是唤出热键时为 `None`。
    pub(crate) fn recording_command(&self) -> Option<&'static catalogue::Command> {
        match self.recording {
            Some(RecordTarget::Command(command)) => Some(command),
            _ => None,
        }
    }

    /// 是不是正在录唤出热键。
    pub(crate) fn recording_summon(&self) -> bool {
        matches!(self.recording, Some(RecordTarget::Summon))
    }

    /// 录制被拒绝的原因（「已被 X 占用」），没冲突时为 `None`。
    pub(crate) fn recording_conflict(&self) -> Option<&str> {
        self.conflict.as_deref()
    }

    /// 把焦点给搜索框，按键才能进去。
    fn focus_search(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.search.read(cx).focus_handle(cx).focus(window, cx);
    }

    /// 高亮某一行，但不滚动列表。
    fn select_row(&self, row: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.list.update(cx, |state, cx| {
            state.set_selected_index(Some(IndexPath::new(row)), window, cx);
        });
    }

    /// 把输入框里的文字当成新的搜索词，用来过滤列表。
    fn search_changed(&mut self, query: String, cx: &mut Context<Self>) {
        if self.query == query {
            return;
        }
        self.query = query;
        // 搜索栏是按这段文字画的，不能等列表。
        cx.notify();

        let value = self.query.trim().to_string();
        with_launcher(cx, move |launcher, window, cx| {
            launcher
                .list
                .update(cx, |list, cx| list.set_query(&value, window, cx));
            cx.notify();
        });
    }

    /// 上下键：命令页里移高亮，设置页里换子模块，列表页里换行；计算器页里不抢方向键
    /// （公式框自己用）。
    pub(crate) fn navigate(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        match self.page {
            Page::Commands => self.move_selection(delta, window, cx),
            Page::Settings(tab) => self.select_tab(tab.shifted(delta), window, cx),
            Page::Calculator => {}
            Page::List(_) => self.move_entry(delta, window, cx),
        }
    }

    /// 工具列表页里高亮挪 `delta` 行，夹在可见范围内。
    fn move_entry(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.entries.read(cx).delegate().len();
        if count == 0 {
            return;
        }
        let current = self.entries.read(cx).delegate().selected();
        let next = (current as isize + delta).clamp(0, count as isize - 1) as usize;
        if next == current {
            return;
        }
        self.entries.update(cx, |list, cx| {
            list.set_selected_index(Some(IndexPath::new(next)), window, cx);
            list.scroll_to_selected_item(window, cx);
        });
    }

    /// 高亮挪 `delta` 行，夹在可见命令范围内。
    fn move_selection(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.list.read(cx).delegate().item_count();
        if count == 0 {
            return;
        }
        let current = self.list.read(cx).delegate().selected;
        let next = (current as isize + delta).clamp(0, count as isize - 1) as usize;
        if next == current {
            return;
        }
        self.list.update(cx, |list, cx| {
            list.set_selected_index(Some(IndexPath::new(next)), window, cx);
            list.scroll_to_selected_item(window, cx);
        });
    }

    /// 开一段新会话：清搜索词、高亮第一行；保留期内再唤出的话，回到上次那个子模块。
    pub(crate) fn reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.query.is_empty() {
            self.search
                .update(cx, |field, cx| field.set_value("", window, cx));
            self.query.clear();
        }
        self.recording = None;
        self.conflict = None;
        self.refresh_catalogue(cx);
        self.list
            .update(cx, |list, cx| list.set_query("", window, cx));
        // 唤出时停在哪一页由记忆决定；列表页要重新装数据，所以走 `open_list`。
        match self.resumed_page(cx) {
            Some(Page::Calculator) => self.open_calculator(window, cx),
            Some(Page::Settings(tab)) => {
                self.page = Page::Settings(tab);
                // 设置页里没有默认输入框：焦点留在主页搜索框上，字会敲进看不见的地方。
                window.blur(cx);
            }
            Some(Page::List(kind)) => self.open_list(kind, window, cx),
            _ => {
                self.page = Page::Commands;
                self.focus_search(window, cx);
            }
        }
        self.select_row(0, window, cx);
        self.watch_focus(cx);
        cx.notify();
    }

    /// 别的窗口抢走前台就收起面板，启动器本来就该这样。
    ///
    /// GPUI 不报窗口激活事件，平台也不把那个状态交给上层，所以这里改成采样前台窗口。
    /// 面板一收起循环就结束，下次唤起重新起一条；而且必须曾经到过前台才允许收，
    /// 免得窗口被系统拒绝抬到前面时「刚出现就消失」。
    fn watch_focus(&mut self, cx: &mut Context<Self>) {
        self.focus_watch = Some(cx.spawn(async move |this, cx| {
            let mut was_in_front = false;
            let mut misses = 0u8;
            loop {
                cx.background_executor().timer(FOCUS_CHECK_INTERVAL).await;
                let watching = this.update(cx, |launcher, cx| {
                    let Some(panel) = cx.try_global::<LauncherPanel>().copied() else {
                        return false;
                    };
                    if !panel.window.is_visible() {
                        return false;
                    }
                    if panel.window.is_foreground() {
                        was_in_front = true;
                        misses = 0;
                        return true;
                    }
                    if !was_in_front {
                        return true;
                    }
                    misses += 1;
                    if misses < FOCUS_LOST_SAMPLES {
                        return true;
                    }
                    launcher.leave_on_focus_loss(cx);
                    false
                });
                if !watching.unwrap_or(false) {
                    break;
                }
            }
        }));
    }

    /// 因为用户去了别的窗口而收起。
    fn leave_on_focus_loss(&mut self, cx: &mut Context<Self>) {
        hide_launcher(cx);
    }

    /// 这次唤出该回到哪一页——离开得够近、还值得记着就回。
    ///
    /// 只回「非命令页」（设置或计算器）：命令页本来就是默认页，记它也只是多余。
    fn resumed_page(&self, cx: &Context<Self>) -> Option<Page> {
        let (at, page) = self.last_page?;
        let window = stored_settings(cx).resume_seconds;
        (window > 0 && at.elapsed() < Duration::from_secs(window.into())).then_some(page)
    }

    /// 收起面板时记下停在哪一页，`reset` 才有地方回去。
    ///
    /// **只看收起那一刻的页面**：停在设置/计算器就记住，停在命令页就忘掉上一次的子页面。
    /// 所以「进了设置、又 Esc 回到主页、再收起」之后重新唤出是主页，不会又跳回设置。
    pub(crate) fn remember_page(&mut self) {
        self.last_page = if matches!(self.page, Page::Commands) {
            None
        } else {
            Some((Instant::now(), self.page))
        };
    }

    /// 收起面板前收拾现场：记下停在哪一页，并结束录制。
    ///
    /// **录制不能跟着面板一起藏起来**：全局热键分发是按「有没有在录制」决定执行还是
    /// 报冲突的，面板藏了但录制还在，所有热键都会静默失效——连唤出面板的那一个都
    /// 按不回来，而按钮早就看不见了。
    pub(crate) fn leave(&mut self) {
        self.recording = None;
        self.conflict = None;
        self.remember_page();
    }

    /// 执行高亮的那一项：指令匹配（复制/打开）、一条命令、列表页的一行，或者设置页
    /// 顺手把改过的文本框存下来。
    pub(crate) fn activate_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.page {
            Page::Commands => {}
            Page::Settings(_) => {
                self.commit_settings_inputs(cx);
                return;
            }
            Page::Calculator => return,
            Page::List(ListKind::Search) => {
                // 搜索框里是新词就先搜；和上次一样时，回车是「打开选中的那个仓库」。
                let keyword = self.repo_search.read(cx).value().trim().to_string();
                if !keyword.is_empty() && keyword != self.repo_query {
                    self.run_search(keyword, cx);
                } else {
                    self.open_entry(cx);
                }
                return;
            }
            Page::List(_) => {
                self.open_entry(cx);
                return;
            }
        }
        match self.list.read(cx).delegate().selected_item() {
            Some(crate::ui::commands_list::HomeItem::Match(matched)) => {
                self.activate_match(matched, window, cx);
            }
            Some(crate::ui::commands_list::HomeItem::Command(command)) => run_command(command, cx),
            None => {}
        }
    }

    /// 打开列表页高亮的那一行。
    fn open_entry(&self, cx: &App) {
        self.entries.read(cx).delegate().open_selected();
    }

    /// 执行一条指令匹配：显示结果的复制走，执行动作的交给 shell。
    pub(crate) fn activate_match(
        &mut self,
        matched: Match,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match matched.action {
            MatchAction::Copy(text) => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                // 亮一小会儿「已复制」，复制没反馈用户会以为点了个寂寞。
                let title = matched.title;
                self.list.update(cx, |list, cx| {
                    list.delegate_mut().copied = Some((title.clone(), Instant::now()));
                    cx.notify();
                });
                cx.notify();
                cx.spawn(async move |launcher, cx| {
                    cx.background_executor().timer(COPY_FEEDBACK).await;
                    launcher
                        .update(cx, |launcher, cx| {
                            let still = launcher
                                .list
                                .read(cx)
                                .delegate()
                                .copied
                                .as_ref()
                                .is_some_and(|(current, _)| *current == title);
                            if still {
                                launcher.list.update(cx, |list, cx| {
                                    list.delegate_mut().copied = None;
                                    cx.notify();
                                });
                            }
                        })
                        .ok();
                })
                .detach();
            }
            MatchAction::Open(target) => shell::open(target),
            MatchAction::None => {}
        }
        // 点列表会把焦点从搜索框挪走；复制完把焦点还回去，用户能接着打字。
        self.focus_search(window, cx);
        cx.notify();
    }

    /// Esc 先从设置/计算器/列表页退出来，再收面板。
    pub(crate) fn escape(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(
            self.page,
            Page::Settings(_) | Page::Calculator | Page::List(_)
        ) {
            self.back_to_commands(window, cx);
        } else {
            hide_launcher(cx);
        }
    }

    /// 把命令列表换成设置页。
    pub(crate) fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.page = Page::Settings(SettingsTab::System);
        // 齿轮本身不吃焦点，搜索框还揣着焦点的话，字会敲进看不见的地方。
        window.blur(cx);
        cx.notify();
    }

    /// 回到命令列表，输入框准备好接收按键。
    ///
    /// 这里**不**记页面：Esc 退回命令页后用户还停在面板上，只有真正收起时才决定
    /// 要不要记得这一页（见 [`Self::remember_page`]）。
    pub(crate) fn back_to_commands(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // 离开列表页就把还在跑的任务丢掉：抓回来的数据不该塞进别的页面。
        self.pending = None;
        self.page = Page::Commands;
        self.focus_search(window, cx);
        cx.notify();
    }

    /// 换一个设置子模块。计算器本体已经不在设置里，这里不再需要为它移交焦点。
    pub(crate) fn select_tab(
        &mut self,
        tab: SettingsTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.page == Page::Settings(tab) {
            return;
        }
        // 换标签之前把文本框里改过的值存下来，顺手把焦点收干净。
        self.commit_settings_inputs(cx);
        window.blur(cx);
        self.page = Page::Settings(tab);
        cx.notify();
    }

    /// 工具标签里切到某个子标签（计算器、火狐、GitHub）。
    pub(crate) fn select_tool_tab(
        &mut self,
        tab: ToolTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tool_tab == tab {
            return;
        }
        self.commit_settings_inputs(cx);
        window.blur(cx);
        self.tool_tab = tab;
        cx.notify();
    }

    /// 把命令列表换成计算器页（主页上的「计算器」命令）。
    ///
    /// 计算器是和设置平级的一级页面，不再挂在设置下面。
    pub(crate) fn open_calculator(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.page = Page::Calculator;
        self.focus_calculator_last(window, cx);
        cx.notify();
    }

    /// 焦点给最后一行计算器，同时把它记成「本行」。
    fn focus_calculator_last(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(row) = self.calculator.last() else {
            return;
        };
        self.current_row = Some(row.id);
        row.input.read(cx).focus_handle(cx).focus(window, cx);
    }

    /// 打开一个工具列表页：先把数据装进去，再决定焦点落在哪儿。
    ///
    /// 书签和历史每次进来都重读一遍库（本地文件，很快）；个人项目和星标先把缓存放上来，
    /// 没有缓存才去网上抓；查询仓库页空着手进来，等用户敲关键词。
    pub(crate) fn open_list(
        &mut self,
        kind: ListKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 上一页还在跑的活儿先丢掉，结果不许串到这一页来。
        self.pending = None;
        self.page = Page::List(kind);
        self.repo_query.clear();
        self.entries.update(cx, |list, cx| {
            list.delegate_mut().reset(kind);
            cx.notify();
        });

        match kind {
            ListKind::Search => {
                self.repo_search
                    .update(cx, |input, cx| input.set_value("", window, cx));
                self.repo_search.read(cx).focus_handle(cx).focus(window, cx);
            }
            ListKind::Bookmarks | ListKind::History => {
                // 列表页没有输入框，焦点留着会敲进看不见的搜索框。
                window.blur(cx);
                self.load_firefox(kind, cx);
            }
            ListKind::Projects | ListKind::Stars => {
                window.blur(cx);
                self.load_repos(kind, false, cx);
            }
        }
        self.entries.update(cx, |list, cx| {
            list.set_selected_index(Some(IndexPath::new(0)), window, cx);
        });
        cx.notify();
    }

    /// 页头那枚刷新按钮：不管缓存，重新去源头取一份。
    pub(crate) fn refresh_list(&mut self, cx: &mut Context<Self>) {
        match self.page {
            Page::List(ListKind::Bookmarks) => self.load_firefox(ListKind::Bookmarks, cx),
            Page::List(ListKind::History) => self.load_firefox(ListKind::History, cx),
            Page::List(kind) if kind.is_repos() => self.load_repos(kind, true, cx),
            _ => {}
        }
    }

    /// 读书签或者历史。sqlite 是阻塞的，扔到后台线程去读。
    fn load_firefox(&mut self, kind: ListKind, cx: &mut Context<Self>) {
        self.commit_settings_inputs(cx);
        let profile = firefox::resolve_profile(stored_settings(cx).firefox_profile.as_deref());
        let Some(profile) = profile else {
            self.entries.update(cx, |list, cx| {
                list.delegate_mut()
                    .set_message(Some(strings::settings_page::NO_FIREFOX_PROFILE.to_string()));
                cx.notify();
            });
            return;
        };
        self.entries.update(cx, |list, cx| {
            list.delegate_mut().set_loading(true);
            cx.notify();
        });

        let reading = cx.background_executor().spawn(async move {
            if kind == ListKind::History {
                firefox::history(&profile).map(visit_entries)
            } else {
                firefox::bookmarks(&profile).map(bookmark_entries)
            }
        });
        self.pending = Some(cx.spawn(async move |this, cx| {
            let outcome = reading.await;
            this.update(cx, |launcher, cx| launcher.finish_list(outcome, cx))
                .ok();
        }));
    }

    /// 取个人项目或者星标。缓存优先：有缓存就先摆上来，刷新按钮才碰网络。
    fn load_repos(&mut self, kind: ListKind, refresh: bool, cx: &mut Context<Self>) {
        self.commit_settings_inputs(cx);
        let settings = stored_settings(cx);
        let user = settings.github_user.trim().to_string();
        if user.is_empty() {
            self.entries.update(cx, |list, cx| {
                list.delegate_mut()
                    .set_message(Some(strings::lists::NO_USER.to_string()));
                cx.notify();
            });
            return;
        }
        let cache_key = format!("github-{}-{user}", kind.cache_name());
        if !refresh {
            let cached = cache::read(&cache_key).and_then(|text| github::from_json(&text));
            if let Some(repos) = cached {
                self.install_entries(repo_entries(repos), cx);
                return;
            }
        }

        self.entries.update(cx, |list, cx| {
            list.delegate_mut().set_loading(true);
            cx.notify();
        });
        let http = cx.http_client();
        let token = settings.github_token.trim().to_string();
        self.pending = Some(cx.spawn(async move |this, cx| {
            let outcome = if kind == ListKind::Projects {
                github::owned_repos(http, &user, &token).await
            } else {
                github::starred(http, &user, &token).await
            };
            this.update(cx, |launcher, cx| {
                launcher.finish_repos(kind, &cache_key, outcome, cx)
            })
            .ok();
        }));
    }

    /// 按关键词搜仓库。
    fn run_search(&mut self, keyword: String, cx: &mut Context<Self>) {
        let keyword = keyword.trim().to_string();
        if keyword.is_empty() {
            return;
        }
        let token = stored_settings(cx).github_token.trim().to_string();
        let http = cx.http_client();
        // 记下来：同一个词再按回车就是「打开选中的仓库」，不再重复搜。
        self.repo_query = keyword.clone();
        self.entries.update(cx, |list, cx| {
            list.delegate_mut().mark_searched();
            list.delegate_mut().set_loading(true);
            cx.notify();
        });
        self.pending = Some(cx.spawn(async move |this, cx| {
            let outcome = github::search(http, &keyword, &token).await;
            this.update(cx, |launcher, cx| {
                launcher.finish_list(outcome.map(repo_entries), cx)
            })
            .ok();
        }));
    }

    /// 网络回来的仓库：写进缓存，再摆到列表上。
    fn finish_repos(
        &mut self,
        kind: ListKind,
        cache_key: &str,
        outcome: Result<Vec<github::Repo>, String>,
        cx: &mut Context<Self>,
    ) {
        self.pending = None;
        // 用户可能已经翻到别的页面去了，那就别往回塞。
        if !matches!(self.page, Page::List(current) if current == kind) {
            return;
        }
        match outcome {
            Ok(repos) => {
                cache::write(cache_key, &github::to_json(&repos));
                if repos.is_empty() {
                    self.entries.update(cx, |list, cx| {
                        list.delegate_mut()
                            .set_message(Some(strings::lists::NO_REPOS.to_string()));
                        cx.notify();
                    });
                } else {
                    self.install_entries(repo_entries(repos), cx);
                }
            }
            Err(error) => {
                // 刷新失败就把缓存留着，只在上面写一句为什么。
                self.entries.update(cx, |list, cx| {
                    list.delegate_mut().set_message(Some(error));
                    cx.notify();
                });
            }
        }
    }

    /// 本地读取回来的数据（书签、历史、搜索结果）。
    fn finish_list(&mut self, outcome: Result<Vec<Entry>, String>, cx: &mut Context<Self>) {
        self.pending = None;
        match outcome {
            Ok(entries) => self.install_entries(entries, cx),
            Err(error) => self.entries.update(cx, |list, cx| {
                list.delegate_mut().set_message(Some(error));
                cx.notify();
            }),
        }
    }

    /// 把一批行装进列表页，高亮回到第一行。
    fn install_entries(&mut self, entries: Vec<Entry>, cx: &mut Context<Self>) {
        self.entries.update(cx, |list, cx| {
            let empty = entries.is_empty();
            list.delegate_mut().set_items(entries);
            if empty {
                list.delegate_mut().set_message(None);
            }
            cx.notify();
        });
    }

    /// 把设置页三个文本框里的值存下来；没变就不写库。
    pub(crate) fn commit_settings_inputs(&mut self, cx: &mut Context<Self>) {
        let profile = self.firefox_input.read(cx).value().trim().to_string();
        let user = self.github_user_input.read(cx).value().trim().to_string();
        let token = self.github_token_input.read(cx).value().trim().to_string();

        let mut settings = stored_settings(cx);
        let profile = (!profile.is_empty()).then_some(profile);
        if settings.firefox_profile == profile
            && settings.github_user == user
            && settings.github_token == token
        {
            return;
        }
        settings.firefox_profile = profile;
        settings.github_user = user;
        settings.github_token = token;
        store_settings(settings, cx);
        cx.notify();
    }

    /// 火狐目录退回自动探测：清空输入框并落盘。
    pub(crate) fn clear_firefox_profile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.firefox_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.commit_settings_inputs(cx);
    }

    /// 末尾追一个空行。新行要有窗口才建得起来，所以在拿得到窗口的地方调。
    pub(crate) fn append_calculator_row(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.next_row_id;
        self.next_row_id += 1;
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, CALCULATOR_MAX_ROWS)
                .placeholder(strings::calculator::PLACEHOLDER)
        });
        let events = cx.subscribe(&input, move |launcher, input, event, cx| match event {
            InputEvent::Change => {
                let value = input.read(cx).value().to_string();
                launcher.calculator_row_changed(id, &value, cx);
            }
            // 点进去的那一行就是「本行」：清空本行清的是它。
            InputEvent::Focus => launcher.current_row = Some(id),
            _ => {}
        });
        self.calculator.push(CalculatorRow {
            id,
            input,
            _events: events,
        });
        cx.notify();
    }

    /// 一行公式变了：末行一有内容，下面就自动补一个空行。
    fn calculator_row_changed(&mut self, id: usize, value: &str, cx: &mut Context<Self>) {
        let is_last = self.calculator.last().is_some_and(|row| row.id == id);
        // 建新行要窗口，而事件回调里只有视图；推到下一拍再做（和搜索词同步一个路子）。
        if is_last && !value.trim().is_empty() {
            with_launcher(cx, |launcher, window, cx| {
                launcher.append_calculator_row(window, cx)
            });
        }
        // 结果是从公式现算的，重画一次就有了。
        cx.notify();
    }

    /// 清空本行：清用户最后一次点进去的那行，清完焦点还留在那儿。
    pub(crate) fn clear_calculator_row(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let target = self
            .current_row
            .or_else(|| self.calculator.last().map(|row| row.id));
        let Some(row) = target.and_then(|id| self.calculator.iter().find(|row| row.id == id))
        else {
            return;
        };
        row.input
            .update(cx, |input, cx| input.set_value("", window, cx));
        row.input.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    /// 清空所有：回到「只有一行空行」的初始状态。
    pub(crate) fn clear_calculator(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.calculator.clear();
        self.current_row = None;
        self.copied = None;
        self.append_calculator_row(window, cx);
        self.focus_calculator_last(window, cx);
    }

    /// 点一下结果：把这行算出来的数复制走，并在行上亮一小会儿「已复制」。
    pub(crate) fn copy_calculator_result(&mut self, id: usize, cx: &mut Context<Self>) {
        let Some(row) = self.calculator.iter().find(|row| row.id == id) else {
            return;
        };
        let precision = stored_settings(cx).calculator_precision;
        let Some(text) = calculator::result_text(&row.input.read(cx).value(), precision) else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.copied = Some((id, Instant::now()));
        cx.notify();
        // 「已复制」亮一会儿就收回去，不收就永远赖在行上。
        cx.spawn(async move |launcher, cx| {
            cx.background_executor().timer(COPY_FEEDBACK).await;
            launcher
                .update(cx, |launcher, cx| {
                    if launcher.copied.is_some_and(|(row, _)| row == id) {
                        launcher.copied = None;
                        cx.notify();
                    }
                })
                .ok();
        })
        .detach();
    }

    /// 这一行是不是刚被复制过（还在亮「已复制」的窗口里）。
    pub(crate) fn calculator_copied(&self, id: usize) -> bool {
        self.copied
            .is_some_and(|(row, at)| row == id && at.elapsed() < COPY_FEEDBACK)
    }

    /// 这一行公式算出来的数；算不了就是 `None`。小数位数按设置里的计算精度。
    pub(crate) fn calculator_result(&self, id: usize, cx: &App) -> Option<String> {
        let row = self.calculator.iter().find(|row| row.id == id)?;
        let precision = stored_settings(cx).calculator_precision;
        calculator::result_text(&row.input.read(cx).value(), precision)
    }

    /// 换配色，所有窗口一起换，并记住这个选择。
    pub(crate) fn choose_theme(
        &mut self,
        choice: ColorScheme,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        let mut settings = stored_settings(cx);
        if settings.theme == choice {
            return;
        }
        settings.theme = choice;
        apply_theme(choice, window, cx);
        store_settings(settings, cx);
        // 主题底色是 Root 那层染色的来源，换主题要重刷一次。
        crate::app::apply_root_background(cx);
        cx.notify();
    }

    /// 开关半透明面板背景，并记住选择。
    pub(crate) fn set_translucent(&mut self, enabled: bool, cx: &mut Context<Self>) {
        let mut settings = stored_settings(cx);
        if settings.translucent == enabled {
            return;
        }
        settings.translucent = enabled;
        store_settings(settings, cx);
        // 窗口的背景外观与 Root 底色都得跟着换：半透明走亚克力，否则退回实色。
        crate::app::apply_panel_appearance(cx);
        crate::app::apply_root_background(cx);
        cx.notify();
    }

    /// 拖动透明度：只改内存里的那份，面板立刻变；落盘由面板收起时统一做。
    pub(crate) fn preview_panel_opacity(&mut self, opacity: u32, cx: &mut Context<Self>) {
        let opacity = opacity.clamp(Settings::PANEL_OPACITY_MIN, Settings::PANEL_OPACITY_MAX);
        let mut settings = stored_settings(cx);
        if settings.panel_opacity == opacity {
            return;
        }
        settings.panel_opacity = opacity;
        // 只写全局值，不落库：拖一下就是几十次，磁盘写入留给收起面板那一次。
        cx.set_global(crate::app::SettingsStore(settings));
        cx.notify();
    }

    /// 改搜索框高度，页头跟着长。
    pub(crate) fn set_search_height(&mut self, height: u32, cx: &mut Context<Self>) {
        let height = height.clamp(Settings::SEARCH_HEIGHT_MIN, Settings::SEARCH_HEIGHT_MAX);
        let mut settings = stored_settings(cx);
        if settings.search_height == height {
            return;
        }
        settings.search_height = height;
        store_settings(settings, cx);
        cx.notify();
    }

    /// 改计算器结果的精度（最多保留几位小数）。
    pub(crate) fn set_calculator_precision(&mut self, precision: u32, cx: &mut Context<Self>) {
        let precision = precision.clamp(
            Settings::CALCULATOR_PRECISION_MIN,
            Settings::CALCULATOR_PRECISION_MAX,
        );
        let mut settings = stored_settings(cx);
        if settings.calculator_precision == precision {
            return;
        }
        settings.calculator_precision = precision;
        store_settings(settings, cx);
        cx.notify();
    }

    /// 打开/关闭一项指令匹配，并立刻按新开关重算当前命中的匹配。
    pub(crate) fn set_matcher(
        &mut self,
        kind: MatchKind,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut settings = stored_settings(cx);
        let slot = match kind {
            MatchKind::Calculator => &mut settings.matcher_calculator,
            MatchKind::Amount => &mut settings.matcher_amount,
            MatchKind::Url => &mut settings.matcher_url,
            MatchKind::Path => &mut settings.matcher_path,
        };
        if *slot == enabled {
            return;
        }
        *slot = enabled;
        store_settings(settings, cx);
        self.refresh_matchers(window, cx);
    }

    /// 把最新的匹配开关推给列表，并把高亮挪回第一项（命中数可能变了）。
    pub(crate) fn refresh_matchers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let config = MatchConfig::from_settings(&stored_settings(cx));
        self.list.update(cx, |list, cx| {
            list.delegate_mut().set_config(config);
            list.set_selected_index(Some(IndexPath::new(0)), window, cx);
            cx.notify();
        });
        cx.notify();
    }

    /// 选择隐藏后多久之内还记得原来的子页面。
    pub(crate) fn set_resume_seconds(&mut self, seconds: u32, cx: &mut Context<Self>) {
        let mut settings = stored_settings(cx);
        if settings.resume_seconds == seconds {
            return;
        }
        settings.resume_seconds = seconds;
        store_settings(settings, cx);
        cx.notify();
    }

    /// 加开机启动项，或者删掉它。
    pub(crate) fn set_launch_at_login(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if !autostart::set_enabled(enabled) {
            // 开关是受控值，写失败它就停在原处：如实报告，别装作改成了。
            eprintln!("gpui-tools: 开机启动项没能更新");
            cx.notify();
            return;
        }
        let mut settings = stored_settings(cx);
        settings.launch_at_login = enabled;
        store_settings(settings, cx);
        cx.notify();
    }

    /// 重新读一遍设置里关于每条命令的内容。
    pub(crate) fn refresh_catalogue(&mut self, cx: &mut Context<Self>) {
        let states = command_states(&stored_settings(cx));
        self.list.update(cx, |list, cx| {
            list.delegate_mut().states = states;
            cx.notify();
        });
    }

    /// 命令页换一个子模块。
    pub(crate) fn select_section(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.section == index {
            return;
        }
        self.section = index;
        self.recording = None;
        self.conflict = None;
        cx.notify();
    }

    /// 把一条命令从主页拿掉，或者放回去。
    pub(crate) fn set_command_enabled(
        &mut self,
        command: &'static catalogue::Command,
        enabled: bool,
        cx: &mut Context<Self>,
    ) {
        let mut settings = stored_settings(cx);
        let (section, id) = (command.section.key(), command.id);
        if settings.command_enabled(section, id) == enabled {
            return;
        }
        settings.set_command_enabled(section, id, enabled);
        store_settings(settings, cx);
        self.refresh_catalogue(cx);
        cx.notify();
    }

    /// 开始听下一组按键，作为这条命令的快捷键；再点一次取消。
    pub(crate) fn toggle_command_recording(
        &mut self,
        command: &'static catalogue::Command,
        cx: &mut Context<Self>,
    ) {
        self.toggle_recording(RecordTarget::Command(command), cx);
    }

    /// 开始听下一组按键，作为唤出面板的热键；再点一次取消。
    pub(crate) fn toggle_summon_recording(&mut self, cx: &mut Context<Self>) {
        self.toggle_recording(RecordTarget::Summon, cx);
    }

    fn toggle_recording(&mut self, target: RecordTarget, cx: &mut Context<Self>) {
        self.recording = if self.recording.is_some_and(|current| current.same(target)) {
            None
        } else {
            Some(target)
        };
        self.conflict = None;
        cx.notify();
    }

    /// 把接下来这一下按键当成正在录制的那一项的组合键。
    ///
    /// 只收带修饰键的组合：没有修饰键的会被面板自己吃掉，做全局快捷键没有意义。
    ///
    /// **绑之前先查冲突**：这组键要是已经归别的命令（或者唤出热键），就不绑，
    /// 只在按钮上写一句「已被 X 占用」，录制继续等下一组键。
    pub(crate) fn capture_shortcut(
        &mut self,
        keystroke: &gpui_kit::Keystroke,
        cx: &mut Context<Self>,
    ) {
        let Some(target) = self.recording else {
            return;
        };
        match keystroke.key.as_str() {
            "escape" => {
                self.recording = None;
                self.conflict = None;
            }
            "backspace" | "delete" => {
                self.clear_shortcut(target, cx);
                self.recording = None;
                self.conflict = None;
            }
            _ => {
                let Some(chord) = chord_from_keystroke(keystroke) else {
                    // 这个键用不了，继续等下一组。
                    return;
                };
                if let Some(owner) = crate::shortcuts::owner_of_chord(&chord, cx) {
                    self.report_conflict(Some(owner), cx);
                    return;
                }
                self.store_shortcut(target, Some(chord), cx);
                self.recording = None;
                self.conflict = None;
            }
        }
        cx.notify();
    }

    /// 录制期间按到一组已经被占用的键。
    ///
    /// 注册过的全局热键会被系统从焦点窗口那里截走，按键根本到不了 `capture_shortcut`，
    /// 冲突是从热键事件那条路上报回来的。按的正好是录制目标自己那组键时不算冲突：
    /// 当作再确认一次，直接收工。
    pub(crate) fn report_conflict(&mut self, owner: Option<ShortcutOwner>, cx: &mut Context<Self>) {
        let Some(target) = self.recording else {
            return;
        };
        let Some(owner) = owner else {
            return;
        };
        if target.is_owner(owner) {
            self.recording = None;
            self.conflict = None;
        } else {
            self.conflict = Some(strings::commands_page::conflict(owner.label()));
        }
        cx.notify();
    }

    /// 清掉一项的快捷键（右键按钮，或者录制中按 Backspace 都走这儿）。
    ///
    /// 清的就是正在录的那一项时，录制也一起结束：按钮上不该再挂着「按下组合键…」。
    pub(crate) fn clear_shortcut(&mut self, target: RecordTarget, cx: &mut Context<Self>) {
        self.store_shortcut(target, None, cx);
        if self.recording.is_some_and(|current| current.same(target)) {
            self.recording = None;
            self.conflict = None;
            cx.notify();
        }
    }

    /// 存一组组合键，或者清空，并让全局热键跟着变。
    fn store_shortcut(
        &mut self,
        target: RecordTarget,
        chord: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let mut settings = stored_settings(cx);
        match target {
            RecordTarget::Summon => {
                if settings.summon_shortcut == chord {
                    return;
                }
                settings.set_summon_shortcut(chord);
                store_settings(settings, cx);
                if !apply_summon_shortcut(cx) {
                    eprintln!("gpui-tools: 唤出热键这次没能绑定，要等下次启动再试");
                }
            }
            RecordTarget::Command(command) => {
                let (section, id) = (command.section.key(), command.id);
                if settings.command_shortcut(section, id).map(str::to_string) == chord {
                    return;
                }
                settings.set_command_shortcut(section, id, chord);
                store_settings(settings, cx);
                apply_command_shortcuts(cx);
                self.refresh_catalogue(cx);
            }
        }
        self.conflict = None;
        cx.notify();
    }
}

/// 书签行：标题是书签名字，副行是网址，右边补一句页面标题（和书签名字不一样时才写）。
fn bookmark_entries(rows: Vec<firefox::Bookmark>) -> Vec<Entry> {
    rows.into_iter()
        .map(|bookmark| {
            let meta = if bookmark.page_title.is_empty() || bookmark.page_title == bookmark.title {
                String::new()
            } else {
                bookmark.page_title
            };
            Entry::new(
                bookmark.title,
                bookmark.url.clone(),
                meta,
                bookmark.url,
                gpui_kit::assets::IconName::Bookmark,
            )
            .with_icon(bookmark.icon.as_ref().map(favicon_image))
        })
        .collect()
}

/// 历史行：标题是页面标题，右边是「访问次数 · 最后访问」。
fn visit_entries(rows: Vec<firefox::Visit>) -> Vec<Entry> {
    rows.into_iter()
        .map(|visit| {
            let mut meta = format!("{} 次", visit.visits);
            if !visit.visited.is_empty() {
                meta = format!("{meta} \u{00b7} {}", visit.visited);
            }
            Entry::new(
                visit.title,
                visit.url.clone(),
                meta,
                visit.url,
                gpui_kit::assets::IconName::Clock,
            )
            .with_icon(visit.icon.as_ref().map(favicon_image))
        })
        .collect()
}

/// 仓库行：标题是 `owner/name`，副行是简介（没有就写网址），右边是「★ 数 · 主语言」。
fn repo_entries(rows: Vec<github::Repo>) -> Vec<Entry> {
    rows.into_iter()
        .map(|repo| {
            let mut meta = format!("\u{2605} {}", repo.stars);
            if !repo.language.is_empty() {
                meta = format!("{meta} \u{00b7} {}", repo.language);
            }
            let detail = if repo.description.is_empty() {
                repo.url.clone()
            } else {
                repo.description
            };
            Entry::new(
                repo.full_name,
                detail,
                meta,
                repo.url,
                gpui_kit::assets::IconName::Github,
            )
        })
        .collect()
}
