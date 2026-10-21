//! 面板本体：页头、页脚、命令列表，以及面板内的导航。

use std::time::{Duration, Instant};

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState, TextareaState};
use gpui_kit::component::list::{List, ListState};
use gpui_kit::component::slider::{SliderEvent, SliderState};
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, IndexPath, Sizable as _, h_flex};
use gpui_kit::{
    AnyElement, App, ClipboardItem, Context, Entity, Focusable, Font, FontWeight, IntoElement,
    Pixels, Render, Subscription, Task, TextRun, Window, WindowControlArea, div, prelude::*, px,
};
use gpui_tools::autostart;
use gpui_tools::calculator;
use gpui_tools::settings::Settings;

use crate::app::{
    LauncherPanel, apply_theme, hide_launcher, run_command, store_settings, stored_settings,
    with_launcher,
};
use crate::catalogue;
use crate::shortcuts::{
    ShortcutOwner, apply_command_shortcuts, apply_summon_shortcut, chord_from_keystroke,
};
use crate::strings;
use crate::ui::colors::UiColors;
use crate::ui::commands_list::{ToolDelegate, command_states};
use crate::ui::settings_page;
use gpui_tools::settings::Theme as ColorScheme;

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
}

/// 设置里的子模块，顺序就是左侧列的顺序。
///
/// 它们同时也是命令页给命令分组的依据，所以命令在 `catalogue.rs` 里声明时就属于其中一个。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SettingsTab {
    System,
    Commands,
    Appearance,
    Calculator,
    About,
}

impl SettingsTab {
    /// 全部子模块，按界面上的顺序。
    pub(crate) const ALL: [Self; 5] = [
        Self::System,
        Self::Commands,
        Self::Appearance,
        Self::Calculator,
        Self::About,
    ];

    /// 这个标签对应命令目录里的哪个子模块。
    pub(crate) fn section(self) -> catalogue::Section {
        match self {
            Self::System => catalogue::Section::System,
            Self::Commands => catalogue::Section::Commands,
            Self::Appearance => catalogue::Section::Appearance,
            Self::Calculator => catalogue::Section::Calculator,
            Self::About => catalogue::Section::About,
        }
    }

    /// 界面上的名字。
    pub(crate) fn label(self) -> &'static str {
        self.section().label()
    }

    /// 界面上的图标。
    pub(crate) fn icon(self) -> gpui_kit::assets::IconName {
        self.section().icon()
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
    /// 最后一次离开设置页停在哪个子模块、什么时候走的。保留期内再唤出就回那一页。
    last_section: Option<(Instant, SettingsTab)>,
    /// 盯着面板有没有失去前台的循环。每次唤起重开，旧的那条已经结束了。
    focus_watch: Option<Task<()>>,
    /// 命令页当前显示第几个子模块，`catalogue::Section::ALL` 的下标。
    pub(crate) section: usize,
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
    /// 跟着视图活着的订阅，丢了输入框就不再上报用户敲了什么。
    _search_events: Subscription,
    _opacity_events: Subscription,
    _search_observation: Subscription,
}

impl Launcher {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let states = command_states(&stored_settings(cx));
        let list = cx.new(|cx| ListState::new(ToolDelegate::new(states), window, cx));
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

