//! GPUI Tools desktop launcher.
//!
//! A hotkey-summoned, frameless panel showing a searchable list of the
//! launcher's own commands. Enter runs the highlighted one, Esc hides the
//! panel.
//!
//! The panel draws its own settings page rather than opening a second window,
//! and its header doubles as the drag surface. That is why the launcher owns the
//! text field instead of letting the list draw its own.
use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::list::{List, ListDelegate, ListState};
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, IndexPath, Selectable, Sizable as _, Theme, ThemeMode,
    h_flex, v_flex,
};
use gpui_kit::{
    AnyElement, AnyWindowHandle, App, Bounds, ClickEvent, Context, Div, Entity, Focusable, Font,
    FontWeight, Hsla, IntoElement, Pixels, QuitMode, Render, RenderOnce, Subscription, Task,
    TextRun, WeakEntity, Window, WindowBounds, WindowControlArea, WindowOptions, actions, div,
    prelude::*, px, size,
};
use gpui_tools::settings::{Settings, Theme as ColorScheme};
use gpui_tools::window::NativeWindow;
use gpui_tray::{Icon as TrayIcon, Tray};

const WINDOW_TITLE: &str = "GPUI Tools Launcher";
const WINDOW_WIDTH: f32 = 720.;
const WINDOW_HEIGHT: f32 = 464.;

const REPOSITORY_URL: &str = "https://github.com/cz-97/gpui-tools";

/// The shortcut that summons the panel, as the settings page shows it.
const HOTKEY_LABEL: &str = "Ctrl + Space";

/// The height of the panel's header, which is also its drag surface.
const HEADER_HEIGHT: f32 = 52.;
/// The width of the settings page's section list.
const SIDEBAR_WIDTH: f32 = 148.;
/// The search field's height: the component's small input plus a little slack,
/// so the line box fits without its descenders being clipped.
const SEARCH_FIELD_HEIGHT: f32 = 26.;
/// The padding the component draws inside the field, both sides together
/// (`input_px` is 8 for its small size). The field has to pay for it to keep
/// its text in view.
const SEARCH_FIELD_PADDING: f32 = 16.;
/// The room an empty field keeps for the caret. Anything wider would eat into
/// the drag surface for nothing.
const SEARCH_FIELD_CARET: f32 = 4.;
/// Slack added to a measured query, so a field that is a hair too narrow never
/// scrolls the text out of sight.
const SEARCH_FIELD_SLACK: f32 = 8.;
/// The widest the field may grow, leaving the icon, the hint and the settings
/// button their room.
const SEARCH_FIELD_MAX: f32 = 560.;
/// The size of the text the field draws, matching the component's medium input.
const SEARCH_TEXT_SIZE: f32 = 14.;
const SEARCH_HINT: &str = "搜索命令\u{2026}";

/// A command the launcher offers.
struct Tool {
    name: &'static str,
    description: &'static str,
    /// Extra words search matches on, so a command is findable by terms that
    /// do not appear in its name.
    keyword: &'static str,
    icon: IconName,
    action: fn(&mut App),
}

/// The launcher's commands, in the order shown when the query is empty.
static TOOLS: &[Tool] = &[
    Tool {
        name: "Program Folder",
        description: "打开启动器所在的文件夹",
        keyword: "program folder directory explorer 程序 目录 文件夹 打开",
        icon: IconName::FolderOpen,
        action: open_program_folder,
    },
    Tool {
        name: "Repository",
        description: "在浏览器打开项目仓库",
        keyword: "github repository source 仓库 源码 项目",
        icon: IconName::Globe,
        action: open_repository,
    },
    Tool {
        name: "Quit GPUI Tools",
        description: "退出启动器",
        keyword: "quit exit close 退出 关闭",
        icon: IconName::Close,
        action: quit_launcher,
    },
];

fn main() {
    launch();
}

