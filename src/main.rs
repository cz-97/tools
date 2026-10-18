//! GPUI Tools desktop launcher.
//!
//! A hotkey-summoned, frameless panel showing a searchable list of the
//! launcher's own commands. Enter runs the highlighted one, Esc hides the
//! panel.
//!
//! The panel draws its own settings page rather than opening a second window,
//! and its header doubles as the drag surface. That is why the launcher owns the
//! text field instead of letting the list draw its own.
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use global_hotkey::GlobalHotKeyManager;
use gpui_kit::Keystroke;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::list::{List, ListDelegate, ListState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, IndexPath, Selectable, Sizable as _, Theme, ThemeMode,
    h_flex, v_flex,
};
use gpui_kit::{
    AnyElement, AnyWindowHandle, App, Bounds, ClickEvent, Context, Div, Entity, Focusable, Font,
    FontWeight, Hsla, IntoElement, Pixels, QuitMode, Render, RenderOnce, SharedString,
    Subscription, Task, TextRun, WeakEntity, Window, WindowBounds, WindowControlArea,
    WindowOptions, actions, div, prelude::*, px, size,
};
use gpui_tools::autostart;
use gpui_tools::logo;
use gpui_tools::settings::{Settings, Theme as ColorScheme};
use gpui_tools::shell;
use gpui_tools::window::NativeWindow;
use gpui_tray::{Icon as TrayIcon, Tray};

mod catalogue;

const WINDOW_TITLE: &str = "GPUI Tools Launcher";
const WINDOW_WIDTH: f32 = 720.;
const WINDOW_HEIGHT: f32 = 464.;

const REPOSITORY_URL: &str = "https://github.com/cz-97/gpui-tools";

/// The shortcut that summons the panel, as the settings page shows it.
const HOTKEY_LABEL: &str = "Ctrl + Space";

/// The icon resource `build.rs` embeds, which the window and the shell show.
const APP_ICON_ID: u16 = 1;

/// The smallest the header may be drawn, whatever the field's height.
const HEADER_MIN_HEIGHT: f32 = 52.;
/// The room the header keeps around the search field, top and bottom together.
const HEADER_SLACK: f32 = 26.;
/// The width of the settings page's section list.
const SIDEBAR_WIDTH: f32 = 148.;
/// The width of the module list inside the commands page.
const MODULE_WIDTH: f32 = 124.;
/// The padding the component draws inside the field, both sides together
/// (`input_px` is 8 for its small size). The field has to pay for it to keep
/// its text in view.
const SEARCH_FIELD_PADDING: f32 = 16.;
/// How often the panel checks whether it still owns the foreground.
const FOCUS_CHECK_INTERVAL: Duration = Duration::from_millis(60);
/// How many checks in a row may miss the foreground before the panel hides.
/// A few, so the moment between raising the window and the shell handing the
/// foreground over does not count as the user leaving.
const FOCUS_LOST_SAMPLES: u8 = 3;
/// The room an empty field keeps for the caret. Anything wider would eat into
/// the drag surface for nothing.
const SEARCH_FIELD_CARET: f32 = 4.;
/// Slack added to a measured query, so a field that is a hair too narrow never
/// scrolls the text out of sight. A full em covers a shaping fallback that
/// measures wider than the theme font does.
const SEARCH_FIELD_SLACK: f32 = 14.;
/// The widest the field may grow, leaving the icon, the hint and the settings
/// button their room.
const SEARCH_FIELD_MAX: f32 = 560.;
/// The size of the text the field draws, matching the component's medium input.
const SEARCH_TEXT_SIZE: f32 = 14.;
const SEARCH_HINT: &str = "搜索命令\u{2026}";

fn main() {
    launch();
}

/// What the list needs to know about one command, mirrored from the settings so
/// the delegate can filter and draw without touching the global record.
#[derive(Clone, Debug, PartialEq, Eq)]
struct CommandState {
    /// Whether the command belongs on the home list.
    enabled: bool,
    uses: u64,
    /// The global shortcut, as a chord such as `ctrl+alt+KeyP`.
    shortcut: Option<String>,
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

/// The state of every command, read from the settings in one pass.
fn command_states(settings: &Settings) -> BTreeMap<&'static str, CommandState> {
    catalogue::COMMANDS
        .iter()
        .map(|command| {
            (
                command.id,
                CommandState {
                    enabled: settings.command_enabled(command.id),
                    uses: settings.command_uses(command.id),
                    shortcut: settings.command_shortcut(command.id).map(str::to_string),
                },
            )
        })
        .collect()
}