        let mut launcher = Self {
            list,
            search,
            query: String::new(),
            page: Page::Commands,
            last_section: None,
            focus_watch: None,
            section: 0,
            opacity,
            calculator: Vec::new(),
            next_row_id: 0,
            current_row: None,
            copied: None,
            recording: None,
            conflict: None,
            _search_events: search_events,
            _opacity_events: opacity_events,
            _search_observation: search_observation,
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

    /// 上下键：命令页里移高亮，设置页里换子模块。
    pub(crate) fn navigate(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        match self.page {
            Page::Commands => self.move_selection(delta, window, cx),
            Page::Settings(tab) => self.select_tab(tab.shifted(delta), window, cx),
        }
    }

    /// 高亮挪 `delta` 行，夹在可见命令范围内。
    fn move_selection(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.list.read(cx).delegate().visible_count();
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
        self.page = match self.resumed_tab(cx) {
            Some(tab) => Page::Settings(tab),
            None => Page::Commands,
        };
        self.focus_search(window, cx);
        // 回到计算器页就把光标放进最后那行空行里：唤出即打，不用再点一下。
        if matches!(self.page, Page::Settings(SettingsTab::Calculator)) {
            self.focus_calculator_last(window, cx);
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

    /// 这次唤出该回到哪个设置子模块——离开得够近、还值得记着就回。
    fn resumed_tab(&self, cx: &Context<Self>) -> Option<SettingsTab> {
        let (at, tab) = self.last_section?;
        let window = stored_settings(cx).resume_seconds;
        (window > 0 && at.elapsed() < Duration::from_secs(window.into())).then_some(tab)
    }

    /// 记下离开时停在哪一页，`reset` 才有地方回去。
    ///
    /// 两条离开路径都算：收起面板，以及退回命令页——因为 Esc 是先退页再收面板。
    pub(crate) fn remember_section(&mut self) {
        if let Page::Settings(tab) = self.page {
            self.last_section = Some((Instant::now(), tab));
        }
    }

    /// 收起面板前收拾现场：记下停在哪一页，并结束录制。
    ///
    /// **录制不能跟着面板一起藏起来**：全局热键分发是按「有没有在录制」决定执行还是
    /// 报冲突的，面板藏了但录制还在，所有热键都会静默失效——连唤出面板的那一个都
    /// 按不回来，而按钮早就看不见了。
    pub(crate) fn leave(&mut self) {
        self.recording = None;
        self.conflict = None;
        self.remember_section();
    }

    /// 执行高亮的那条命令。
    pub(crate) fn activate_selected(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.page, Page::Commands) {
            return;
        }
        if let Some(command) = self.list.read(cx).delegate().selected_command() {
            run_command(command, cx);
        }
    }

    /// Esc 先从设置页退出来，再收面板。
    pub(crate) fn escape(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.page, Page::Settings(_)) {
            self.back_to_commands(window, cx);
        } else {
            hide_launcher(cx);
        }
    }

    /// 把命令列表换成设置页。
    pub(crate) fn open_settings(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.page = Page::Settings(SettingsTab::System);
        cx.notify();
    }

    /// 回到命令列表，输入框准备好接收按键。
    pub(crate) fn back_to_commands(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.remember_section();
        self.page = Page::Commands;
        self.focus_search(window, cx);
        cx.notify();
    }

    /// 换一个设置子模块。
    ///
    /// 切到计算器页时把焦点交给最后那行空行：它是这一页唯一要用的输入框。
    pub(crate) fn select_tab(
        &mut self,
        tab: SettingsTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.page == Page::Settings(tab) {
            return;
        }
        self.page = Page::Settings(tab);
        if matches!(tab, SettingsTab::Calculator) {
            self.focus_calculator_last(window, cx);
        }
        cx.notify();
    }

    /// 把命令列表换成计算器页（主页上的「计算器」命令）。
    pub(crate) fn open_calculator(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.page = Page::Settings(SettingsTab::Calculator);
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
        let Some(text) = calculator::result_text(&row.input.read(cx).value()) else {
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

    /// 这一行公式算出来的数；算不了就是 `None`。
    pub(crate) fn calculator_result(&self, id: usize, cx: &App) -> Option<String> {
        let row = self.calculator.iter().find(|row| row.id == id)?;
        calculator::result_text(&row.input.read(cx).value())
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
            Page::Settings(_) => bar
                .child(
                    div().id("settings-back").occlude().flex_shrink_0().child(
                        Button::new("back-to-commands")
                            .ghost()
                            .icon(IconName::ArrowLeft)
                            .tooltip(strings::launcher::TOOLTIP_BACK)
                            .on_click(cx.listener(|launcher, _, window, cx| {
                                launcher.back_to_commands(window, cx)
                            })),
                    ),
                )
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .child(strings::launcher::SETTINGS_TITLE),
                )
                .child(div().flex_1())
                .into_any_element(),
        }
    }
}

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

    let theme = cx.theme();
    let colors = UiColors::current(cx);
    let run = TextRun {
        len: query.len(),
        font: Font {
            family: theme.font_family.clone(),
            ..Default::default()
        },
        color: colors.foreground,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let text = window.text_system().shape_line(
        query.to_string().into(),
        px(SEARCH_TEXT_SIZE),
        &[run],
        None,
    );
    (text.width() + padding + px(SEARCH_FIELD_SLACK)).min(px(SEARCH_FIELD_MAX))
}

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
        let count = self.list.read(cx).delegate().visible_count();

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
        };

        let footer = h_flex()
            .px_3()
            .py_2()
            .gap_4()
            .border_t_1()
            .border_color(border)
            .text_xs()
            .children(hints.into_iter().map(|(key, label)| chip(key, label)))
            .child(div().flex_1())
            .when(!trailing.is_empty(), move |this| {
                this.child(div().text_color(muted_fg).child(trailing))
            });

        let body = match self.page {
            Page::Commands => div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .child(List::new(&self.list).px_1().py_1())
                .into_any_element(),
            Page::Settings(tab) => settings_page::render(self, tab, cx).into_any_element(),
        };

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
    }
}
