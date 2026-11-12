//! 「自定义条目」页：文件 / 文件夹 / 快捷终端 三种用户条目的列表。
//!
//! 数据和书签、历史那几页同一个套路：内容交给 gpui-component 的 `List`，滚动、
//! 选中、键盘都由它管——手写 `div` + `overflow_y_scroll` 在设置页里试过两轮，
//! 高度约束和滚动句柄总有对不齐的地方，这里直接用现成组件省掉那一整类问题。
//!
//! 行只做三件事：看一眼（图标 + 名字 + 目标）、点一下就跑、右上角新增弹窗。

use gpui_kit::component::list::{List, ListDelegate, ListState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Icon, IndexPath, Selectable, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, Context, Entity, IntoElement, RenderOnce, SharedString, Window, div,
    prelude::*, px,
};
use tools::settings::{ToolKind, UserTool};

use crate::app::stored_settings;

use crate::strings;
use crate::ui::colors::UiColors;
use crate::ui::metrics::{RADIUS_CHIP, RADIUS_CONTROL};
use crate::ui::launcher::{EnabledFilter, user_kind_icon};

/// 某一类用户条目的列表数据。
///
/// **条目是缓存的**：`items_count`、`render_item` 每帧都要问一遍，如果每次都现读设置再
/// 全量克隆一遍，滚一下就卡一下（用户报过）。缓存只在 [`Self::retarget`] 里重建——
/// 增删改、开关、筛选、换类别都会走到 `Launcher::sync_user_list`，那就是唯一的刷新点。
pub(crate) struct UserToolsDelegate {
    kind: ToolKind,
    query: String,
    /// 「开启状态」筛选：全部 / 只看上主页的 / 只看没上主页的。
    filter: EnabledFilter,
    /// 当前类别 + 筛选 + 搜索词下能看到的条目。
    rows: Vec<UserTool>,
    /// 高亮在第几行（单击选中的就是它）。
    selected: usize,
    /// 正在录快捷键的那一条（行上的按钮要显示「按下组合键…」）。
    recording: Option<i64>,
    /// 录制期间被拒绝的原因。
    conflict: Option<String>,
}

impl UserToolsDelegate {
    pub(crate) fn new(kind: ToolKind) -> Self {
        Self {
            kind,
            query: String::new(),
            filter: EnabledFilter::All,
            rows: Vec::new(),
            selected: 0,
            recording: None,
            conflict: None,
        }
    }

    /// 换类别、搜索词或筛选，并按新条件重建缓存。
    ///
    /// 条件没变也照样重建：增删改、开关都不改这三个条件，只改数据库里的内容，
    /// 早退一次就会留着旧的那一批。
    pub(crate) fn retarget(
        &mut self,
        kind: ToolKind,
        query: &str,
        filter: EnabledFilter,
        cx: &App,
    ) {
        self.kind = kind;
        self.query = query.trim().to_lowercase();
        self.filter = filter;
        self.reload(cx);
    }

    /// 现从设置里筛一遍。名字和路径都能搜，原文找不到时按拼音（全拼 / 首字母）再找一遍。
    fn reload(&mut self, cx: &App) {
        self.rows = stored_settings(cx)
            .tools_of(self.kind)
            .filter(|tool| self.filter.keeps(tool.enabled))
            .filter(|tool| {
                tools::pinyin::matches(&format!("{} {}", tool.name, tool.target), &self.query)
            })
            .cloned()
            .collect();
        if self.selected >= self.rows.len() {
            self.selected = 0;
        }
    }

    /// 能看见的行数。
    pub(crate) fn len(&self) -> usize {
        self.rows.len()
    }

    /// 高亮到第几行。收起面板时记现场用。
    pub(crate) fn selected(&self) -> usize {
        self.selected
    }

    /// 把高亮摆回第 `row` 行（过滤条件变了、或者从记忆里回来时用）。
    pub(crate) fn restore_selection(&mut self, row: usize) {
        self.selected = row.min(self.rows.len().saturating_sub(1));
    }

    /// 第 `row` 行是哪一条（双击执行用）。
    pub(crate) fn id_at(&self, row: usize) -> Option<i64> {
        self.rows.get(row).map(|tool| tool.id)
    }

    /// 高亮那一行是哪一条（回车执行用）。
    pub(crate) fn selected_id(&self) -> Option<i64> {
        self.id_at(self.selected)
    }

    /// 录制状态变了：按钮上的字跟着变。
    pub(crate) fn set_recording(&mut self, recording: Option<i64>, conflict: Option<String>) {
        self.recording = recording;
        self.conflict = conflict;
    }
}

impl ListDelegate for UserToolsDelegate {
    type Item = UserToolRow;