/// The directory the launcher runs from.
fn program_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(std::path::Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// One command row: icon, name and description, with what the settings add —
/// how often it has run and the shortcut that runs it.
#[derive(IntoElement)]
struct ToolRow {
    command: &'static catalogue::Command,
    state: CommandState,
    selected: bool,
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
        let (fg, muted, muted_fg, accent, accent_fg) = {
            let theme = cx.theme();
            (
                theme.foreground,
                theme.muted,
                theme.muted_foreground,
                theme.accent,
                theme.accent_foreground,
            )
        };
        let selected = self.selected;

        let name_color = if selected { accent_fg } else { fg };
        let detail_color = if selected {
            accent_fg.opacity(0.7)
        } else {
            muted_fg
        };
        let tile_bg = if selected {
            accent_fg.opacity(0.14)
        } else {
            muted
        };

        // Everything the user can decide about a command stays visible on the
        // row: which section it belongs to, how often it has run, and the chord
        // that runs it — even when that is "never" and "none", so the row does
        // not silently change shape once a counter starts moving.
        let section = div()
            .text_xs()
            .text_color(detail_color)
            .child(self.command.section.label());
        let uses = div()
            .text_xs()
            .text_color(detail_color)
            .child(format!("{} 次", self.state.uses));
        let shortcut = match self.state.shortcut.clone() {
            Some(chord) => div()
                .px_1p5()
                .py_0p5()
                .rounded(px(4.))
                .bg(tile_bg)
                .text_xs()
                .text_color(detail_color)
                .child(shortcut_label(&chord))
                .into_any_element(),
            None => div()
                .text_xs()
                .text_color(detail_color.opacity(0.7))
                .child("无快捷键")
                .into_any_element(),
        };

        h_flex()
            .w_full()
            .h(px(52.))
            .px_3()
            .gap_3()
            .rounded(px(8.))
            .when(selected, move |this| this.bg(accent))
            .when(!selected, move |this| {
                this.hover(move |style| style.bg(muted))
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
                    .child(
                        Icon::new(self.command.icon.clone())
                            .size_4()
                            .text_color(if selected { accent_fg } else { fg }),
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
            )
    }
}

/// The launcher list, backed by the command catalogue.
struct ToolDelegate {
    query: String,
    selected: usize,
    /// What the settings say about each command, so filtering and drawing stay
    /// free of globals.
    states: BTreeMap<&'static str, CommandState>,
    /// Notified on every query change so the host chrome stays current.
    view: Option<WeakEntity<Launcher>>,
}

impl ToolDelegate {
    fn new(states: BTreeMap<&'static str, CommandState>) -> Self {
        Self {
            query: String::new(),
            selected: 0,
            states,
            view: None,
        }
    }

    /// Every command that the user kept on the home list and the query matches.
    fn visible(&self) -> Vec<&'static catalogue::Command> {
        catalogue::COMMANDS
            .iter()
            .filter(|command| self.enabled(command.id) && self.matches(command))
            .collect()
    }

    /// Whether a command is allowed on the home list.
    fn enabled(&self, id: &str) -> bool {
        self.states.get(id).is_none_or(|state| state.enabled)
    }

    /// What is known about a command, or its defaults.
    fn state(&self, id: &str) -> CommandState {
        self.states.get(id).cloned().unwrap_or_default()
    }

    /// Case-insensitive matching where every whitespace-separated token of the
    /// query must appear somewhere in the command's searchable text.
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

    fn visible_count(&self) -> usize {
        self.visible().len()
    }

    fn selected_command(&self) -> Option<&'static catalogue::Command> {
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
            // Deferred: a query change can arrive while the launcher is itself
            // being updated (summoning the panel clears the query), and
            // updating it again from there panics.
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
            state: self.state(command.id),
            selected: row == self.selected,
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
        hide_launcher(cx);
    }

    fn render_empty(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let label = if self.query.is_empty() {
            "没有可用的命令".to_string()
        } else {
            format!("没有匹配 \u{201c}{}\u{201d} 的命令", self.query)
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

/// Which page the panel is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    /// The command list.
    Commands,
    /// Settings, opened on the given section.
    Settings(SettingsTab),
}

/// The settings sections, in the order the section list shows them.
///
/// These are the sub-modules the commands page groups commands by, so a command
/// is declared against one of them in `catalogue.rs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SettingsTab {
    System,
    Commands,
    Appearance,
    About,
}

impl SettingsTab {
    /// Every section, in the order the section list shows them.
    const ALL: [Self; 4] = [Self::System, Self::Commands, Self::Appearance, Self::About];

    /// The catalogue section this tab shows.
    fn section(self) -> catalogue::Section {
        match self {
            Self::System => catalogue::Section::System,
            Self::Commands => catalogue::Section::Commands,
            Self::Appearance => catalogue::Section::Appearance,
            Self::About => catalogue::Section::About,
        }
    }

    /// The label shown in the section list and the commands page.
    fn label(self) -> &'static str {
        self.section().label()
    }

    fn icon(self) -> IconName {
        self.section().icon()
    }

    /// The section `delta` steps away, clamped to the last one.
    fn shifted(self, delta: isize) -> Self {
        let index = Self::ALL
            .iter()
            .position(|tab| *tab == self)
            .unwrap_or_default() as isize;
        Self::ALL[(index + delta).clamp(0, Self::ALL.len() as isize - 1) as usize]
    }
}

struct Launcher {
    list: Entity<ListState<ToolDelegate>>,
    search: Entity<InputState>,
    /// The committed search text, mirrored from the field to drive the list.
    /// Text an IME is still composing is not here yet, which is why the bar
    /// sizes itself from the field's own value instead (see `header`).
    query: String,
    /// The page on show, which the header, body and footer all follow.
    page: Page,
    /// The settings section the panel last stood on, and when it left it. A
    /// summon inside the stored retention window reopens that section instead
    /// of the command list.
    last_section: Option<(Instant, SettingsTab)>,
    /// The loop that hides the panel once another window takes the foreground.
    /// Restarted on every summon; the previous one has already ended by then.
    focus_watch: Option<Task<()>>,
    /// Which sub-module the commands page is showing, as an index into
    /// `catalogue::Section::ALL`.
    section: usize,
    /// The command whose shortcut is being captured, if any.
    recording: Option<&'static str>,
    /// Kept alive for as long as the launcher is: dropping either would stop
    /// the field from reporting what the user typed.
    _search_events: Subscription,
    _search_observation: Subscription,
}

impl Launcher {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let states = command_states(&stored_settings(cx));
        let list = cx.new(|cx| ListState::new(ToolDelegate::new(states), window, cx));
        let view = cx.weak_entity();
        list.update(cx, |state, _cx| {
            state.delegate_mut().view = Some(view);
        });