/// The directory the launcher runs from.
fn program_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(std::path::Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// One command row: icon, name and description.
#[derive(IntoElement)]
struct ToolRow {
    name: &'static str,
    description: &'static str,
    icon: IconName,
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
                    .child(Icon::new(self.icon).size_4().text_color(if selected {
                        accent_fg
                    } else {
                        fg
                    })),
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
                            .child(self.name),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(detail_color)
                            .child(self.description),
                    ),
            )
    }
}

/// The launcher list, backed by the command table.
struct ToolDelegate {
    query: String,
    selected: usize,
    /// Notified on every query change so the host chrome stays current.
    view: Option<WeakEntity<Launcher>>,
}

impl ToolDelegate {
    fn new() -> Self {
        Self {
            query: String::new(),
            selected: 0,
            view: None,
        }
    }

    fn visible(&self) -> Vec<&'static Tool> {
        TOOLS.iter().filter(|tool| self.matches(tool)).collect()
    }

    /// Case-insensitive matching where every whitespace-separated token of the
    /// query must appear somewhere in the command's searchable text.
    fn matches(&self, tool: &Tool) -> bool {
        let query = self.query.trim();
        if query.is_empty() {
            return true;
        }
        let haystack =
            format!("{} {} {}", tool.name, tool.description, tool.keyword).to_lowercase();
        query
            .split_whitespace()
            .all(|token| haystack.contains(&token.to_lowercase()))
    }

    fn visible_count(&self) -> usize {
        self.visible().len()
    }

    fn selected_tool(&self) -> Option<&'static Tool> {
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
        let tool = *self.visible().get(row)?;
        Some(ToolRow {
            name: tool.name,
            description: tool.description,
            icon: tool.icon.clone(),
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
        let Some(tool) = self.selected_tool() else {
            return;
        };
        (tool.action)(cx);
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SettingsTab {
    System,
    Appearance,
    About,
}

impl SettingsTab {
    /// Every section, in the order the section list shows them.
    const ALL: [Self; 3] = [Self::System, Self::Appearance, Self::About];

    fn label(self) -> &'static str {
        match self {
            Self::System => "系统",
            Self::Appearance => "外观",
            Self::About => "关于",
        }
    }

    fn icon(self) -> IconName {
        match self {
            Self::System => IconName::Cpu,
            Self::Appearance => IconName::Palette,
            Self::About => IconName::Info,
        }
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
    /// The text in the search field, kept so the bar can size itself to it.
    query: String,
    /// The page on show, which the header, body and footer all follow.
    page: Page,
    /// Kept alive for as long as the launcher is: dropping it would stop the
    /// field from reporting what the user typed.
    _search_events: Subscription,
}

impl Launcher {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let list = cx.new(|cx| ListState::new(ToolDelegate::new(), window, cx));
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

        let launcher = Self {
            list,
            search,
            query: String::new(),
            page: Page::Commands,
            _search_events: search_events,
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

    /// Start a fresh session: command list, query cleared, first row highlighted.
    fn reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.query.is_empty() {
            self.search
                .update(cx, |field, cx| field.set_value("", window, cx));
            self.query.clear();
        }
        self.list
            .update(cx, |list, cx| list.set_query("", window, cx));
        self.page = Page::Commands;
        self.focus_search(window, cx);
        self.select_row(0, window, cx);
        cx.notify();
    }

    /// Run the highlighted command.
    fn activate_selected(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.page, Page::Commands) {
            return;
        }
        if let Some(tool) = self.list.read(cx).delegate().selected_tool() {
            (tool.action)(cx);
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
    fn choose_theme(&mut self, choice: ColorScheme, window: &mut Window, cx: &mut Context<Self>) {
        let mut settings = stored_settings(cx);
        if settings.theme == choice {
            return;
        }
        settings.theme = choice;
        apply_theme(choice, Some(window), cx);
        store_settings(settings, cx);
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

        let bar = div()
            .id("panel-bar")
            .window_control_area(WindowControlArea::Drag)
            .flex()
            .items_center()
            .gap_2()
            .h(px(HEADER_HEIGHT))
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
                        .w(search_field_width(window, cx, &self.query))
                        .h(px(SEARCH_FIELD_HEIGHT))
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
                .when(self.query.is_empty(), |this| {
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
        let (border, muted, muted_fg, fg, accent, accent_fg) = {
            let theme = cx.theme();
            (
                theme.border,
                theme.muted,
                theme.muted_foreground,
                theme.foreground,
                theme.accent,
                theme.accent_foreground,
            )
        };
        let colors = RowColors {
            background: muted,
            foreground: fg,
            muted_foreground: muted_fg,
            accent,
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

        let content =
            match tab {
                SettingsTab::System => v_flex()
                    .gap_1()
                    .child(info_row("全局快捷键", HOTKEY_LABEL.to_string(), colors))
                    .child(action_row(
                        "open-config",
                        "配置文件夹",
                        settings_directory(),
                        colors,
                        cx.listener(|_, _, _, cx| open_settings_folder(cx)),
                    ))
                    .child(action_row(
                        "quit",
                        "退出启动器",
                        "关闭面板与托盘".to_string(),
                        colors,
                        cx.listener(|_, _, _, cx| quit_launcher(cx)),
                    ))
                    .into_any_element(),
                SettingsTab::Appearance => v_flex()
                    .gap_1()
                    .child(section(
                        "主题",
                        muted_fg,
                        h_flex()
                            .gap_2()
                            .children(ColorScheme::ALL.into_iter().enumerate().map(
                                |(ix, choice)| {
                                    let selected = choice == settings.theme;
                                    div()
                                        .id(("theme", ix))
                                        .px_3()
                                        .py_1()
                                        .rounded(px(6.))
                                        .text_sm()
                                        .text_color(if selected { accent_fg } else { fg })
                                        .cursor_pointer()
                                        .when(selected, move |this| this.bg(accent))
                                        .when(!selected, move |this| {
                                            this.hover(move |style| {
                                                style.bg(muted_fg.opacity(0.18))
                                            })
                                        })
                                        .on_click(cx.listener(move |launcher, _, window, cx| {
                                            launcher.choose_theme(choice, window, cx)
                                        }))
                                        .child(choice.label())
                                },
                            )),
                    ))
                    .into_any_element(),
                SettingsTab::About => v_flex()
                    .gap_1()
                    .child(info_row(
                        "版本",
                        env!("CARGO_PKG_VERSION").to_string(),
                        colors,
                    ))
                    .child(action_row(
                        "open-repository",
                        "项目仓库",
                        REPOSITORY_URL.to_string(),
                        colors,
                        cx.listener(|_, _, _, cx| open_repository(cx)),
                    ))
                    .child(action_row(
                        "open-folder",
                        "程序文件夹",
                        program_dir().display().to_string(),
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
}

/// The folder the settings file lives in, for the row that opens it.
fn settings_directory() -> String {
    Settings::directory().map_or_else(
        || "未找到配置目录".to_string(),
        |dir| dir.display().to_string(),
    )
}

/// Open the folder that holds the settings file.
fn open_settings_folder(cx: &mut App) {
    let Some(dir) = Settings::directory() else {
        eprintln!("gpui-tools: no settings directory to open");
        return;
    };
    if !dir.is_dir() {
        eprintln!("gpui-tools: {} has not been created yet", dir.display());
        return;
    }
    open_folder(&dir, cx);
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
    background: Hsla,
    foreground: Hsla,
    muted_foreground: Hsla,
    accent: Hsla,
}

/// A settings row: a label on the left, its value on the right.
fn info_row(label: &'static str, value: String, colors: RowColors) -> Div {
    h_flex()
        .justify_between()
        .items_center()
        .gap_3()
        .px_3()
        .py_2()
        .rounded(px(6.))
        .bg(colors.background)
        .overflow_hidden()
        .child(
            div()
                .flex_shrink_0()
                .text_sm()
                .text_color(colors.muted_foreground)
                .child(label),
        )
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .text_sm()
                .text_color(colors.foreground)
                .child(value),
        )
}

/// A settings row that runs something when clicked.
fn action_row(
    id: &'static str,
    label: &'static str,
    value: String,
    colors: RowColors,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    info_row(label, value, colors)
        .id(id)
        .cursor_pointer()
        .hover(move |style| style.bg(colors.accent.opacity(0.18)))
        .on_click(on_click)
}

/// A settings section: a caption above its content.
fn section(title: &'static str, muted_fg: Hsla, content: impl IntoElement) -> impl IntoElement {
    v_flex()
        .gap_2()
        .child(div().text_xs().text_color(muted_fg).child(title))
        .child(content)
}

fn launch() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .with_quit_mode(QuitMode::Explicit)
        .run(|cx: &mut App| {
            gpui_kit::init(cx);

            let settings = Settings::load();
            cx.set_global(SettingsStore(settings));

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

            if let Ok(manager) = global_hotkey::GlobalHotKeyManager::new() {
                use global_hotkey::{
                    GlobalHotKeyEvent, HotKeyState,
                    hotkey::{Code, HotKey, Modifiers},
                };
                let hotkey = HotKey::new(Some(Modifiers::CONTROL), Code::Space);
                if manager.register(hotkey).is_ok() {
                    cx.spawn(async move |cx: &mut gpui_kit::AsyncApp| {
                        loop {
                            cx.background_executor()
                                .timer(Duration::from_millis(80))
                                .await;
                            while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
                                if event.id == hotkey.id() && event.state == HotKeyState::Pressed {
                                    cx.update(show_launcher);
                                }
                            }
                        }
                    })
                    .detach();
                    cx.set_global(HotkeyRegistration { _manager: manager });
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

fn launcher_tray_icon() -> TrayIcon {
    let mut rgba = Vec::with_capacity(16 * 16 * 4);
    for y in 0..16 {
        for x in 0..16 {
            let inside = (3..13).contains(&x) && (3..13).contains(&y);
            rgba.extend_from_slice(if inside {
                &[39, 196, 189, 255]
            } else {
                &[0, 0, 0, 0]
            });
        }
    }
    TrayIcon::from_rgba(rgba, 16, 16).expect("valid launcher tray icon")
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
        .map_or_else(Settings::default, |store| store.0)
}

/// Store the settings in memory and on disk.
fn store_settings(settings: Settings, cx: &mut App) {
    cx.set_global(SettingsStore(settings));
    if !settings.save() {
        eprintln!("gpui-tools: could not store the settings");
    }
}

/// Remember where the panel sits, so the next run opens there.
///
/// Only the top edge is kept: the horizontal position is re-derived by
/// centring, so the panel never reappears against one edge of the screen.
fn remember_window_position(cx: &mut App) {
    let Some(panel) = cx.try_global::<LauncherPanel>() else {
        return;
    };
    let y = panel.window.position().1;
    let mut settings = stored_settings(cx);
    if settings.window_y == Some(y) {
        return;
    }
    settings.window_y = Some(y);
    store_settings(settings, cx);
}

fn open_program_folder(cx: &mut App) {
    open_folder(&program_dir(), cx);
}

/// Show a folder in the file manager, bringing the panel back to the front
/// afterwards: Explorer takes the foreground, and the panel is meant to be what
/// the user is looking at.
fn open_folder(dir: &std::path::Path, cx: &mut App) {
    // An argument array, never a shell string.
    if let Err(error) = std::process::Command::new("explorer").arg(dir).spawn() {
        eprintln!("gpui-tools: could not open {}: {error}", dir.display());
    }
    cx.activate(true);
}

fn open_repository(cx: &mut App) {
    cx.open_url(REPOSITORY_URL);
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

struct HotkeyRegistration {
    _manager: global_hotkey::GlobalHotKeyManager,
}
impl gpui_kit::Global for HotkeyRegistration {}