    fn items_count(&self, _section: usize, _cx: &App) -> usize {
        self.rows.len()
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _window: &mut Window,
        _cx: &mut Context<ListState<Self>>,
    ) -> Option<Self::Item> {
        let tool = self.rows.get(ix.row)?.clone();
        let selected = self.selected == ix.row;
        let recording = self.recording == Some(tool.id);
        let conflict = self.conflict.clone();
        Some(UserToolRow {
            tool,
            index: ix.row,
            selected,
            recording,
            conflict,
        })
    }

    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        // 单击、上下键都走到这儿（`ListState` 自己那个 `selected_index` 才是真相，
        // 行上画的选中态也是它派下来的）。
        self.selected = ix.map(|path| path.row).unwrap_or(0);
        cx.notify();
    }

    fn confirm(
        &mut self,
        _secondary: bool,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        let app: &mut App = cx;
        let Some(id) = self.id_at(self.selected) else {
            return;
        };
        crate::app::run_tool(id, app);
    }

    fn render_empty(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        let colors = UiColors::current(cx);
        div()
            .px_2()
            .py_2()
            .text_xs()
            .text_color(colors.muted_foreground)
            .child(strings::tools_page::empty_hint(self.kind))
            .into_any_element()
    }
}

/// 列表里的一行：系统图标 + 名字 + 目标，右侧是快捷键。
#[derive(IntoElement)]
pub(crate) struct UserToolRow {
    tool: UserTool,
    index: usize,
    selected: bool,
    recording: bool,
    conflict: Option<String>,
}

impl Selectable for UserToolRow {
    /// **不认 `List` 推下来的选中态。**
    ///
    /// 这一页的选中是 delegate 自己管的（单击、双击语义和别的页不一样，`ListState` 建的时候
    /// 就 `selectable(false)` 交出来了）。`ListState` 仍然会拿它自己那个 `selected_index`
    /// 调这个方法把行刷新一遍——增删条目之后那个下标会过期，认了就会出现「高亮在 A、
    /// 回车执行 B」。`selected_index` 我们只用来滚动，所以这里原样返回。
    fn selected(self, _selected: bool) -> Self {
        self
    }

    fn is_selected(&self) -> bool {
        self.selected
    }
}

impl RenderOnce for UserToolRow {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = UiColors::current(cx);
        let icon: AnyElement = match crate::ui::icons::for_tool(self.tool.kind, &self.tool.target) {
            Some(image) => gpui_kit::img(image)
                .w(px(20.))
                .h(px(20.))
                .into_any_element(),
            None => Icon::new(user_kind_icon(self.tool.kind))
                .size_4()
                .text_color(colors.foreground)
                .into_any_element(),
        };
        let id = self.tool.id;
        let kind = self.tool.kind;
        let index = self.index;