        let search = cx.new(|cx| InputState::new(window, cx));
        // The list's own search box cannot host the bar's icon, its drag area or
        // the settings button, so the launcher owns the field and pushes the
        // text into the list instead.
        let search_events = cx.subscribe(&search, |launcher, search, event, cx| {
            if matches!(event, InputEvent::Change) {
                let query = search.read(cx).value().to_string();
                launcher.search_changed(query, cx);
            }
        });
        // A change event only fires once an IME composition is committed, but
        // the field already holds the composing text while it is in flight, and
        // the bar has to grow with it. The state notifies on every such update,
        // so follow it and redraw whenever the two texts disagree.
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
        // GPUI focuses nothing by default, so the launcher would swallow every
        // keystroke until the user clicked the field.
        launcher.focus_search(window, cx);
        launcher.select_row(0, window, cx);
        launcher
    }

    /// Focus the search field, so keystrokes reach it.
    fn focus_search(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.search.read(cx).focus_handle(cx).focus(window, cx);
    }

    /// Highlight a row without scrolling the list.
    fn select_row(&self, row: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.list.update(cx, |state, cx| {
            state.set_selected_index(Some(IndexPath::new(row)), window, cx);
        });
    }

    /// Take the field's text as the new query, which filters the list.
    fn search_changed(&mut self, query: String, cx: &mut Context<Self>) {
        if self.query == query {
            return;
        }
        self.query = query;
        // The bar is redrawn from this text, so it must not wait for the list.
        cx.notify();

        let value = self.query.trim().to_string();
        with_launcher(cx, move |launcher, window, cx| {
            launcher
                .list
                .update(cx, |list, cx| list.set_query(&value, window, cx));
            cx.notify();
        });
    }

    /// Up and down: the highlighted command, or the settings section.
    fn navigate(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        match self.page {
            Page::Commands => self.move_selection(delta, window, cx),
            Page::Settings(tab) => self.select_tab(tab.shifted(delta), cx),
        }
    }

    /// Move the highlight by `delta` rows, clamped to the visible commands.
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

    /// Start a fresh session: query cleared, first row highlighted, and — if
    /// the user came back inside the retention window — the settings section
    /// they were last on.
    fn reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    /// Hide the panel once another window takes the foreground, the way a
    /// launcher is expected to behave.
    ///
    /// GPUI reports no window-activation event, and the platform keeps that
    /// state to itself, so the panel samples the foreground window instead. The
    /// loop ends as soon as the panel hides, and the next summon starts a fresh
    /// one; the panel must have been in front at least once, so a window the
    /// shell refused to raise is not hidden before it is ever seen.
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

    /// Leave the panel because the user moved to another window.
    fn leave_on_focus_loss(&mut self, cx: &mut Context<Self>) {
        hide_launcher(cx);
    }

    /// The settings section a summon should reopen, if the panel left it
    /// recently enough to still be on the user's mind.
    fn resumed_tab(&self, cx: &Context<Self>) -> Option<SettingsTab> {
        let (at, tab) = self.last_section?;
        let window = stored_settings(cx).resume_seconds;
        (window > 0 && at.elapsed() < Duration::from_secs(window.into())).then_some(tab)
    }

    /// Note the section the panel is leaving, so `reset` can come back to it.
    ///
    /// Both ways out count: hiding the panel and stepping back to the command
    /// list, because Escape steps back before it hides.
    fn remember_section(&mut self) {
        if let Page::Settings(tab) = self.page {
            self.last_section = Some((Instant::now(), tab));
        }
    }

    /// Run the highlighted command.
    fn activate_selected(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.page, Page::Commands) {
            return;
        }
        if let Some(command) = self.list.read(cx).delegate().selected_command() {
            run_command(command, cx);
        }
    }

    /// Esc steps back out of settings before it hides the panel.
    fn escape(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.page, Page::Settings(_)) {
            self.back_to_commands(window, cx);
        } else {
            hide_launcher(cx);
        }
    }

    /// Show the settings page in place of the command list.
    fn open_settings(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.page = Page::Settings(SettingsTab::System);
        cx.notify();
    }

    /// Return to the command list, with the field ready to type into.
    fn back_to_commands(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.remember_section();
        self.page = Page::Commands;
        self.focus_search(window, cx);
        cx.notify();
    }

    /// Show another settings section.
    fn select_tab(&mut self, tab: SettingsTab, cx: &mut Context<Self>) {
        if self.page == Page::Settings(tab) {
            return;
        }
        self.page = Page::Settings(tab);
        cx.notify();
    }

    /// Switch colour scheme, in every window, and remember the choice.
    fn choose_theme(
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

    /// Resize the search field, and with it the header that holds it.
    fn set_search_height(&mut self, height: u32, cx: &mut Context<Self>) {
        let height = height.clamp(Settings::SEARCH_HEIGHT_MIN, Settings::SEARCH_HEIGHT_MAX);
        let mut settings = stored_settings(cx);
        if settings.search_height == height {
            return;
        }
        settings.search_height = height;
        store_settings(settings, cx);
        cx.notify();
    }

    /// Choose how long a hidden panel remembers its settings section.
    fn set_resume_seconds(&mut self, seconds: u32, cx: &mut Context<Self>) {
        let mut settings = stored_settings(cx);
        if settings.resume_seconds == seconds {
            return;
        }
        settings.resume_seconds = seconds;
        store_settings(settings, cx);
        cx.notify();
    }

    /// Add or remove the login startup entry.
    fn set_launch_at_login(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if !autostart::set_enabled(enabled) {
            // The switch is a controlled value, so a failed write only leaves
            // it where it was: say so rather than pretending it took.
            eprintln!("gpui-tools: could not update the login startup entry");
            cx.notify();
            return;
        }
        let mut settings = stored_settings(cx);
        settings.launch_at_login = enabled;
        store_settings(settings, cx);
        cx.notify();
    }

    /// Re-read what the settings say about every command.
    fn refresh_catalogue(&mut self, cx: &mut Context<Self>) {
        let states = command_states(&stored_settings(cx));
        self.list.update(cx, |list, cx| {
            list.delegate_mut().states = states;
            cx.notify();
        });
    }

    /// Show another sub-module on the commands page.
    fn select_section(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.section == index {
            return;
        }
        self.section = index;
        self.recording = None;
        cx.notify();
    }

    /// Take a command off the home list, or put it back.
    fn set_command_enabled(&mut self, id: &'static str, enabled: bool, cx: &mut Context<Self>) {
        let mut settings = stored_settings(cx);
        if settings.command_enabled(id) == enabled {
            return;
        }
        settings.set_command_enabled(id, enabled);
        store_settings(settings, cx);
        self.refresh_catalogue(cx);
        cx.notify();
    }

    /// Start listening for the chord that should run `id`, or stop listening.
    fn toggle_recording(&mut self, id: &'static str, cx: &mut Context<Self>) {
        self.recording = if self.recording == Some(id) {
            None
        } else {
            Some(id)
        };
        cx.notify();
    }

    /// Take the next keystroke as the shortcut for the command being recorded.
    ///
    /// Only chords with a modifier are accepted: a bare letter would be captured
    /// from the panel itself and is useless as a global shortcut.
    fn capture_shortcut(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) {
        let Some(id) = self.recording else {
            return;
        };
        match keystroke.key.as_str() {
            "escape" => self.recording = None,
            "backspace" | "delete" => {
                self.set_shortcut(id, None, cx);
                self.recording = None;
            }
            _ => {
                let Some(chord) = chord_from_keystroke(keystroke) else {
                    // Nothing usable: keep listening.
                    return;
                };
                self.set_shortcut(id, Some(chord), cx);
                self.recording = None;
            }
        }
        cx.notify();
    }

    /// Bind a command to a chord, or clear its binding.
    fn set_shortcut(&mut self, id: &'static str, chord: Option<String>, cx: &mut Context<Self>) {
        let mut settings = stored_settings(cx);
        if settings.command_shortcut(id).map(str::to_string) == chord {
            return;
        }
        settings.set_command_shortcut(id, chord);
        store_settings(settings, cx);
        apply_command_shortcuts(cx);
        self.refresh_catalogue(cx);
        cx.notify();
    }

    /// The panel's header, which is also its drag surface.
    ///
    /// On the command page it holds the search field and the way into settings;
    /// on the settings page it shows the way back instead. Either way only the
    /// controls that need the mouse themselves are excluded from the drag
    /// region: the field is exactly as wide as the text in it, so an empty bar
    /// is free to drag the panel by, hint included.
    fn header(&self, window: &Window, cx: &Context<Self>) -> AnyElement {
        let (muted_fg, border) = {
            let theme = cx.theme();
            (theme.muted_foreground, theme.border)
        };
        // Size the bar from the field's own text, not from the committed query:
        // while an IME composition is in flight the buffer already holds text
        // the query knows nothing about, and a bar sized to the query would
        // scroll that text out of view to the left.
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
            .border_color(border);

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
                            // Small, not the default medium: the medium input
                            // spends 16px of its height on its own vertical
                            // padding, which is more than the line box has
                            // left, and the bottom of every glyph gets cut.
                            Input::new(&self.search)
                                .small()
                                .appearance(false)
                                .cleanable(false),
                        ),
                )
                .when(field_text.is_empty(), |this| {
                    // The hint belongs to the drag surface, not to the field: it
                    // is there to be read while the bar is dragged by.
                    this.child(div().text_sm().text_color(muted_fg).child(SEARCH_HINT))
                })
                .child(div().flex_1())
                .child(
                    div().id("settings-button").occlude().flex_shrink_0().child(
                        Button::new("settings")
                            .ghost()
                            .icon(IconName::Settings)
                            .tooltip("设置")
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
                            .tooltip("返回")
                            .on_click(cx.listener(|launcher, _, window, cx| {
                                launcher.back_to_commands(window, cx)
                            })),
                    ),
                )
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .child("设置"),
                )
                .child(div().flex_1())
                .into_any_element(),
        }
    }

    /// The settings page: the sections on the left, the chosen one on the right.
    fn settings_page(&self, tab: SettingsTab, cx: &Context<Self>) -> impl IntoElement {
        let (border, background, muted, muted_fg, fg, accent, accent_fg) = {
            let theme = cx.theme();
            (
                theme.border,
                theme.background,
                theme.muted,
                theme.muted_foreground,
                theme.foreground,
                theme.accent,
                theme.accent_foreground,
            )
        };
        let colors = RowColors {
            surface: muted,
            control: background,
            foreground: fg,
            muted_foreground: muted_fg,
            accent,
            accent_foreground: accent_fg,
        };
        let settings = stored_settings(cx);

        let sections = v_flex()
            .w(px(SIDEBAR_WIDTH))
            .flex_shrink_0()
            .p_2()
            .gap_1()
            .border_r_1()
            .border_color(border)
            .children(SettingsTab::ALL.into_iter().enumerate().map(|(ix, item)| {
                let selected = item == tab;
                h_flex()
                    .id(("section", ix))
                    .gap_2()
                    .px_3()
                    .py_2()
                    .rounded(px(6.))
                    .text_sm()
                    .text_color(if selected { accent_fg } else { fg })
                    .cursor_pointer()
                    .when(selected, move |this| this.bg(accent))
                    .when(!selected, move |this| {
                        this.hover(move |style| style.bg(muted_fg.opacity(0.18)))
                    })
                    .on_click(cx.listener(move |launcher, _, _, cx| launcher.select_tab(item, cx)))
                    .child(Icon::new(item.icon()).size_4().text_color(if selected {
                        accent_fg
                    } else {
                        muted_fg
                    }))
                    .child(div().child(item.label()))
            }));

        let content = match tab {
            SettingsTab::System => v_flex()
                .gap_1()
                .child(settings_row(
                    "全局快捷键",
                    "在任意窗口唤出面板",
                    colors,
                    value_label(HOTKEY_LABEL, colors),
                ))
                .child(settings_row(
                    "开机启动",
                    "登录 Windows 后自动在后台运行",
                    colors,
                    Switch::new("launch-at-login")
                        .checked(settings.launch_at_login)
                        .on_change(cx.listener(|launcher, checked: &bool, _window, cx| {
                            launcher.set_launch_at_login(*checked, cx)
                        })),
                ))
                .child(settings_row(
                    "子页面保持",
                    "隐藏后短时间内再唤出，仍停在原页面",
                    colors,
                    segmented(
                        "resume",
                        Settings::RESUME_CHOICES,
                        settings.resume_seconds,
                        colors,
                        cx,
                        |launcher, seconds, cx| launcher.set_resume_seconds(seconds, cx),
                    ),
                ))
                .child(action_row(
                    "open-config",
                    "配置文件夹",
                    settings_directory(),
                    "打开",
                    colors,
                    cx.listener(|_, _, _, cx| open_settings_folder(cx)),
                ))
                .child(action_row(
                    "quit",
                    "退出启动器",
                    "关闭面板与托盘",
                    "退出",
                    colors,
                    cx.listener(|_, _, _, cx| quit_launcher(cx)),
                ))
                .into_any_element(),
            SettingsTab::Commands => self.commands_page(colors, cx).into_any_element(),
            SettingsTab::Appearance => v_flex()
                .gap_1()
                .child(settings_row(
                    "主题",
                    "面板、列表与设置页的配色",
                    colors,
                    segmented(
                        "theme",
                        ColorScheme::ALL
                            .into_iter()
                            .map(|theme| (theme, theme.label())),
                        settings.theme,
                        colors,
                        cx,
                        |launcher, choice, cx| launcher.choose_theme(choice, None, cx),
                    ),
                ))
                .child(settings_row(
                    "搜索框高度",
                    "输入框的高度，越高越好点中",
                    colors,
                    stepper(
                        "search-height",
                        settings.search_height,
                        colors,
                        cx,
                        |launcher, height, cx| launcher.set_search_height(height, cx),
                    ),
                ))
                .into_any_element(),
            SettingsTab::About => v_flex()
                .gap_1()
                .child(settings_row(
                    "版本",
                    "当前运行的程序版本",
                    colors,
                    value_label(env!("CARGO_PKG_VERSION"), colors),
                ))
                .child(action_row(
                    "open-repository",
                    "项目仓库",
                    REPOSITORY_URL,
                    "打开",
                    colors,
                    cx.listener(|_, _, _, cx| open_repository(cx)),
                ))
                .child(action_row(
                    "open-folder",
                    "程序文件夹",
                    program_dir().display().to_string(),
                    "打开",
                    colors,
                    cx.listener(|_, _, _, cx| open_program_folder(cx)),
                ))
                .into_any_element(),
        };

        // `h_flex` centers on the cross axis by default, which would float the
        // page in the middle of the panel; the sections belong at the top.
        h_flex()
            .flex_1()
            .min_w_0()
            .items_stretch()
            .overflow_hidden()
            .child(sections)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .px_4()
                    .py_4()
                    .child(content),
            )
    }

    /// The commands page: the settings sub-modules on the left, the commands of
    /// the chosen one on the right.
    ///
    /// Everything on it comes from the catalogue and the per-command settings,
    /// so a command declared against a section shows up here without touching
    /// this function.
    fn commands_page(&self, colors: RowColors, cx: &Context<Self>) -> impl IntoElement {
        let settings = stored_settings(cx);
        let selected = self.section.min(catalogue::Section::ALL.len() - 1);
        let border = cx.theme().border;

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
                            .filter(|command| settings.command_enabled(command.id))
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
                                this.hover(move |style| {
                                    style.bg(colors.muted_foreground.opacity(0.18))
                                })
                            })
                            .on_click(cx.listener(move |launcher, _, _, cx| {
                                launcher.select_section(ix, cx)
                            }))
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
                                        colors.accent_foreground.opacity(0.7)
                                    } else {
                                        colors.muted_foreground
                                    })
                                    .child(format!("{shown}/{}", owned.len())),
                            )
                    }),
            );

        let section = catalogue::Section::ALL[selected];
        let commands = catalogue::commands(section)
            .map(|command| {
                command_row(
                    command,
                    &settings.command(command.id),
                    self.recording == Some(command.id),
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
                            .child(format!("{}的命令", section.label())),
                    )
                    .when(commands.is_empty(), |this| {
                        this.child(
                            div()
                                .px_2()
                                .py_2()
                                .text_xs()
                                .text_color(colors.muted_foreground)
                                .child("这个子模块还没有命令"),
                        )
                    })
                    .children(commands),
            )
    }
}

