//! 面板本体：页头、页脚、命令列表，以及面板内的导航。

use std::time::{Duration, Instant};

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::list::{List, ListState};
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, IndexPath, Sizable as _, h_flex};
use gpui_kit::{
    AnyElement, Context, Entity, Focusable, Font, FontWeight, IntoElement, Pixels, Render,
    Subscription, Task, TextRun, Window, WindowControlArea, div, prelude::*, px,
};
use gpui_tools::autostart;
use gpui_tools::settings::Settings;

use crate::app::{
    LauncherPanel, apply_theme, hide_launcher, run_command, store_settings, stored_settings,
    with_launcher,
};
use crate::catalogue;
use crate::shortcuts::{apply_command_shortcuts, chord_from_keystroke};
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
    About,
}

impl SettingsTab {
    /// 全部子模块，按界面上的顺序。
    pub(crate) const ALL: [Self; 4] = [Self::System, Self::Commands, Self::Appearance, Self::About];

    /// 这个标签对应命令目录里的哪个子模块。
    pub(crate) fn section(self) -> catalogue::Section {
        match self {
            Self::System => catalogue::Section::System,
            Self::Commands => catalogue::Section::Commands,
            Self::Appearance => catalogue::Section::Appearance,
            Self::About => catalogue::Section::About,
        }
    }

    /// 界面上的名字。
    pub(crate) fn label(self) -> &'static str {
        self.section().label()
    }

    /// 界面上的图标。
    pub(crate) fn icon(self) -> IconName {
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
    /// 正在录制的命令。
    recording: Option<&'static catalogue::Command>,
    /// 跟着视图活着的订阅，丢了输入框就不再上报用户敲了什么。
    _search_events: Subscription,
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

        let launcher = Self {
            list,
            search,
            query: String::new(),
            page: Page::Commands,
            last_section: None,
            focus_watch: None,
            section: 0,
            recording: None,
            _search_events: search_events,
            _search_observation: search_observation,
        };
        // GPUI 默认不给焦点，用户点一下输入框之前，面板会把所有按键都吞掉。
        launcher.focus_search(window, cx);
        launcher.select_row(0, window, cx);
        launcher
    }

    /// 正在录制的命令。
    pub(crate) fn recording(&self) -> Option<&'static catalogue::Command> {
        self.recording
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
            Page::Settings(tab) => self.select_tab(tab.shifted(delta), cx),
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
        self.refresh_catalogue(cx);
        self.list
            .update(cx, |list, cx| list.set_query("", window, cx));
        self.page = match self.resumed_tab(cx) {
            Some(tab) => Page::Settings(tab),
            None => Page::Commands,
        };
        self.focus_search(window, cx);
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
    pub(crate) fn select_tab(&mut self, tab: SettingsTab, cx: &mut Context<Self>) {
        if self.page == Page::Settings(tab) {
            return;
        }
        self.page = Page::Settings(tab);
        cx.notify();
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
    pub(crate) fn toggle_recording(
        &mut self,
        command: &'static catalogue::Command,
        cx: &mut Context<Self>,
    ) {
        self.recording = if self
            .recording
            .is_some_and(|current| std::ptr::eq(current, command))
        {
            None
        } else {
            Some(command)
        };
        cx.notify();
    }

    /// 把接下来这一下按键当成正在录制的那条命令的快捷键。
    ///
    /// 只收带修饰键的组合：没有修饰键的会被面板自己吃掉，做全局快捷键没有意义。
    pub(crate) fn capture_shortcut(
        &mut self,
        keystroke: &gpui_kit::Keystroke,
        cx: &mut Context<Self>,
    ) {
        let Some(command) = self.recording else {
            return;
        };
        match keystroke.key.as_str() {
            "escape" => self.recording = None,
            "backspace" | "delete" => {
                self.set_shortcut(command, None, cx);
                self.recording = None;
            }
            _ => {
                let Some(chord) = chord_from_keystroke(keystroke) else {
                    // 这个键用不了，继续等下一组。
                    return;
                };
                self.set_shortcut(command, Some(chord), cx);
                self.recording = None;
            }
        }
        cx.notify();
    }

    /// 给命令绑一个组合键，或者解绑。
    fn set_shortcut(
        &mut self,
        command: &'static catalogue::Command,
        chord: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let mut settings = stored_settings(cx);
        let (section, id) = (command.section.key(), command.id);
        if settings.command_shortcut(section, id).map(str::to_string) == chord {
            return;
        }
        settings.set_command_shortcut(section, id, chord);
        store_settings(settings, cx);
        apply_command_shortcuts(cx);
        self.refresh_catalogue(cx);
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
            colors.control,
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
                        .text_color(fg.opacity(0.85))
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

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(bg)
            .text_color(fg)
            .child(self.header(window, cx))
            .child(body)
            .child(footer)
    }
}
