//! 面板本体：页头、页脚、命令列表，以及面板内的导航。

use std::time::{Duration, Instant};

use gpui_kit::component::IndexPath;
use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::component::list::ListState;
use gpui_kit::component::slider::{SliderEvent, SliderState};
use gpui_kit::{
    App, ClipboardItem, Context, Entity, Focusable, ScrollHandle, Subscription, Task, Window,
    point, prelude::*, px,
};
use tools::admin;
use tools::autostart;
use tools::cache;
use tools::calculator;
use tools::firefox;
use tools::github;
use tools::matcher::{Match, MatchAction, MatchConfig, MatchKind};
use tools::settings::{HomeLayout, Settings, ToolKind, UserTool};
use tools::shell;

use crate::app::{
    LauncherPanel, apply_theme, hide_launcher, run_command, store_settings, stored_settings,
    with_launcher,
};
use crate::catalogue;
use crate::shortcuts::{
    ShortcutOwner, apply_shortcuts, apply_summon_shortcut, chord_from_keystroke,
};
use crate::strings;
use crate::ui::commands_list::{ToolDelegate, command_states};
use crate::ui::lists::{Entry, EntryDelegate, favicon_image};
use crate::ui::pages::user_tools::UserToolsDelegate;
use tools::settings::Theme as ColorScheme;

// 绘制拆到 `view.rs`：页头、页脚与整页骨架在那里，这里只管状态和行为。
mod view;

/// 页头最矮也要画这么高，不管搜索框设成多少。
const HEADER_MIN_HEIGHT: f32 = 52.;
/// 页头留给搜索框上下的余量，两个方向合起来算。
const HEADER_SLACK: f32 = 26.;
/// 设置页左侧子模块列的宽度。
/// 用户条目（文件 / 文件夹 / 快捷终端）没有系统图标时的兜底图标。
pub(crate) fn user_kind_icon(kind: ToolKind) -> gpui_kit::assets::IconName {
    match kind {
        ToolKind::File => gpui_kit::assets::IconName::FileText,
        ToolKind::Folder => gpui_kit::assets::IconName::Folder,
        ToolKind::Terminal => gpui_kit::assets::IconName::SquareTerminal,
    }
}

/// 三列布局里第二列（子模块 / 子工具）的宽度。
pub(crate) const MODULE_WIDTH: f32 = 168.;