/// One command on the commands page: its face, its counter, its shortcut and
/// whether the home list shows it.
fn command_row(
    command: &'static catalogue::Command,
    state: &gpui_tools::settings::CommandSettings,
    recording: bool,
    colors: RowColors,
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
            .child("按下组合键\u{2026}")
            .into_any_element()
    } else {
        let label = match state.shortcut.as_deref() {
            Some(chord) => shortcut_label(chord),
            None => "快捷键".to_string(),
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
            .hover(move |style| style.bg(colors.muted_foreground.opacity(0.18)))
            .on_click(
                cx.listener(move |launcher, _, _, cx| launcher.toggle_recording(command.id, cx)),
            )
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
                .child(format!("{} 次", state.uses)),
        )
        .child(div().flex_shrink_0().child(shortcut))
        .child(
            div().flex_shrink_0().child(
                Switch::new(SharedString::from(format!("enabled-{}", command.id)))
                    .checked(state.enabled)
                    .on_change(cx.listener(move |launcher, checked: &bool, _window, cx| {
                        launcher.set_command_enabled(command.id, *checked, cx)
                    })),
            ),
        )
}

/// The folder the settings file lives in, for the row that opens it.
fn settings_directory() -> String {
    Settings::directory().map_or_else(
        || "未找到配置目录".to_string(),
        |dir| dir.display().to_string(),
    )
}