        h_flex()
            // **element id 一律用数据库主键，不用行号。** 行号会随着过滤、增删、排序换位置，
            // 而 gpui 的命中链和动画状态是按 id 记的：用行号的话，列表一重建，原来第 3 行
            // 的开关状态就套到了新的第 3 行身上（用户报过「点一下快捷键，后面几行的开关在动」）。
            .id(SharedString::from(format!("user-tool-row-{id}")))
            .w_full()
            .min_w_0()
            .gap_3()
            .px_2()
            .py_1p5()
            .rounded(px(RADIUS_CONTROL))
            .cursor_pointer()
            .when(self.selected, move |this| this.bg(colors.accent))
            .when(!self.selected, move |this| {
                this.hover(move |style| style.bg(colors.hover))
            })
            .on_click(move |event: &gpui_kit::ClickEvent, _window, cx| {
                // 单击只选中（和书签、历史同一套手感），双击才执行。
                // 选中态归 `ListState`，所以得绕回 `Launcher` 去改。
                cx.stop_propagation();
                let clicks = event.click_count();
                crate::app::with_launcher(cx, move |launcher, window, cx| {
                    launcher.user_row_clicked(index, clicks, window, cx);
                });
            })
            .child(div().flex_shrink_0().child(icon))
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
                            .text_color(colors.foreground)
                            .child(self.tool.name.clone()),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(self.tool.target.clone()),
                    ),
            )
            .child(
                // 固定宽度 + 右对齐：数字位数不同也叠在同一列上。
                h_flex()
                    .flex_shrink_0()
                    .w(px(52.))
                    .justify_end()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(strings::common::uses(self.tool.uses)),
            )
            // 快捷键：点一下开始录，右键清掉（和设置里那条一样的操作）。
            //
            // 行里的控件一律用 `block_mouse_except_scroll`，**不要用 `occlude`**：
            // `occlude` 是 `HitboxBehavior::BlockMouse`，它会把鼠标底下的所有 hitbox
            // 从命中链里**截断**，列表的滚动容器也就拿不到 `should_handle_scroll`——
            // 表现是「鼠标停在按钮/开关上滚不动」。前者只挡点击和悬停，滚轮照传。
            .child(
                div()
                    .block_mouse_except_scroll()
                    .w(px(104.))
                    .flex_shrink_0()
                    .child(crate::ui::commands_page::shortcut_button(
                    SharedString::from(format!("user-tool-shortcut-{id}")),
                    if self.recording {
                        self.conflict
                            .clone()
                            .map_or_else(
                                || strings::commands_page::RECORDING.to_string(),
                                |text: String| text,
                            )
                            .into()
                    } else {
                        match self.tool.shortcut.as_deref() {
                            Some(chord) => crate::shortcuts::shortcut_label(chord).into(),
                            None => strings::commands_page::SHORTCUT_EMPTY.into(),
                        }
                    },
                    self.recording,
                    colors,
                    px(96.),
                    move |_, _, cx| {
                        crate::app::with_launcher(cx, move |launcher, window, cx| {
                            launcher.toggle_tool_recording(id, cx);
                            // 点完控件就把焦点还给搜索框：否则焦点留在按钮上，用户接着
                            // 用中文输入法打字，第一个字母进不了输入法上下文。
                            launcher.focus_active_input(window, cx);
                        });
                    },
                    move |_, _, cx| {
                        crate::app::with_launcher(cx, move |launcher, window, cx| {
                            launcher.clear_shortcut(
                                crate::ui::launcher::RecordTarget::Tool(id),
                                cx,
                            );
                            launcher.focus_active_input(window, cx);
                        });
                    },
                ),
            ))
            // 开关：这一条上不上主页。
            .child(
                div()
                    .flex_shrink_0()
                    .block_mouse_except_scroll()
                    .w(px(40.))
                    .child(
                        Switch::new(SharedString::from(format!("user-tool-switch-{id}")))
                        .checked(self.tool.enabled)
                        .on_change(move |checked, _window, cx| {
                            let checked = *checked;
                            crate::app::with_launcher(cx, move |launcher, window, cx| {
                                launcher.set_tool_enabled(id, checked, cx);
                                launcher.focus_active_input(window, cx);
                            });
                        }),
                    ),
            )
            // 文件是「用默认应用开」还是「打开所在位置」；终端是「静默跑」还是「留窗口」。
            .when(kind == ToolKind::File, |this| {
                let mode = if self.tool.open_default {
                    strings::tools_page::MODE_DEFAULT_APP
                } else {
                    strings::tools_page::MODE_LOCATION
                };
                this.child(row_button(
                    SharedString::from(format!("user-tool-mode-{id}")),
                    mode.into(),
                    64.,
                    colors,
                    move |_, _window, cx| {
                        crate::app::with_launcher(cx, move |launcher, window, cx| {
                            launcher.toggle_tool_open_default(id, cx);
                            launcher.focus_active_input(window, cx);
                        });
                    },
                ))
            })
            .when(kind == ToolKind::Terminal, |this| {
                let mode = if self.tool.quiet {
                    strings::tools_page::MODE_QUIET
                } else {
                    strings::tools_page::MODE_TERMINAL
                };
                this.child(row_button(
                    SharedString::from(format!("user-tool-mode-{id}")),
                    mode.into(),
                    64.,
                    colors,
                    move |_, _window, cx| {
                        crate::app::with_launcher(cx, move |launcher, window, cx| {
                            launcher.toggle_tool_quiet(id, cx);
                            launcher.focus_active_input(window, cx);
                        });
                    },
                ))
            })
            // 改：弹窗从这一行旁边点开，比先选中再找按钮快。
            .child(row_button(
                SharedString::from(format!("user-tool-edit-{id}")),
                strings::tools_page::EDIT.into(),
                32.,
                colors,
                move |_, _window, cx| {
                    crate::app::with_launcher(cx, move |launcher, window, cx| {
                        launcher.start_tool_editor(kind, Some(id), window, cx)
                    });
                },
            ))
            // 删：直接删，没有二次确认——设置里那条也是这样。
            .child(row_button(
                SharedString::from(format!("user-tool-delete-{id}")),
                strings::tools_page::DELETE.into(),
                32.,
                colors,
                    move |_, _window, cx| {
                        crate::app::with_launcher(cx, move |launcher, window, cx| {
                            launcher.delete_tool(id, cx);
                            launcher.focus_active_input(window, cx);
                        });
                    },
            ))
    }
}

/// 行尾那种小文字按钮。
fn row_button(
    id: SharedString,
    label: SharedString,
    width: f32,
    colors: UiColors,
    on_click: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        // 只挡点击和悬停，滚轮要能传给列表（`occlude` 会把滚轮一起截掉）。
        .block_mouse_except_scroll()
        .flex_shrink_0()
        .w(px(width))
        .flex()
        .justify_center()
        .px_1p5()
        .py_0p5()
        .rounded(px(RADIUS_CHIP))
        .text_xs()
        .cursor_pointer()
        .text_color(colors.muted_foreground)
        .hover(move |style| style.text_color(colors.foreground))
        .child(label)
        .on_click(on_click)
}

/// 把某一类条目画成一张列表。`entries` 的数据由 `Launcher::sync_user_list` 装好。
pub(crate) fn render(entries: &Entity<ListState<UserToolsDelegate>>) -> AnyElement {
    List::new(entries).px_1().py_1().into_any_element()
}