pub(crate) const SIDEBAR_WIDTH: f32 = 148.;
/// 命令页里左侧子模块列的宽度。要放得下「GitHub 3/3」这种最长的组合。
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
/// 系统文件框关掉之后，再给前台多久才允许按「失去前台」收起面板。
const FRONT_GRACE: Duration = Duration::from_millis(1200);

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
    /// 自定义条目：和书签、历史同一级的一页。
    UserTools,
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
            Self::Projects => gpui_kit::assets::IconName::House,
            Self::Stars => gpui_kit::assets::IconName::Star,
            Self::Search => gpui_kit::assets::IconName::Github,
        }
    }

    /// 这一页的数据是不是 GitHub 仓库（缓存优先，页头有刷新）。
    pub(crate) fn is_repos(self) -> bool {
        matches!(self, Self::Projects | Self::Stars)
    }

    /// 页头右上角要不要给按钮。
    ///
    /// 书签和历史每次进页面都重读库，天生是最新的，不给刷新；仓库两页读缓存、
    /// 查询仓库要发请求，都得有个手动入口。
    pub(crate) fn needs_action(self) -> bool {
        matches!(self, Self::Projects | Self::Stars | Self::Search)
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
    /// 某一条静态命令。
    Command(&'static catalogue::Command),
    /// 用户自己加的一条工具，按 id 认。
    Tool(i64),
}

impl RecordTarget {
    /// 两个录制目标是不是同一项。
    pub(crate) fn same(self, other: Self) -> bool {
        match (self, other) {
            (Self::Summon, Self::Summon) => true,
            (Self::Command(a), Self::Command(b)) => std::ptr::eq(a, b),
            (Self::Tool(a), Self::Tool(b)) => a == b,
            _ => false,
        }
    }

    /// 热键现在归的那个对象，是不是就是录制目标本身。
    fn is_owner(self, owner: ShortcutOwner) -> bool {
        match (self, owner) {
            (Self::Summon, ShortcutOwner::Summon) => true,
            (Self::Command(a), ShortcutOwner::Command(b)) => std::ptr::eq(a, b),
            (Self::Tool(a), ShortcutOwner::Tool(b)) => a == b,
            _ => false,
        }
    }
}

/// 仓库搜索页的现场：关键词、结果、高亮在第几行。
struct SearchState {
    keyword: String,
    rows: Vec<Entry>,
    selected: usize,
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

/// 设置页里正在编辑的用户工具（新增，或者改名字/改路径）。
///
/// 几个输入框自己就是状态：编辑区一关，里面没保存的内容跟着丢，不用另外回滚。
pub(crate) struct ToolEditor {
    pub(crate) kind: ToolKind,
    /// 改的是哪一条；`None` 表示还没保存过的新条目。
    pub(crate) id: Option<i64>,
    pub(crate) name: Entity<InputState>,
    pub(crate) target: Entity<InputState>,
    /// 只有快捷终端用得上：终端的工作目录，留空 = 用户文件夹。
    pub(crate) start_dir: Entity<InputState>,
    /// 输入框的回车订阅，跟着编辑区一起活。
    _events: Vec<Subscription>,
}

impl ToolEditor {
    /// 名字输入框里现在的内容，去掉两头空白。
    pub(crate) fn name_text(&self, cx: &App) -> String {
        self.name.read(cx).value().trim().to_string()
    }

    /// 路径/命令输入框里现在的内容，去掉两头空白。
    pub(crate) fn target_text(&self, cx: &App) -> String {
        self.target.read(cx).value().trim().to_string()
    }

    /// 起始目录输入框里现在的内容，去掉两头空白。
    pub(crate) fn start_dir_text(&self, cx: &App) -> String {
        self.start_dir.read(cx).value().trim().to_string()
    }
}

/// 这次挑东西是为谁挑的。
#[derive(Clone, Copy)]
pub(crate) enum PickFor {
    /// 新增一条文件。
    NewFile,
    /// 新增一条文件夹。
    NewFolder,
    /// 把某一条的路径换掉（按它自己的种类决定挑文件还是挑目录）。
    Replace(i64),
    /// 改编辑区里「文件 / 文件夹 / 命令」那一行，还没保存。
    EditorTarget,
    /// 改编辑区里「起始目录」那一行（只挑目录）。
    EditorStartDir,
}

/// 「文件不在了」的那条提示。
pub(crate) struct MissingPrompt {
    pub(crate) id: i64,
    pub(crate) name: String,
    pub(crate) target: String,
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
    /// 列表页页头的搜索框。五页共用，换页时清空。
    pub(crate) list_search: Entity<InputState>,
    /// 已经发给 GitHub 的那个关键词（远程搜索用）。
    list_query: String,
    /// 仓库搜索页上次的结果：面板收起时留一份，`resume_seconds` 内再进来直接摆回去。
    search_state: Option<SearchState>,
    /// 后台任务（读库、抓网络）。存着它任务才不会被丢掉。
    pending: Option<Task<()>>,
    /// 设置里那三个文本框（火狐目录、GitHub 用户名、令牌）。
    pub(crate) firefox_input: Entity<InputState>,
    pub(crate) github_user_input: Entity<InputState>,
    pub(crate) github_token_input: Entity<InputState>,
    /// 正在编辑的用户工具（设置 → 工具 → 文件快捷方式 / 快捷终端）。`None` 表示没有编辑区。
    pub(crate) tool_editor: Option<ToolEditor>,
    /// 编辑区里那句校验不过的提示，比如「命令不能为空」。
    pub(crate) tool_notice: Option<&'static str>,
    /// 「文件不在了」的提示框。
    pub(crate) prompt: Option<MissingPrompt>,
    /// 管理员模式正在装/正在卸的那一会儿，系统页上显示一句话。
    pub(crate) admin_notice: Option<&'static str>,
    /// 装/卸计划任务那条任务，留着它别被丢掉。
    admin_task: Option<Task<()>>,
    /// 正在装/卸计划任务。**守卫用这个标志，不用 `admin_task.is_some()`**：
    /// Task 跑完之后句柄还留在字段里，拿它当锁会让开关永久卡死（关掉之后再也点不开，
    /// 只有重启才恢复——踩过）。
    admin_busy: bool,
    /// 系统文件框那一次等待。`Some` 期间面板失去前台也**不收起来**：用户正在那个
    /// 框里挑文件，面板一消失，挑完回来页面就变了。
    ///
    /// 单独一个字段、不塞进 `pending`：`pending` 会跟着换页被丢掉，而这个任务被
    /// 丢掉就等于「对话框永远开着」，面板以后再也不自动收起了。
    dialog: Option<Task<()>>,
    /// 对话框刚关上的这一小会儿，别因为「还没拿到前台」就收起面板。
    ///
    /// 框关掉到前台转回来之间有一两百毫秒的空档，正好够失焦检查数满三次；不拦一下，
    /// 用户挑完文件/取消之后会看到面板自己消失。
    front_grace: Option<Instant>,
    /// 跟着视图活着的订阅，丢了输入框就不再上报用户敲了什么。
    _search_events: Subscription,
    _opacity_events: Subscription,
    _search_observation: Subscription,
    /// 列表页搜索框的订阅。
    _list_search_events: Subscription,
    /// 方格布局的滚动位置（列表布局不用它）。
    pub(crate) grid_scroll: ScrollHandle,
    /// 三个工具列表里高亮到第几条（相对**过滤后**的列表）。
    pub(crate) tools_selected: usize,
    /// 命令页第二列最底下那条「自定义条目」展开成了哪一类。
    ///
    /// `None` = 看的是某个子模块的命令；`Some(kind)` = 看这一类用户条目的列表。
    pub(crate) user_kind: ToolKind,
    /// 自定义条目那张列表（和书签、历史同一套 `List`，滚动交给组件）。
    pub(crate) user_list: Entity<ListState<UserToolsDelegate>>,
    /// 设置里「文件 / 文件夹 / 快捷终端」列表的搜索框与关键词。
    ///
    /// 三页共用一份：换页时清空，免得带着上一页的关键词进来一片空白。
    pub(crate) tools_search: Entity<InputState>,
    pub(crate) tools_query: String,
    /// 那三个列表自己的滚动位置（头部固定，只有列表滚）。
    pub(crate) tools_scroll: ScrollHandle,
    /// 设置页右半边的滚动位置。
    ///
    /// 设置里那些列表（尤其快捷终端）能有几十条，比窗口高，必须能滚；换标签时要回到
    /// 顶部，否则从「滚到底的终端列表」切到「系统」会停在一半。
    pub(crate) settings_scroll: ScrollHandle,
    /// 鼠标正压在哪个方格上（扁平索引）。只用来出那句描述。
    pub(crate) hovered_cell: Option<usize>,
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
        // 点击由行自己分单击/双击，所以关掉 List 自带的点击执行。
        let entries = cx.new(|cx| {
            ListState::new(EntryDelegate::new(ListKind::Bookmarks), window, cx).selectable(false)
        });
        let view = cx.weak_entity();
        entries.update(cx, |state, _cx| {
            state.delegate_mut().view = Some(view);
        });

        // 设置里那三个工具列表的搜索框。
        let tools_search = cx.new(|cx| InputState::new(window, cx));
        let tools_search_events = cx.subscribe(&tools_search, |launcher, search, event, cx| {
            if matches!(event, InputEvent::Change) {
                launcher.tools_query = search.read(cx).value().to_string();
                launcher.sync_user_list(cx);
                cx.notify();
            }
        });

        // 列表页的搜索框：和主页那个分开，两页的输入互不干扰。
        let list_search = cx.new(|cx| InputState::new(window, cx));
        let list_search_events = cx.subscribe(&list_search, |launcher, search, event, cx| {
            if matches!(event, InputEvent::Change) {
                let text = search.read(cx).value().to_string();
                launcher.list_search_changed(text, cx);
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
            list_search,
            list_query: String::new(),
            search_state: None,
            pending: None,
            firefox_input,
            github_user_input,
            github_token_input,
            tool_editor: None,
            tool_notice: None,
            prompt: None,
            admin_notice: None,
            admin_task: None,
            admin_busy: false,
            dialog: None,
            front_grace: None,
            _search_events: search_events,
            _opacity_events: opacity_events,
            _search_observation: search_observation,
            _list_search_events: list_search_events,
            grid_scroll: ScrollHandle::new(),
            tools_selected: 0,
            user_kind: ToolKind::File,
            user_list: cx
                .new(|cx| ListState::new(UserToolsDelegate::new(ToolKind::File), window, cx)),
            tools_search,
            tools_query: String::new(),
            tools_scroll: ScrollHandle::new(),
            settings_scroll: ScrollHandle::new(),
            hovered_cell: None,
            _settings_events: settings_events
                .into_iter()
                .chain([tools_search_events])
                .collect(),
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

    /// 正在录制的命令；录的是唤出热键时、或者在给用户工具录制时为 `None`。
    pub(crate) fn recording_command(&self) -> Option<&'static catalogue::Command> {
        match self.recording {
            Some(RecordTarget::Command(command)) => Some(command),
            _ => None,
        }
    }

    /// 正在录制的用户工具 id。
    pub(crate) fn recording_tool(&self) -> Option<i64> {
        match self.recording {
            Some(RecordTarget::Tool(id)) => Some(id),
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
            Page::UserTools => self.move_user_selection(delta, cx),
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

    /// 切换主页排版（外观设置里那个分段控件）。
    ///
    /// 两种排版共用同一份数据和高亮下标，切换只是换画法，不用重建列表。
    pub(crate) fn set_home_layout(&mut self, layout: HomeLayout, cx: &mut Context<Self>) {
        let mut settings = stored_settings(cx);
        if settings.home_layout == layout {
            return;
        }
        settings.home_layout = layout;
        store_settings(settings, cx);
        // 换回列表时别留着上一版的悬停格子。
        self.hovered_cell = None;
        cx.notify();
    }

    /// 现在是不是该按方格布局处理按键（主页 + 设置里选了方格）。
    pub(crate) fn grid_active(&self, cx: &App) -> bool {
        self.page == Page::Commands && stored_settings(cx).home_layout == HomeLayout::Grid
    }

    /// 方格布局下挪高亮：`dx` 左右一列，`dy` 上下一个格（网格里一行是 `columns` 个）。
    ///
    /// 命中区仍然在最上面：在命中区里 `↑↓` 就是上一条下一条，从最后一条再往下就落到
    /// 第一个方格；从第一行方格往上回到命中区最后一条。
    pub(crate) fn move_grid(
        &mut self,
        dx: isize,
        dy: isize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let metrics = self.grid_metrics(window);
        let (count, matches_len, selected) = {
            let delegate = self.list.read(cx).delegate();
            (
                delegate.item_count(),
                delegate.match_count(),
                delegate.selected,
            )
        };
        if count == 0 {
            return;
        }
        let next = if selected < matches_len {
            let target = selected as isize + dy;
            if dy > 0 && target >= matches_len as isize {
                // 越过最后一条命中，进网格第一个。
                matches_len.min(count.saturating_sub(1))
            } else if dy < 0 {
                target.clamp(0, matches_len as isize - 1) as usize
            } else {
                selected
            }
        } else {
            let last = count - matches_len - 1;
            let grid = selected - matches_len;
            let row = grid / metrics.columns;
            let column = grid % metrics.columns;
            if dx != 0 {
                let column = (column as isize + dx).clamp(0, metrics.columns as isize - 1) as usize;
                (row * metrics.columns + column).min(last) + matches_len
            } else if dy < 0 {
                if row == 0 {
                    if matches_len == 0 {
                        selected
                    } else {
                        matches_len - 1
                    }
                } else {
                    ((row - 1) * metrics.columns + column).min(last) + matches_len
                }
            } else if dy > 0 {
                let last_row = last / metrics.columns;
                ((row + 1).min(last_row) * metrics.columns + column).min(last) + matches_len
            } else {
                selected
            }
        };
        if next == selected {
            return;
        }
        self.list
            .update(cx, |list, _cx| list.delegate_mut().select(next));
        self.grid_reveal(window, cx);
        cx.notify();
    }

    /// 方格布局里点了一下：选中并直接执行。
    ///
    /// **单击就执行**，和列表行一个规矩。别跟指令匹配那几行混了——那几行单击是复制、
    /// 双击才打开，方格里的条目没有「先看看」这一步。
    pub(crate) fn grid_click(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.list
            .update(cx, |list, _cx| list.delegate_mut().select(index));
        self.grid_reveal(window, cx);
        self.activate_selected(window, cx);
    }

    /// 鼠标压上了某个方格 / 离开了：只有压着的那一个才出描述。
    pub(crate) fn set_hovered_cell(&mut self, index: usize, hovered: bool, cx: &mut Context<Self>) {
        let next = hovered.then_some(index);
        if self.hovered_cell != next {
            self.hovered_cell = next;
            cx.notify();
        }
    }

    /// 把当前高亮的方格滚进可视范围。
    ///
    /// 方格是等距的，位置直接算得出来，不必让滚动容器替我们记（它也记不住——这里没有
    /// 逐项注册边界）。
    fn grid_reveal(&self, window: &Window, cx: &App) {
        let metrics = self.grid_metrics(window);
        let (matches_len, selected) = {
            let delegate = self.list.read(cx).delegate();
            (delegate.match_count(), delegate.selected)
        };
        // 命中区钉在最上面，不参与滚动。
        if selected < matches_len {
            return;
        }
        let viewport = f32::from(self.grid_scroll.bounds().size.height);
        if viewport <= 0.0 {
            return;
        }
        let step = metrics.row_height(crate::ui::grid::CELL_GAP);
        let row = ((selected - matches_len) / metrics.columns) as f32;
        let (top, bottom) = (row * step, row * step + metrics.cell);
        let mut offset = f32::from(self.grid_scroll.offset().y);
        if top < offset {
            offset = top;
        } else if bottom > offset + viewport {
            offset = bottom - viewport;
        }
        let limit = f32::from(self.grid_scroll.max_offset().y).max(0.0);
        self.grid_scroll
            .set_offset(point(px(0.), px(offset.clamp(0.0, limit))));
    }

    /// 当前宽度下一行能放几个方格。绘制和方向键两边都问它，免得算得不一致。
    pub(crate) fn grid_metrics(&self, window: &Window) -> crate::ui::grid::GridMetrics {
        crate::ui::grid::metrics_for(f32::from(window.viewport_size().width))
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
        self.prompt = None;
        self.tool_editor = None;
        self.tool_notice = None;
        // 新的会话从干净状态开始：上一次要是还挂着系统文件框（那不可能在面板可见时
        // 完成），这里把等它的那条任务放掉，免得自动收起一直被它挡着。
        self.dialog = None;
        self.refresh_catalogue(cx);
        // 上一次留下的列表数据先放掉：历史是 500 条、每行还挂着图标，不释放就一直占着。
        // 下面按记忆回到那一页时会重新装一次。
        self.entries.update(cx, |list, cx| {
            list.delegate_mut().set_items(Vec::new());
            cx.notify();
        });
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

    /// 别的窗口抢走前台就收起面板，启动器本来就该这样——**除非被钉住**。
    ///
    /// GPUI 不报窗口激活事件，平台也不把那个状态交给上层，所以这里改成采样前台窗口。
    /// 面板一收起循环就结束，下次唤起重新起一条；而且必须曾经到过前台才允许收，
    /// 免得窗口被系统拒绝抬到前面时「刚出现就消失」。
    ///
    /// 「钉住」一个循环读一次窗口的 `WS_EX_TOPMOST`：右键那条路在原生窗口过程里改的
    /// 就是它，所以这里读到的永远是**当前**状态，不用再往上传一遍。钉住期间不累计
    /// 丢焦次数，一旦取消，下一拍就恢复正常收起。
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
                    // 钉住了：用户就是想在别的窗口上边用这个面板，收起来等于砸场子。
                    if launcher.sync_pinned(panel, cx) {
                        misses = 0;
                        return true;
                    }
                    if panel.window.is_foreground() {
                        was_in_front = true;
                        misses = 0;
                        return true;
                    }
                    // 系统文件框开着（或者刚关上、前台还在路上）的时候不算「用户跑了」：
                    // 挑完文件回来还要接着填这条快捷方式，面板得原地等着。
                    let grace = launcher
                        .front_grace
                        .is_some_and(|until| Instant::now() < until);
                    if launcher.dialog.is_some() || grace {
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

    /// 把「钉住」状态从窗口同步到设置镜像里，返回现在是不是钉住。
    ///
    /// 右键切换发生在原生窗口过程里，应用这边的镜像要等到保存才更新；这里顺手对齐，
    /// 免得用户「钉住 → 收起面板」时数据库里存的还是旧值。状态真的变了就重绘一次。
    fn sync_pinned(&mut self, panel: crate::app::LauncherPanel, cx: &mut Context<Self>) -> bool {
        let pinned = panel.window.is_topmost();
        let known = stored_settings(cx).pinned;
        if pinned != known {
            let mut settings = stored_settings(cx);
            settings.pinned = pinned;
            store_settings(settings, cx);
            cx.notify();
        }
        pinned
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
    ///
    /// 仓库搜索页多做一件事：把关键词和结果留一份。真正的用法是「搜到 → 双击打开 →
    /// 面板被别的窗口顶掉 → 再唤出来接着挑下一个」，结果没了等于白搜一次。
    pub(crate) fn remember_page(&mut self, cx: &App) {
        self.last_page = if matches!(self.page, Page::Commands) {
            None
        } else {
            Some((Instant::now(), self.page))
        };
        if self.page == Page::List(ListKind::Search) {
            let (rows, selected) = self.entries.read(cx).delegate().snapshot();
            self.search_state = Some(SearchState {
                keyword: self.list_search.read(cx).value().trim().to_string(),
                rows,
                selected,
            });
        }
    }

    /// 收起面板前收拾现场：记下停在哪一页，并结束录制。
    ///
    /// **录制不能跟着面板一起藏起来**：全局热键分发是按「有没有在录制」决定执行还是
    /// 报冲突的，面板藏了但录制还在，所有热键都会静默失效——连唤出面板的那一个都
    /// 按不回来，而按钮早就看不见了。
    pub(crate) fn leave(&mut self, cx: &App) {
        self.recording = None;
        self.conflict = None;
        self.remember_page(cx);
    }

    /// 执行高亮的那一项：指令匹配（复制/打开）、一条命令、列表页的一行，或者设置页
    /// 顺手把改过的文本框存下来。
    pub(crate) fn activate_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // 提示框开着的时候回车不许穿透到后面的列表上：那时候用户该先回答删还是改。
        if self.prompt.is_some() {
            return;
        }
        match self.page {
            Page::Commands => {}
            // 自定义条目页：回车由 `List` 自己的 confirm 处理。
            Page::UserTools => {}
            Page::Settings(_) => {
                // 编辑区开着时回车 = 保存，和填表一个手感。
                if self.tool_editor.is_some() {
                    self.save_tool_editor(window, cx);
                } else {
                    self.commit_settings_inputs(cx);
                }
                return;
            }
            Page::Calculator => return,
            Page::List(kind) => {
                if kind == ListKind::Search {
                    // 搜索框里是新词就先搜；和上次一样时，回车是「打开选中的那个仓库」。
                    let keyword = self.list_search.read(cx).value().trim().to_string();
                    if !keyword.is_empty() && keyword != self.list_query {
                        self.run_search(keyword, cx);
                    } else {
                        self.open_entry(cx);
                    }
                } else {
                    self.open_entry(cx);
                }
                return;
            }
        }
        match self.list.read(cx).delegate().selected_item() {
            Some(crate::ui::commands_list::HomeItem::Match(matched)) => {
                self.activate_match(matched, window, cx);
            }
            Some(crate::ui::commands_list::HomeItem::Command(command)) => run_command(command, cx),
            Some(crate::ui::commands_list::HomeItem::Tool(tool)) => {
                crate::app::run_tool(tool.id, cx)
            }
            None => {}
        }
    }

    /// 打开列表页高亮的那一行。
    fn open_entry(&self, cx: &App) {
        self.entries.read(cx).delegate().open_selected();
    }

    /// 行被点了一下：单击只选中并复制链接，双击才打开。
    ///
    /// 单击之后还得把焦点还给搜索框：点列表会把焦点从输入框夺走，不还回去的话
    /// 用户接着敲字，字就哪儿也去不了。
    pub(crate) fn entry_clicked(
        &mut self,
        row: usize,
        clicks: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(self.page, Page::List(_)) {
            return;
        }
        if clicks >= 2 {
            self.entries.read(cx).delegate().open(row);
            cx.notify();
            return;
        }

        self.entries.update(cx, |list, cx| {
            list.set_selected_index(Some(IndexPath::new(row)), window, cx);
            list.delegate_mut().mark_copied(row);
            cx.notify();
        });
        let target = self
            .entries
            .read(cx)
            .delegate()
            .selected_entry()
            .map(|entry| entry.target.clone());
        if let Some(target) = target {
            cx.write_to_clipboard(ClipboardItem::new_string(target));
        }
        self.focus_list_search(window, cx);

        // 「已复制」亮一小会儿就收回去。
        cx.spawn(async move |launcher, cx| {
            cx.background_executor().timer(COPY_FEEDBACK).await;
            launcher
                .update(cx, |launcher, cx| {
                    launcher.entries.update(cx, |list, cx| {
                        list.delegate_mut().clear_copied();
                        cx.notify();
                    });
                })
                .ok();
        })
        .detach();
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

    /// Esc 先关掉提示框、编辑区，再从设置/计算器/列表页退出来，最后才收面板。
    pub(crate) fn escape(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.prompt.is_some() {
            self.dismiss_prompt(window, cx);
            return;
        }
        if self.tool_editor.is_some() {
            self.cancel_tool_editor(cx);
            return;
        }
        if matches!(
            self.page,
            Page::Settings(_) | Page::Calculator | Page::List(_) | Page::UserTools
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
        // 回到主页就是离开搜索了，现场不再留着。
        self.search_state = None;
        // 编辑区里没保存的内容跟着走；提示框也一样，它只对当前这一刻有意义。
        self.tool_editor = None;
        self.tool_notice = None;
        self.prompt = None;
        self.page = Page::Commands;
        self.focus_search(window, cx);
        cx.notify();
    }

    /// 换一个设置子模块。计算器本体已经不在设置里，这里不再需要为它移交焦点。
    /// 打开自定义条目页（和书签、历史同一级的一页）。
    pub(crate) fn open_user_tools(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.page = Page::UserTools;
        self.sync_user_list(cx);
        let handle = self.tools_search.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
        cx.notify();
    }

    /// 上下键在自定义条目页里挪高亮（和书签、历史同一套手感）。
    fn move_user_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        self.user_list.update(cx, |list, cx| {
            let count = list.delegate().rows(cx).len();
            list.delegate_mut().shift_selection(delta, count);
            cx.notify();
        });
        cx.notify();
    }

    /// 把列表的「类别 + 搜索词」对齐到当前状态（数据由 delegate 现算）。
    pub(crate) fn sync_user_list(&mut self, cx: &mut Context<Self>) {
        let kind = self.user_kind;
        let query = self.tools_query.clone();
        let recording = self.recording_tool();
        let conflict = self.conflict.clone();
        self.user_list.update(cx, |list, _cx| {
            list.delegate_mut().retarget(kind, &query);
            list.delegate_mut().set_recording(recording, conflict);
        });
    }

    /// 顶部三个筛选切类别。
    pub(crate) fn set_user_kind(&mut self, kind: ToolKind, cx: &mut Context<Self>) {
        if self.user_kind == kind {
            return;
        }
        self.user_kind = kind;
        self.sync_user_list(cx);
        cx.notify();
    }

    /// 切到一个设置标签。
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
        // 编辑区没点「保存」就走人，等于放弃这次改动。
        self.tool_editor = None;
        self.tool_notice = None;
        window.blur(cx);
        self.page = Page::Settings(tab);
        self.tools_selected = 0;
        self.reset_settings_scroll();
        cx.notify();
    }

    /// 换一个设置子模块（命令页左列）。
    pub(crate) fn select_section(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.section == index {
            return;
        }
        self.commit_settings_inputs(cx);
        self.section = index;
        self.recording = None;
        self.conflict = None;
        cx.notify();
    }

    /// 命令页第二列现在展开的是哪一类自定义条目。

    /// 把列表的「类别 + 搜索词」对齐到当前状态。
    ///
    /// 数据由 delegate 现算（见 `UserToolsDelegate::rows`），这里不用喂数据。

    /// 当前类别下、按搜索词过滤之后的条目。

    /// 左右键：在「文件 / 文件夹 / 快捷终端」之间切（只在已经展开自定义条目时有用）。

    /// 顶部分段切类型：和点左边那条「自定义条目」是同一件事，只是不必先回列表。

    /// 选中「自定义条目」下的某一类（文件 / 文件夹 / 快捷终端）。

    /// 收起「自定义条目」，回到某个子模块的命令列表。

    /// 搜索词变了：把结果重新喂给列表。
    pub(crate) fn user_tools_query_changed(&mut self, cx: &mut Context<Self>) {
        self.sync_user_list(cx);
        cx.notify();
    }

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
        self.tool_editor = None;
        self.tool_notice = None;
        window.blur(cx);
        self.tool_tab = tab;
        // 换一页就不带着上一页的关键词和高亮行。
        self.tools_selected = 0;
        self.tools_query.clear();
        self.tools_search
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.reset_settings_scroll();
        cx.notify();
    }

    /// 设置页的滚动条回到顶部。
    fn reset_settings_scroll(&self) {
        self.settings_scroll.set_offset(point(px(0.), px(0.)));
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
        // 从「收起后再唤出」回来的搜索页：把上次的关键词和结果摆回去，别再重搜一次。
        let restoring =
            matches!(self.page, Page::List(ListKind::Search)) && kind == ListKind::Search;
        let restored = restoring.then(|| self.search_state.take()).flatten();

        // 上一页还在跑的活儿先丢掉，结果不许串到这一页来。
        self.pending = None;
        self.page = Page::List(kind);
        self.list_query.clear();
        self.entries.update(cx, |list, cx| {
            list.delegate_mut().reset(kind);
            cx.notify();
        });

        if let Some(state) = restored {
            self.list_query = state.keyword.clone();
            self.list_search
                .update(cx, |input, cx| input.set_value(state.keyword, window, cx));
            self.entries.update(cx, |list, cx| {
                list.delegate_mut().restore(state.rows, state.selected);
                cx.notify();
            });
            self.focus_list_search(window, cx);
            cx.notify();
            return;
        }

        self.list_search
            .update(cx, |input, cx| input.set_value("", window, cx));

        match kind {
            ListKind::Search => {}
            ListKind::Bookmarks | ListKind::History => self.load_firefox(kind, cx),
            ListKind::Projects | ListKind::Stars => self.load_repos(kind, false, cx),
        }
        self.entries.update(cx, |list, cx| {
            list.set_selected_index(Some(IndexPath::new(0)), window, cx);
        });
        // 列表页的搜索框永远是默认焦点：进来就能过滤。
        self.focus_list_search(window, cx);
        cx.notify();
    }

    /// 把焦点交回这一页该有输入的地方。
    ///
    /// `Tab` 和「焦点被别的东西拿走」都靠它收拾：面板里除了输入框没有该吃键盘的控件，
    /// 焦点一旦跑到别处，用户敲字就完全没有反应（GPUI 的 `Root` 把 `tab` 绑成了切焦点）。
    pub(crate) fn focus_active_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.page {
            Page::Commands => self.focus_search(window, cx),
            Page::List(_) => self.focus_list_search(window, cx),
            Page::UserTools => {
                let handle = self.tools_search.read(cx).focus_handle(cx);
                window.focus(&handle, cx);
            }
            Page::Calculator => self.focus_calculator_last(window, cx),
            // 设置页只有那几个文本框吃键盘，没有哪个算「默认」的，焦点留空。
            Page::Settings(_) => {
                if self.settings_input_focused(window, cx) {
                    return;
                }
                // 编辑区开着就把焦点送回名字框，用户接着改就行。
                if let Some(editor) = self.tool_editor.as_ref() {
                    editor.name.read(cx).focus_handle(cx).focus(window, cx);
                    return;
                }
                window.blur(cx);
            }
        }
    }

    /// 这一页是不是已经有输入框拿着焦点。
    pub(crate) fn active_input_focused(&self, window: &Window, cx: &App) -> bool {
        match self.page {
            Page::Commands => self.search.read(cx).focus_handle(cx).is_focused(window),
            Page::UserTools => self
                .tools_search
                .read(cx)
                .focus_handle(cx)
                .is_focused(window),
            Page::List(_) => self
                .list_search
                .read(cx)
                .focus_handle(cx)
                .is_focused(window),
            Page::Calculator => self
                .calculator
                .iter()
                .any(|row| row.input.read(cx).focus_handle(cx).is_focused(window)),
            Page::Settings(_) => self.settings_input_focused(window, cx),
        }
    }

    /// 设置页那几个文本框里有没有一个拿着焦点（固定的三个 + 工具编辑区那两个）。
    fn settings_input_focused(&self, window: &Window, cx: &App) -> bool {
        let fixed = [
            &self.firefox_input,
            &self.github_user_input,
            &self.github_token_input,
        ]
        .into_iter()
        .any(|input| input.read(cx).focus_handle(cx).is_focused(window));
        if fixed {
            return true;
        }
        self.tool_editor.as_ref().is_some_and(|editor| {
            [&editor.name, &editor.target, &editor.start_dir]
                .into_iter()
                .any(|input| input.read(cx).focus_handle(cx).is_focused(window))
        })
    }

    /// 焦点给列表页的搜索框。进出页面、点完行、点完按钮都调它。
    pub(crate) fn focus_list_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.list_search.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    /// 页头那枚按钮的动作：仓库两页是「重新去网上取」，查询仓库是「立刻按现在的词搜」。
    pub(crate) fn activate_list_action(&mut self, kind: ListKind, cx: &mut Context<Self>) {
        match kind {
            ListKind::Search => {
                let keyword = self.list_search.read(cx).value().trim().to_string();
                if !keyword.is_empty() && keyword != self.list_query {
                    self.run_search(keyword, cx);
                }
            }
            _ => self.refresh_list(cx),
        }
    }

    /// 页头那枚按钮：不管缓存，重新去源头取一份。
    pub(crate) fn refresh_list(&mut self, cx: &mut Context<Self>) {
        match self.page {
            Page::List(ListKind::Bookmarks) => self.load_firefox(ListKind::Bookmarks, cx),
            Page::List(ListKind::History) => self.load_firefox(ListKind::History, cx),
            Page::List(kind) if kind.is_repos() => self.load_repos(kind, true, cx),
            _ => {}
        }
    }

    /// 搜索框内容变了：本地列表立刻过滤，**不发请求**。
    ///
    /// 查询仓库页也只在回车/点右上角按钮时才联网，打字过程中一个包都不发。
    /// 它同样**不做本地过滤**：那是全网的搜索结果，`language:rust` 这种限定符
    /// 一过滤就把结果全滤没了。
    fn list_search_changed(&mut self, text: String, cx: &mut Context<Self>) {
        let Page::List(kind) = self.page else {
            return;
        };
        cx.notify();
        if kind == ListKind::Search {
            return;
        }
        self.entries.update(cx, |list, cx| {
            list.delegate_mut().set_query(&text);
            cx.notify();
        });
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
            // 只有缓存，没有就干等着用户按刷新：进页面不联网。
            let cached = cache::read(&cache_key).and_then(|text| github::from_json(&text));
            if let Some(repos) = cached {
                self.install_entries(repo_entries(repos), cx);
            } else {
                self.entries.update(cx, |list, cx| {
                    list.delegate_mut()
                        .set_message(Some(strings::lists::NO_CACHE.to_string()));
                    cx.notify();
                });
            }
            return;
        }

        self.entries.update(cx, |list, cx| {
            list.delegate_mut().set_loading(true);
            cx.notify();
        });
        let token = settings.github_token.trim().to_string();
        // 网络是阻塞的（WinHTTP），和读库一样扔到后台线程去跑。
        let fetching = cx.background_executor().spawn(async move {
            if kind == ListKind::Projects {
                github::owned_repos(&user, &token)
            } else {
                github::starred(&user, &token)
            }
        });
        self.pending = Some(cx.spawn(async move |this, cx| {
            let outcome = fetching.await;
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
        // 记下来：同一个词再按回车就是「打开选中的仓库」，不再重复搜。
        self.list_query = keyword.clone();
        self.entries.update(cx, |list, cx| {
            list.delegate_mut().mark_searched();
            list.delegate_mut().set_loading(true);
            cx.notify();
        });
        let searching = cx
            .background_executor()
            .spawn(async move { github::search(&keyword, &token) });
        self.pending = Some(cx.spawn(async move |this, cx| {
            let outcome = searching.await;
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
        // 管理员模式下开机启动由那条计划任务负责，注册表那条 Run 反而是多余的。
        // 关掉「开机启动」等于把任务也删掉（两者本来就是同一件事），所以先把这个
        // 开关本身记成关，免得删完任务之后又被下面那句「换回注册表」重新打开。
        if stored_settings(cx).run_as_admin {
            let mut settings = stored_settings(cx);
            settings.launch_at_login = enabled;
            store_settings(settings, cx);
            if !enabled {
                self.set_run_as_admin(false, cx);
            }
            cx.notify();
            return;
        }
        if !autostart::set_enabled(enabled) {
            // 开关是受控值，写失败它就停在原处：如实报告，别装作改成了。
            eprintln!("tools: 开机启动项没能更新");
            cx.notify();
            return;
        }
        let mut settings = stored_settings(cx);
        settings.launch_at_login = enabled;
        store_settings(settings, cx);
        cx.notify();
    }

    /// 重新读一遍设置里关于每条命令（以及每条用户工具）的内容。
    pub(crate) fn refresh_catalogue(&mut self, cx: &mut Context<Self>) {
        let settings = stored_settings(cx);
        let states = command_states(&settings);
        let tools = settings.tools;
        self.list.update(cx, |list, cx| {
            list.delegate_mut().set_catalogue(states, tools);
            cx.notify();
        });
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
        self.sync_user_list(cx);
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
        tools::admin::log(&format!(
            "录制中收到按键 key={} ctrl={} alt={} shift={}",
            keystroke.key,
            keystroke.modifiers.control,
            keystroke.modifiers.alt,
            keystroke.modifiers.shift,
        ));
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
                self.sync_user_list(cx);
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
            self.conflict = Some(strings::commands_page::conflict(&owner.label(cx)));
        }
        self.sync_user_list(cx);
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
                    eprintln!("tools: 唤出热键这次没能绑定，要等下次启动再试");
                }
            }
            RecordTarget::Command(command) => {
                let (section, id) = (command.section.key(), command.id);
                if settings.command_shortcut(section, id).map(str::to_string) == chord {
                    return;
                }
                settings.set_command_shortcut(section, id, chord);
                store_settings(settings, cx);
                apply_shortcuts(cx);
                self.refresh_catalogue(cx);
            }
            RecordTarget::Tool(id) => {
                if settings.tool(id).and_then(|tool| tool.shortcut.clone()) == chord {
                    return;
                }
                settings.set_tool_shortcut(id, chord);
                store_settings(settings, cx);
                apply_shortcuts(cx);
                self.refresh_catalogue(cx);
            }
        }
        self.conflict = None;
        cx.notify();
    }

    /// 开始听下一组按键，作为某条用户工具的快捷键；再点一次取消。
    pub(crate) fn toggle_tool_recording(&mut self, id: i64, cx: &mut Context<Self>) {
        self.toggle_recording(RecordTarget::Tool(id), cx);
        tools::admin::log(&format!("录制工具 {id} → {:?}", self.recording.is_some()));
    }

    /// 加一条文件：先让用户去系统文件框里挑，挑完再落库。
    /// 新增一条用户条目（按当前类别走不同的选择器）。
    pub(crate) fn add_user_tool(
        &mut self,
        kind: ToolKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match kind {
            ToolKind::File => self.add_file_tool(cx),
            ToolKind::Folder => self.add_folder_tool(cx),
            ToolKind::Terminal => self.new_terminal_tool(window, cx),
        }
    }

    pub(crate) fn add_file_tool(&mut self, cx: &mut Context<Self>) {
        self.pick(PickFor::NewFile, cx);
    }

    /// 加一条文件夹：挑的是目录。
    pub(crate) fn add_folder_tool(&mut self, cx: &mut Context<Self>) {
        self.pick(PickFor::NewFolder, cx);
    }

    /// 开一个空的快捷终端编辑区。
    pub(crate) fn new_terminal_tool(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.start_tool_editor(ToolKind::Terminal, None, window, cx);
    }

    /// 打开编辑区：`id` 为 `None` 时是新增，否则把那一条的内容填进去。
    pub(crate) fn start_tool_editor(
        &mut self,
        kind: ToolKind,
        id: Option<i64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let existing = id.and_then(|id| stored_settings(cx).tool(id).cloned());
        let placeholder = match kind {
            ToolKind::File => strings::tools_page::FILE_PLACEHOLDER,
            ToolKind::Folder => strings::tools_page::FOLDER_PLACEHOLDER,
            ToolKind::Terminal => strings::tools_page::COMMAND_PLACEHOLDER,
        };
        let name_placeholder = strings::tools_page::NAME_PLACEHOLDER;
        let dir_placeholder = strings::tools_page::START_DIR_PLACEHOLDER;
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(name_placeholder)
                .default_value(
                    existing
                        .as_ref()
                        .map(|tool| tool.name.clone())
                        .unwrap_or_default(),
                )
        });
        let target = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(placeholder)
                .default_value(
                    existing
                        .as_ref()
                        .map(|tool| tool.target.clone())
                        .unwrap_or_default(),
                )
        });
        let start_dir = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(dir_placeholder)
                .default_value(
                    existing
                        .as_ref()
                        .and_then(|tool| tool.start_dir.clone())
                        .unwrap_or_default(),
                )
        });
        name.read(cx).focus_handle(cx).focus(window, cx);
        // 几个框里按回车都等于点「保存」：填表的手感就该是这样。事件回调里没有窗口，
        // 所以走 `with_launcher` 推到下一拍。
        let events = [&name, &target, &start_dir]
            .into_iter()
            .map(|input| {
                cx.subscribe(input, |_launcher, _input, event, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        with_launcher(cx, |launcher, window, cx| {
                            launcher.save_tool_editor(window, cx)
                        });
                    }
                })
            })
            .collect();
        self.tool_editor = Some(ToolEditor {
            kind,
            id,
            name,
            target,
            start_dir,
            _events: events,
        });
        self.tool_notice = None;
        cx.notify();
    }

    /// 保存编辑区：新的就建一条，已有的就地改掉名字和路径。
    ///
    /// 名字和路径（命令）都得填：主页上那两行就是它们，空一个就等于建了条认不出来的
    /// 命令。文件 / 文件夹从系统对话框**直接**加进来时另说，那时候名字由文件（夹）名
    /// 兜底（见 [`Self::finish_pick`]）。起始目录留空是合法的，含义是用户文件夹。
    pub(crate) fn save_tool_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.tool_editor.as_ref() else {
            return;
        };
        let (kind, id) = (editor.kind, editor.id);
        let name = editor.name_text(cx);
        let target = editor.target_text(cx);
        let start_dir = editor.start_dir_text(cx);
        if name.is_empty() {
            self.tool_notice = Some(strings::tools_page::NAME_REQUIRED);
            return;
        }
        if target.is_empty() {
            self.tool_notice = Some(strings::tools_page::target_required(kind));
            return;
        }
        self.tool_editor = None;

        let mut settings = stored_settings(cx);
        let id = match id {
            Some(id) => {
                if !settings.update_tool(id, name, target) {
                    // 这一条在别处被删了，就当这次编辑没发生。
                    cx.notify();
                    return;
                }
                id
            }
            None => settings.add_tool(kind, name, target),
        };
        if kind == ToolKind::Terminal {
            settings.set_tool_start_dir(id, Some(start_dir));
        }
        store_settings(settings, cx);
        self.tool_notice = None;
        self.refresh_catalogue(cx);
        window.blur(cx);
        cx.notify();
    }

    /// 放弃编辑区里的内容。
    pub(crate) fn cancel_tool_editor(&mut self, cx: &mut Context<Self>) {
        self.tool_editor = None;
        self.tool_notice = None;
        cx.notify();
    }

    /// 删掉一条用户工具：库里删掉，还挂在它身上的快捷键也放掉。
    pub(crate) fn delete_tool(&mut self, id: i64, cx: &mut Context<Self>) {
        let mut settings = stored_settings(cx);
        if settings.remove_tool(id).is_none() {
            return;
        }
        store_settings(settings, cx);
        apply_shortcuts(cx);
        if self
            .tool_editor
            .as_ref()
            .is_some_and(|editor| editor.id == Some(id))
        {
            self.tool_editor = None;
        }
        if self.prompt.as_ref().is_some_and(|prompt| prompt.id == id) {
            self.prompt = None;
        }
        self.refresh_catalogue(cx);
        cx.notify();
    }

    /// 把一条用户工具放上主页，或者从主页拿掉。
    pub(crate) fn set_tool_enabled(&mut self, id: i64, enabled: bool, cx: &mut Context<Self>) {
        let mut settings = stored_settings(cx);
        if settings
            .tool(id)
            .is_some_and(|tool| tool.enabled == enabled)
        {
            return;
        }
        settings.set_tool_enabled(id, enabled);
        store_settings(settings, cx);
        self.refresh_catalogue(cx);
        cx.notify();
    }

    /// 路径不在了：把「删还是改」的提示摆出来（面板上那张卡片）。
    pub(crate) fn prompt_missing_path(
        &mut self,
        id: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tool) = stored_settings(cx).tool(id).cloned() else {
            return;
        };
        self.prompt = Some(MissingPrompt {
            id,
            name: tool.name,
            target: tool.target,
        });
        // 提示框靠鼠标点：焦点不能留在看不见的搜索框上。
        window.blur(cx);
        cx.notify();
    }

    /// 收起提示框，什么都不做。
    pub(crate) fn dismiss_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt = None;
        self.focus_active_input(window, cx);
        cx.notify();
    }

    /// 提示框里的「删除」：这一条从库里拿掉。
    pub(crate) fn prompt_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(prompt) = self.prompt.take() else {
            return;
        };
        self.delete_tool(prompt.id, cx);
        self.focus_active_input(window, cx);
    }

    /// 提示框里的「修改」：再去挑一次（文件条目挑文件，文件夹条目挑目录），
    /// 挑完把这一条的路径换掉。
    pub(crate) fn prompt_relocate(&mut self, cx: &mut Context<Self>) {
        let Some(prompt) = self.prompt.take() else {
            return;
        };
        self.pick(PickFor::Replace(prompt.id), cx);
        cx.notify();
    }

    /// 编辑区里的「选择文件 / 选择文件夹」：挑完只改输入框，保存才落库。
    pub(crate) fn pick_editor_target(&mut self, cx: &mut Context<Self>) {
        if self.tool_editor.is_none() {
            return;
        }
        self.pick(PickFor::EditorTarget, cx);
    }

    /// 编辑区里的「起始目录」：只挑目录。
    pub(crate) fn pick_editor_start_dir(&mut self, cx: &mut Context<Self>) {
        if self.tool_editor.is_none() {
            return;
        }
        self.pick(PickFor::EditorStartDir, cx);
    }

    /// 文件条目：切换「用默认应用打开 / 打开所在位置」。
    pub(crate) fn toggle_tool_open_default(&mut self, id: i64, cx: &mut Context<Self>) {
        let mut settings = stored_settings(cx);
        let Some(tool) = settings.tool(id) else {
            return;
        };
        let next = !tool.open_default;
        settings.set_tool_open_default(id, next);
        store_settings(settings, cx);
        self.refresh_catalogue(cx);
        cx.notify();
    }

    /// 终端条目：切换「开终端窗口跑 / 静默跑」。
    pub(crate) fn toggle_tool_quiet(&mut self, id: i64, cx: &mut Context<Self>) {
        let mut settings = stored_settings(cx);
        let Some(tool) = settings.tool(id) else {
            return;
        };
        let next = !tool.quiet;
        settings.set_tool_quiet(id, next);
        store_settings(settings, cx);
        self.refresh_catalogue(cx);
        cx.notify();
    }

    /// 打开 / 关掉「以管理员身份启动」。
    ///
    /// 这条设置不是往数据库里写一个布尔值就完事：它对应系统里的一条计划任务。
    /// 建任务本身要提权（`schtasks /RL HIGHEST`），所以**开启时会弹一次 UAC**；
    /// 建成之后立刻用那条任务重启自己——新进程天生提权，用户不会再看到第二个弹窗。
    /// 关掉就是把任务删掉，开机启动退回注册表那一条。
    pub(crate) fn set_run_as_admin(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.admin_busy {
            return;
        }
        if enabled == admin::installed() && enabled == stored_settings(cx).run_as_admin {
            return;
        }
        self.admin_busy = true;
        self.admin_notice = Some(strings::settings_page::ADMIN_INSTALLING);
        cx.notify();

        let exe = std::env::current_exe().unwrap_or_default();
        let work = cx.background_executor().spawn(async move {
            if enabled {
                admin::install(&exe)
            } else {
                admin::remove()
            }
        });
        // 上一次的句柄先放下：这里替换的是已经跑完的那个，丢掉没有副作用。
        self.admin_task = Some(cx.spawn(async move |_this, cx| {
            let ok = work.await;
            cx.update(move |cx: &mut App| {
                let mut settings = stored_settings(cx);
                if enabled {
                    // 任务在不在才是事实，成功与否都以它为准（schtasks 的退出码只是旁证）。
                    let installed = admin::installed();
                    if !installed {
                        eprintln!(
                            "tools: 计划任务没能建成（schtasks 返回 {ok}），管理员模式没打开"
                        );
                    }
                    settings.run_as_admin = installed;
                    if installed {
                        // 任务带的是「登录时启动」，注册表那条 Run 就别再留一份。
                        autostart::set_enabled(false);
                        settings.launch_at_login = true;
                    }
                    store_settings(settings, cx);
                    with_launcher(cx, move |launcher, _window, cx| {
                        launcher.admin_busy = false;
                        launcher.admin_notice =
                            (!installed).then_some(strings::settings_page::ADMIN_FAILED);
                        cx.notify();
                    });
                    if installed {
                        // 用任务重启：新进程是提权的，而且不会再问一次。
                        crate::actions::restart_launcher(cx);
                    }
                } else {
                    let still = admin::installed();
                    if still {
                        eprintln!("tools: 计划任务没能删掉（schtasks 返回 {ok}）");
                    }
                    settings.run_as_admin = still;
                    let launch_at_login = settings.launch_at_login;
                    store_settings(settings, cx);
                    // 开机启动还想要的话，换回注册表那条（普通权限）。
                    if launch_at_login {
                        autostart::set_enabled(true);
                    }
                    with_launcher(cx, move |launcher, _window, cx| {
                        // 关掉之后不重启，开关必须马上能再点。
                        launcher.admin_busy = false;
                        launcher.admin_notice =
                            still.then_some(strings::settings_page::ADMIN_FAILED);
                        cx.notify();
                    });
                }
            });
        }));
    }

    /// 弹系统的文件框（或文件夹框）挑一个目标。
    ///
    /// 对话框是阻塞的，所以整件事走后台线程：线程里挑完，再回界面线程把结果交回来。
    /// 这期间 [`Self::dialog`] 是 `Some`，面板不会因为失去前台而收起来。
    fn pick(&mut self, purpose: PickFor, cx: &mut Context<Self>) {
        cx.notify();
        let owner = crate::app::panel_handle(cx);
        let folder = match purpose {
            PickFor::NewFolder | PickFor::EditorStartDir => true,
            // 换路径时按这一条自己的种类走：文件夹条目不能再变成文件。
            PickFor::Replace(id) => stored_settings(cx)
                .tool(id)
                .is_some_and(|tool| tool.kind == ToolKind::Folder),
            PickFor::NewFile | PickFor::EditorTarget => false,
        };
        let title = if folder {
            strings::tools_page::PICK_FOLDER_TITLE
        } else {
            strings::tools_page::PICK_TITLE
        };
        let picking = cx.background_executor().spawn(async move {
            if folder {
                tools::dialog::pick_folder(owner, title)
            } else {
                tools::dialog::pick_file(owner, title)
            }
        });
        self.dialog = Some(cx.spawn(async move |_this, cx| {
            let picked = picking.await;
            cx.update(move |cx: &mut App| {
                with_launcher(cx, move |launcher, window, cx| {
                    launcher.finish_pick(purpose, picked, window, cx)
                });
            });
        }));
    }

    /// 对话框关掉了：把挑到的东西按原来的用途用掉。
    fn finish_pick(
        &mut self,
        purpose: PickFor,
        picked: Option<std::path::PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dialog = None;
        // 给前台留一点回来的时间，这期间失焦检查不算数。
        self.front_grace = Some(Instant::now() + FRONT_GRACE);
        crate::app::reclaim_foreground(cx);
        let Some(path) = picked else {
            cx.notify();
            return;
        };
        let target = path.display().to_string();
        match purpose {
            PickFor::NewFile | PickFor::NewFolder => {
                let kind = if matches!(purpose, PickFor::NewFile) {
                    ToolKind::File
                } else {
                    ToolKind::Folder
                };
                let mut settings = stored_settings(cx);
                let name = default_tool_name(&target);
                settings.add_tool(kind, name, target);
                store_settings(settings, cx);
                self.refresh_catalogue(cx);
            }
            PickFor::Replace(id) => {
                let mut settings = stored_settings(cx);
                if let Some(tool) = settings.tool(id).cloned() {
                    settings.update_tool(id, tool.name, target);
                    store_settings(settings, cx);
                    self.refresh_catalogue(cx);
                }
            }
            PickFor::EditorTarget => {
                if let Some(editor) = self.tool_editor.as_ref() {
                    editor
                        .target
                        .update(cx, |input, cx| input.set_value(&target, window, cx));
                }
            }
            PickFor::EditorStartDir => {
                if let Some(editor) = self.tool_editor.as_ref() {
                    editor
                        .start_dir
                        .update(cx, |input, cx| input.set_value(&target, window, cx));
                }
            }
        }
        // 挑完文件，焦点还给编辑区（名字框），接着就能改名字。
        if let Some(editor) = self.tool_editor.as_ref() {
            editor.name.read(cx).focus_handle(cx).focus(window, cx);
        }
        cx.notify();
    }
}

/// 从对话框直接加条目时的默认名：文件取文件名（去掉扩展名），文件夹就取目录名。
fn default_tool_name(target: &str) -> String {
    let path = std::path::Path::new(target);
    path.file_stem()
        .or_else(|| path.file_name())
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| target.to_string())
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
            .with_icon(bookmark.icon.as_deref().map(favicon_image))
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
            .with_icon(visit.icon.as_deref().map(favicon_image))
        })
        .collect()
}

/// 仓库行：标题是 `owner/name`，副行是简介（没有就写网址），右边是「★ 数 · 主语言」。
fn repo_entries(rows: Vec<github::Repo>) -> Vec<Entry> {
    rows.into_iter()
        .map(|repo| {
            // 第一行放语言（靠右），第二行是「★ 星数 · 简介」。
            let detail = if repo.description.is_empty() {
                format!("\u{2605} {}", repo.stars)
            } else {
                format!("\u{2605} {} \u{00b7} {}", repo.stars, repo.description)
            };
            Entry::new(
                repo.full_name,
                detail,
                repo.language,
                repo.url,
                gpui_kit::assets::IconName::Github,
            )
        })
        .collect()
}