/// Open the folder that holds the settings file.
fn open_settings_folder(_cx: &mut App) {
    let Some(dir) = Settings::directory() else {
        eprintln!("gpui-tools: no settings directory to open");
        return;
    };
    if !dir.is_dir() {
        eprintln!("gpui-tools: {} has not been created yet", dir.display());
        return;
    }
    open_folder(&dir);
}

/// How wide the search field must be to show `query` without scrolling.
///
/// The field is sized to its text rather than to the bar, because every pixel
/// it claims is a pixel the panel cannot be dragged by.
fn search_field_width(window: &Window, cx: &Context<Launcher>, query: &str) -> Pixels {
    let padding = px(SEARCH_FIELD_PADDING);
    if query.is_empty() {
        return padding + px(SEARCH_FIELD_CARET);
    }

    let theme = cx.theme();
    let run = TextRun {
        len: query.len(),
        font: Font {
            family: theme.font_family.clone(),
            ..Default::default()
        },
        color: theme.foreground,
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
        let (bg, fg, border, muted, muted_fg) = {
            let theme = cx.theme();
            (
                theme.background,
                theme.foreground,
                theme.border,
                theme.muted,
                theme.muted_foreground,
            )
        };
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
                    ("\u{2191}\u{2193}", "Navigate"),
                    ("Enter", "Open"),
                    ("Esc", "Close"),
                ],
                format!("{count} commands"),
            ),
            Page::Settings(_) => (
                vec![("\u{2191}\u{2193}", "Sections"), ("Esc", "Back")],
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
            Page::Settings(tab) => self.settings_page(tab, cx).into_any_element(),
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

/// The colours a settings row draws with.
#[derive(Clone, Copy)]
struct RowColors {
    /// The row's own backing, which separates it from the page.
    surface: Hsla,
    /// The backing of a control inside the row, so it reads as one piece.
    control: Hsla,
    foreground: Hsla,
    muted_foreground: Hsla,
    accent: Hsla,
    accent_foreground: Hsla,
}

/// A settings row: what the setting is on the left, its control on the right.
///
/// Every setting is a row of this shape, so the name and its description always
/// sit in the same place whatever the control beside them looks like.
fn settings_row(
    name: &'static str,
    description: impl Into<SharedString>,
    colors: RowColors,
    control: impl IntoElement,
) -> Div {
    h_flex()
        .w_full()
        .min_w_0()
        .justify_between()
        .items_center()
        .gap_4()
        .px_3()
        .py_2()
        .rounded(px(6.))
        .bg(colors.surface)
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .child(div().text_sm().text_color(colors.foreground).child(name))
                .child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .text_ellipsis()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(description.into()),
                ),
        )
        .child(div().flex_shrink_0().child(control))
}

/// The text a read-only row shows in place of a control.
fn value_label(value: impl Into<SharedString>, colors: RowColors) -> impl IntoElement {
    div()
        .text_sm()
        .text_color(colors.muted_foreground)
        .child(value.into())
}

/// A settings row that runs something when clicked.
fn action_row(
    id: &'static str,
    name: &'static str,
    description: impl Into<SharedString>,
    hint: &'static str,
    colors: RowColors,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    settings_row(
        name,
        description,
        colors,
        h_flex()
            .gap_1()
            .child(
                div()
                    .text_sm()
                    .text_color(colors.muted_foreground)
                    .child(hint),
            )
            .child(
                Icon::new(IconName::ChevronRight)
                    .size_4()
                    .text_color(colors.muted_foreground),
            ),
    )
    .id(id)
    .cursor_pointer()
    .hover(move |style| style.bg(colors.accent.opacity(0.18)))
    .on_click(on_click)
}

/// A row control that picks one of a few values, drawn as a pill of chips.
fn segmented<T: Copy + PartialEq + 'static>(
    id: &'static str,
    options: impl IntoIterator<Item = (T, &'static str)>,
    selected: T,
    colors: RowColors,
    cx: &Context<Launcher>,
    on_select: impl Fn(&mut Launcher, T, &mut Context<Launcher>) + Copy + 'static,
) -> impl IntoElement {
    h_flex()
        .gap_0p5()
        .p_0p5()
        .rounded(px(8.))
        .bg(colors.control)
        .children(
            options
                .into_iter()
                .enumerate()
                .map(move |(ix, (value, label))| {
                    let current = value == selected;
                    div()
                        .id((id, ix))
                        .px_2()
                        .py_1()
                        .rounded(px(6.))
                        .text_sm()
                        .text_color(if current {
                            colors.accent_foreground
                        } else {
                            colors.muted_foreground
                        })
                        .cursor_pointer()
                        .when(current, move |this| this.bg(colors.accent))
                        .when(!current, move |this| {
                            this.hover(move |style| style.bg(colors.muted_foreground.opacity(0.18)))
                        })
                        .on_click(cx.listener(move |launcher, _: &ClickEvent, _window, cx| {
                            on_select(launcher, value, cx)
                        }))
                        .child(label)
                }),
        )
}

/// A row control that steps a number, drawn as minus / value / plus.
fn stepper(
    id: &'static str,
    value: u32,
    colors: RowColors,
    cx: &Context<Launcher>,
    on_change: impl Fn(&mut Launcher, u32, &mut Context<Launcher>) + Copy + 'static,
) -> impl IntoElement {
    let lowest = Settings::SEARCH_HEIGHT_MIN;
    let highest = Settings::SEARCH_HEIGHT_MAX;
    let step = Settings::SEARCH_HEIGHT_STEP as isize;
    let at_bottom = value <= lowest;
    let at_top = value >= highest;

    let button = move |ix: usize, icon: IconName, disabled: bool| {
        let button = div()
            .id((id, ix))
            .size_6()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(6.))
            .text_color(colors.foreground);
        if disabled {
            return button.opacity(0.3);
        }
        let delta = if ix == 0 { -step } else { step };
        let next = (value as isize + delta).clamp(lowest as isize, highest as isize) as u32;
        button
            .cursor_pointer()
            .hover(move |style| style.bg(colors.muted_foreground.opacity(0.18)))
            .on_click(cx.listener(move |launcher, _: &ClickEvent, _window, cx| {
                on_change(launcher, next, cx)
            }))
            .child(Icon::new(icon).size_4())
    };

    h_flex()
        .gap_1()
        .p_0p5()
        .rounded(px(8.))
        .bg(colors.control)
        .child(button(0, IconName::Minus, at_bottom))
        .child(
            div()
                .w(px(64.))
                .text_center()
                .text_sm()
                .text_color(colors.foreground)
                .child(format!("{value} px")),
        )
        .child(button(1, IconName::Plus, at_top))
}

/// The height the header needs to hold a field `height` logical pixels tall
/// with the same room above and below as the default.
fn header_height(settings: Settings) -> Pixels {
    let needed = settings.search_height as f32 + HEADER_SLACK;
    px(needed.max(HEADER_MIN_HEIGHT))
}

fn launch() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .with_quit_mode(QuitMode::Explicit)
        .run(|cx: &mut App| {
            gpui_kit::init(cx);

            let mut settings = Settings::load();
            // The registry, not the settings file, decides whether the app
            // starts at login: the user can remove the entry from Task Manager
            // without the host noticing.
            settings.launch_at_login = autostart::enabled();
            cx.set_global(SettingsStore(settings.clone()));

            let bounds = Bounds::centered(None, size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx);
            let (window_handle, launcher) = gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    show: false,
                    focus: false,
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    apply_theme(settings.theme, Some(window), cx);
                    window.set_window_title(WINDOW_TITLE);
                    window.on_window_should_close(cx, |_, cx| {
                        hide_launcher(cx);
                        false
                    });
                    cx.new(|cx| Launcher::new(window, cx))
                },
            )
            .expect("launcher window opens");

            // The key interceptor and the action handlers only receive
            // `&mut App`, so they need a way back to this view and its window.
            cx.set_global(LauncherHandle {
                view: launcher.downgrade(),
            });
            cx.set_global(LauncherWindow(window_handle));

            // GPUI does not size or place its windows, so the panel asks for the
            // frameless chrome itself. A panel that could not be dressed still
            // works, so a failure is reported rather than fatal.
            match NativeWindow::find(WINDOW_TITLE) {
                Some(window) => {
                    if !window.apply_panel_chrome(WINDOW_WIDTH, WINDOW_HEIGHT) {
                        eprintln!("gpui-tools: could not apply the panel chrome in full");
                    }
                    // The task bar and Alt-Tab take their icon from the window,
                    // which GPUI leaves unset; the icon resource is the answer.
                    if !window.set_icon(APP_ICON_ID) {
                        eprintln!("gpui-tools: could not adopt the window icon");
                    }
                    // The horizontal position is deliberately whatever centring
                    // produced; only the remembered top edge is honoured.
                    if let Some(y) = settings.window_y
                        && !window.center_x_at(y)
                    {
                        eprintln!("gpui-tools: could not restore the panel position");
                    }
                    cx.set_global(LauncherPanel { window });
                }
                None => eprintln!(
                    "gpui-tools: launcher window not found after creation; keeping the native frame"
                ),
            }

            cx.on_action(show_launcher_action)
                .on_action(quit_application_action);

            // The panel's keys are claimed before keymap resolution: the field
            // would otherwise swallow Enter, and the list — which is no longer
            // the focused view — would never see the arrows or Escape.
            let interceptor = cx.intercept_keystrokes(|event, window, cx| {
                if !owns_window(window, cx) {
                    return;
                }
                // Capturing a shortcut outranks every other key: the next chord
                // is the answer to the row that is waiting for one.
                if recording_command(cx).is_some() {
                    let keystroke = event.keystroke.clone();
                    with_launcher(cx, move |launcher, _window, cx| {
                        launcher.capture_shortcut(&keystroke, cx)
                    });
                    cx.stop_propagation();
                    return;
                }
                match event.keystroke.key.as_str() {
                    "enter" => {
                        with_launcher(cx, |launcher, _window, cx| launcher.activate_selected(cx));
                    }
                    "up" => {
                        with_launcher(cx, |launcher, window, cx| launcher.navigate(-1, window, cx));
                        cx.stop_propagation();
                    }
                    "down" => {
                        with_launcher(cx, |launcher, window, cx| launcher.navigate(1, window, cx));
                        cx.stop_propagation();
                    }
                    "escape" => {
                        with_launcher(cx, |launcher, window, cx| launcher.escape(window, cx));
                        cx.stop_propagation();
                    }
                    _ => {}
                }
            });
            cx.set_global(KeyInterceptor {
                _subscription: interceptor,
            });

            match Tray::builder()
                .icon(launcher_tray_icon())
                .title("GPUI Tools")
                .tooltip("GPUI Tools")
                .menu(|_| {
                    vec![
                        gpui_kit::MenuItem::action("Open launcher", ShowLauncher),
                        gpui_kit::MenuItem::separator(),
                        gpui_kit::MenuItem::action("Quit", QuitApplication),
                    ]
                })
                .build(cx)
            {
                Ok(tray) => cx.set_global(TrayRegistration { _tray: tray }),
                Err(error) => eprintln!("Could not create system tray icon: {error}"),
            }

            if let Ok(manager) = GlobalHotKeyManager::new() {
                use global_hotkey::{
                    GlobalHotKeyEvent, HotKeyState,
                    hotkey::{Code, HotKey, Modifiers},
                };
                let summon = HotKey::new(Some(Modifiers::CONTROL), Code::Space);
                if manager.register(summon).is_ok() {
                    let summon_id = summon.id();
                    cx.set_global(Shortcuts {
                        manager,
                        commands: RefCell::new(Vec::new()),
                    });
                    // Whatever the settings already bind, registered before the
                    // loop starts so the first press is not missed.
                    apply_command_shortcuts(cx);

                    cx.spawn(async move |cx: &mut gpui_kit::AsyncApp| {
                        loop {
                            cx.background_executor()
                                .timer(Duration::from_millis(80))
                                .await;
                            while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
                                if event.state != HotKeyState::Pressed {
                                    continue;
                                }
                                if event.id == summon_id {
                                    cx.update(show_launcher);
                                    continue;
                                }
                                cx.update(|cx| {
                                    let command =
                                        cx.try_global::<Shortcuts>().and_then(|shortcuts| {
                                            shortcuts
                                                .commands
                                                .borrow()
                                                .iter()
                                                .find(|(hotkey, _)| hotkey.id() == event.id)
                                                .map(|(_, id)| *id)
                                        });
                                    if let Some(command) = command.and_then(catalogue::find) {
                                        run_command(command, cx);
                                    }
                                });
                            }
                        }
                    })
                    .detach();
                } else {
                    eprintln!(
                        "Could not register {HOTKEY_LABEL}; another application may already own it."
                    );
                }
            } else {
                eprintln!("Could not initialize the global hotkey manager.");
            }
        });
}

actions!(gpui_tools, [ShowLauncher, QuitApplication]);

/// The command whose shortcut is being captured, read straight from the panel.
///
/// The key interceptor has to decide synchronously whether to swallow a key, so
/// it cannot go through the deferred hop the rest of the callbacks use.
fn recording_command(cx: &App) -> Option<&'static str> {
    let handle = cx.try_global::<LauncherHandle>()?;
    handle
        .view
        .read_with(cx, |launcher, _| launcher.recording)
        .unwrap_or(None)
}

fn launcher_tray_icon() -> TrayIcon {
    // The tray wants its own resolution: Windows hands the shell a bitmap and
    // scales it to the task bar's icon metric.
    const SIZE: u32 = 32;
    TrayIcon::from_rgba(logo::rgba(SIZE), SIZE, SIZE).expect("valid launcher tray icon")
}

fn show_launcher_action(_: &ShowLauncher, cx: &mut App) {
    show_launcher(cx);
}

fn quit_application_action(_: &QuitApplication, cx: &mut App) {
    remember_window_position(cx);
    cx.quit();
}

/// Whether `window` is the launcher panel, rather than the settings window.
///
/// The key interceptor hears every window in the application, so it has to ask
/// before acting on a keystroke that was meant for another one.
fn owns_window(window: &Window, cx: &App) -> bool {
    cx.try_global::<LauncherWindow>()
        .is_some_and(|handle| handle.0 == window.window_handle())
}

/// Run `action` against the launcher view, deferred past the current effect
/// cycle.
///
/// GPUI only resolves a window for a window's *root* entity, so a callback
/// holding nothing but `&mut App` reaches the view through the handles stored
/// at startup. The deferral keeps the window update out of key dispatch, where
/// re-entering the window would panic.
fn with_launcher(
    cx: &mut App,
    action: impl FnOnce(&mut Launcher, &mut Window, &mut Context<Launcher>) + 'static,
) {
    cx.defer(move |cx| {
        let Some(view) = cx
            .try_global::<LauncherHandle>()
            .map(|handle| handle.view.clone())
        else {
            return;
        };
        let Some(window) = cx.try_global::<LauncherWindow>().map(|handle| handle.0) else {
            return;
        };
        // Never silently: a failed hop here looks exactly like a dead keyboard.
        if let Err(error) = window.update(cx, move |_root, window, cx| {
            if let Err(error) = view.update(cx, move |launcher, launcher_cx| {
                action(launcher, window, launcher_cx);
            }) {
                eprintln!("gpui-tools: launcher view is gone: {error:?}");
            }
        }) {
            eprintln!("gpui-tools: launcher window is unreachable: {error:?}");
        }
    });
}

/// Show and raise the launcher panel, always starting from a clean session.
fn show_launcher(cx: &mut App) {
    with_launcher(cx, |launcher, window, cx| launcher.reset(window, cx));
    if let Some(panel) = cx.try_global::<LauncherPanel>() {
        panel.window.show();
        panel.window.focus();
    }
    // Windows may refuse the foreground change, and GPUI tracks activation
    // separately, so ask for it as well.
    cx.activate(true);
}

/// Hide the launcher panel without destroying it.
fn hide_launcher(cx: &mut App) {
    // Note the page being left before the window goes: a summon inside the
    // retention window should come back to it.
    with_launcher(cx, |launcher, _window, _cx| launcher.remember_section());
    if let Some(panel) = cx.try_global::<LauncherPanel>() {
        panel.window.hide();
    }
    remember_window_position(cx);
}

/// Put the stored colour scheme in force in every window.
fn apply_theme(choice: ColorScheme, window: Option<&mut Window>, cx: &mut App) {
    match choice {
        ColorScheme::System => Theme::sync_system_appearance(window, cx),
        ColorScheme::Light => Theme::change(ThemeMode::Light, window, cx),
        ColorScheme::Dark => Theme::change(ThemeMode::Dark, window, cx),
    }
}

/// The settings currently in force.
fn stored_settings(cx: &App) -> Settings {
    cx.try_global::<SettingsStore>()
        .map_or_else(Settings::default, |store| store.0.clone())
}

/// Store the settings in memory and on disk.
fn store_settings(settings: Settings, cx: &mut App) {
    if !settings.save() {
        eprintln!("gpui-tools: could not store the settings");
    }
    cx.set_global(SettingsStore(settings));
}

/// Remember where the panel sits, so the next run opens there.
///
/// Only the top edge is kept: the horizontal position is re-derived by
/// centring, so the panel never reappears against one edge of the screen.
///
/// This is also where the usage counters reach the file: they are kept in
/// memory while the panel is up, and hiding is the moment the record is
/// settled.
fn remember_window_position(cx: &mut App) {
    let mut settings = stored_settings(cx);
    if let Some(panel) = cx.try_global::<LauncherPanel>() {
        settings.window_y = Some(panel.window.position().1);
    }
    store_settings(settings, cx);
}

fn open_program_folder(_cx: &mut App) {
    open_folder(&program_dir());
}

/// Show a folder in whatever file manager the user has made the default.
///
/// The shell resolves the association, so naming a program here — Explorer
/// included — would ignore a different default. Opening something else is the
/// user leaving the launcher, so the panel hides behind it like any other
/// focus loss.
fn open_folder(dir: &std::path::Path) {
    shell::open(dir);
}

/// Open the project page in the user's default browser.
fn open_repository(_cx: &mut App) {
    shell::open(REPOSITORY_URL);
}

fn quit_launcher(cx: &mut App) {
    // The panel is still up, so this is the last chance to note where it sits.
    remember_window_position(cx);
    cx.quit();
}

/// The settings as they were read at startup, so every window agrees on them.
struct SettingsStore(Settings);
impl gpui_kit::Global for SettingsStore {}

/// The launcher's native window, kept so every caller shares one handle
/// instead of re-finding the window by title on each action.
#[derive(Clone, Copy)]
struct LauncherPanel {
    window: NativeWindow,
}
impl gpui_kit::Global for LauncherPanel {}

/// The launcher view, for the callbacks that only receive `&mut App`.
struct LauncherHandle {
    view: WeakEntity<Launcher>,
}
impl gpui_kit::Global for LauncherHandle {}

/// The launcher's GPUI window, so `&mut App` callbacks can reach its window.
struct LauncherWindow(AnyWindowHandle);
impl gpui_kit::Global for LauncherWindow {}

/// Keeps the key interceptor subscribed for the life of the application.
struct KeyInterceptor {
    _subscription: Subscription,
}
impl gpui_kit::Global for KeyInterceptor {}

struct TrayRegistration {
    _tray: Tray,
}
impl gpui_kit::Global for TrayRegistration {}

/// The global shortcuts this process holds.
///
/// The summon chord is registered once and never moves; command shortcuts are
/// rebuilt from the settings whenever the user changes one.
struct Shortcuts {
    manager: GlobalHotKeyManager,
    /// The commands currently bound, with the chord that runs each.
    commands: RefCell<Vec<(global_hotkey::hotkey::HotKey, &'static str)>>,
}
impl gpui_kit::Global for Shortcuts {}

/// Rebuild the registered command shortcuts from the settings.
///
/// Every binding is dropped and re-registered: the set is tiny, and a partial
/// update would have to track which chord belonged to which command after an
/// edit. A chord the system refuses is reported and skipped, so one collision
/// does not cost the others.
fn apply_command_shortcuts(cx: &mut App) {
    let settings = stored_settings(cx);
    let wanted = catalogue::COMMANDS
        .iter()
        .filter_map(|command| {
            let chord = settings.command_shortcut(command.id)?;
            match parse_chord(chord) {
                Some(hotkey) => Some((command.id, hotkey)),
                None => {
                    eprintln!("gpui-tools: {chord} is not a shortcut this launcher understands");
                    None
                }
            }
        })
        .collect::<Vec<_>>();

    let Some(shortcuts) = cx.try_global::<Shortcuts>() else {
        return;
    };
    let mut held = shortcuts.commands.borrow_mut();
    for (hotkey, _) in held.drain(..) {
        if let Err(error) = shortcuts.manager.unregister(hotkey) {
            eprintln!("gpui-tools: could not release a stale shortcut: {error}");
        }
    }
    for (id, hotkey) in wanted {
        match shortcuts.manager.register(hotkey) {
            Ok(()) => held.push((hotkey, id)),
            Err(error) => eprintln!("gpui-tools: could not bind the shortcut for {id}: {error}"),
        }
    }
}

/// Run a command and count the run.
///
/// The counter is kept in memory and reaches the file the next time the panel
/// hides or the application quits; a write on every keystroke-triggered command
/// would be the only disk traffic this program has.
fn run_command(command: &'static catalogue::Command, cx: &mut App) {
    let mut settings = stored_settings(cx);
    settings.count_use(command.id);
    cx.set_global(SettingsStore(settings));
    (command.action)(cx);
}

/// The chord a keystroke stands for, or nothing when it carries no modifier.
///
/// A chord without a modifier would be captured from the panel's own input and
/// could never be a global shortcut, so those are refused rather than stored.
fn chord_from_keystroke(keystroke: &Keystroke) -> Option<String> {
    let mut parts = Vec::new();
    if keystroke.modifiers.control {
        parts.push("ctrl".to_string());
    }
    if keystroke.modifiers.alt {
        parts.push("alt".to_string());
    }
    if keystroke.modifiers.shift {
        parts.push("shift".to_string());
    }
    if keystroke.modifiers.platform {
        parts.push("super".to_string());
    }
    if parts.is_empty() {
        return None;
    }
    parts.push(code_name(&keystroke.key)?);
    Some(parts.join("+"))
}

/// The `keyboard-types` name of the key a GPUI keystroke reports.
fn code_name(key: &str) -> Option<String> {
    let mut characters = key.chars();
    if let (Some(character), None) = (characters.next(), characters.next()) {
        if character.is_ascii_alphabetic() {
            return Some(format!("Key{}", character.to_ascii_uppercase()));
        }
        if character.is_ascii_digit() {
            return Some(format!("Digit{character}"));
        }
    }
    if let Some(number) = key.strip_prefix('f')
        && number.parse::<u8>().is_ok_and(|n| (1..=24).contains(&n))
    {
        return Some(format!("F{number}"));
    }
    let named = match key {
        "space" => "Space",
        "tab" => "Tab",
        "comma" => "Comma",
        "period" => "Period",
        "slash" => "Slash",
        "semicolon" => "Semicolon",
        "quote" => "Quote",
        "backquote" => "Backquote",
        "backslash" => "Backslash",
        "minus" => "Minus",
        "equal" => "Equal",
        "bracketleft" => "BracketLeft",
        "bracketright" => "BracketRight",
        "insert" => "Insert",
        "delete" => "Delete",
        "home" => "Home",
        "end" => "End",
        "pageup" => "PageUp",
        "pagedown" => "PageDown",
        "left" => "ArrowLeft",
        "right" => "ArrowRight",
        "up" => "ArrowUp",
        "down" => "ArrowDown",
        _ => return None,
    };
    Some(named.to_string())
}

/// Parse a stored chord into a shortcut the system can register.
fn parse_chord(chord: &str) -> Option<global_hotkey::hotkey::HotKey> {
    use global_hotkey::hotkey::{Code, HotKey, Modifiers};

    let mut modifiers = Modifiers::empty();
    let mut code = None;
    for part in chord.split('+') {
        match part.trim() {
            "ctrl" => modifiers |= Modifiers::CONTROL,
            "alt" => modifiers |= Modifiers::ALT,
            "shift" => modifiers |= Modifiers::SHIFT,
            "super" => modifiers |= Modifiers::SUPER,
            other => code = Some(other.parse::<Code>().ok()?),
        }
    }
    let code = code?;
    Some(HotKey::new(
        (!modifiers.is_empty()).then_some(modifiers),
        code,
    ))
}

/// How a chord reads to the user: `ctrl+alt+KeyP` becomes `Ctrl + Alt + P`.
fn shortcut_label(chord: &str) -> String {
    chord
        .split('+')
        .map(|part| match part {
            "ctrl" => "Ctrl".to_string(),
            "alt" => "Alt".to_string(),
            "shift" => "Shift".to_string(),
            "super" => "Win".to_string(),
            other => part_label(other),
        })
        .collect::<Vec<_>>()
        .join(" + ")
}

/// The face of one key: the letters and digits drop their `keyboard-types`
/// prefix, the arrows become the arrows they draw.
fn part_label(code: &str) -> String {
    if let Some(rest) = code.strip_prefix("Key") {
        return rest.to_string();
    }
    if let Some(rest) = code.strip_prefix("Digit") {
        return rest.to_string();
    }
    match code {
        "ArrowLeft" => "\u{2190}".to_string(),
        "ArrowRight" => "\u{2192}".to_string(),
        "ArrowUp" => "\u{2191}".to_string(),
        "ArrowDown" => "\u{2193}".to_string(),
        other => other.to_string(),
    }
}
