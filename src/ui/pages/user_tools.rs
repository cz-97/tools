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
use crate::ui::launcher::user_kind_icon;

/// 某一类用户条目的列表数据。
pub(crate) struct UserToolsDelegate {
    kind: ToolKind,
    query: String,
    /// 高亮在第几行（单击选中的就是它）。
    selected: usize,
}

impl UserToolsDelegate {
    pub(crate) fn new(kind: ToolKind) -> Self {
        Self {
            kind,
            query: String::new(),
            selected: 0,
        }
    }

    /// 换类别或搜索词。**数据不在这里存**：每次现从设置里筛，改动立刻反映出来，
    /// 不用在添加/删除/编辑之后挨个地方记得刷新。
    pub(crate) fn retarget(&mut self, kind: ToolKind, query: &str) {
        self.kind = kind;
        self.query = query.trim().to_lowercase();
    }

    pub(crate) fn kind(&self) -> ToolKind {
        self.kind
    }

    /// 当前类别 + 搜索词下能看到的条目。
    pub(crate) fn rows(&self, cx: &App) -> Vec<UserTool> {
        stored_settings(cx)
            .tools_of(self.kind)
            .filter(|tool| {
                self.query.is_empty()
                    || tool.name.to_lowercase().contains(&self.query)
                    || tool.target.to_lowercase().contains(&self.query)
            })
            .cloned()
            .collect()
    }
}

impl ListDelegate for UserToolsDelegate {
    type Item = UserToolRow;

    fn items_count(&self, _section: usize, cx: &App) -> usize {
        self.rows(cx).len()
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<Self::Item> {
        let tool = self.rows(cx).get(ix.row)?.clone();
        let selected = self.selected == ix.row;
        Some(UserToolRow {
            tool,
            index: ix.row,
            selected,
        })
    }

    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        _window: &mut Window,
        _cx: &mut Context<ListState<Self>>,
    ) {
        // 单击选中：记住是高亮哪一行，画的时候点亮。
        if let Some(ix) = ix {
            self.selected = ix.row;
        }
    }

    fn confirm(
        &mut self,
        _secondary: bool,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        let app: &mut App = cx;
        let selected = self.selected;
        let Some(id) = self.rows(app).get(selected).map(|tool| tool.id) else {
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
}

impl Selectable for UserToolRow {
    fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
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

        h_flex()
            .id(SharedString::from(format!("user-tool-{}", self.index)))
            .w_full()
            .min_w_0()
            .gap_3()
            .px_2()
            .py_1p5()
            .rounded(px(6.))
            .cursor_pointer()
            .when(self.selected, move |this| this.bg(colors.accent))
            .when(!self.selected, move |this| {
                this.hover(move |style| style.bg(colors.hover))
            })
            .on_click(move |event: &gpui_kit::ClickEvent, _window, cx| {
                // 单击只选中（`List` 自己会记），双击才执行。
                cx.stop_propagation();
                if event.click_count() >= 2 {
                    crate::app::run_tool(id, cx);
                }
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
                div()
                    .flex_shrink_0()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(strings::common::uses(self.tool.uses)),
            )
            // 快捷键：点一下开始录，右键清掉（和设置里那条一样的操作）。
            .child(
                div()
                    .occlude()
                    .child(crate::ui::commands_page::shortcut_button(
                        SharedString::from(format!("user-tool-shortcut-{}", self.index)),
                        match self.tool.shortcut.as_deref() {
                            Some(chord) => crate::shortcuts::shortcut_label(chord).into(),
                            None => strings::commands_page::SHORTCUT_EMPTY.into(),
                        },
                        false,
                        colors,
                        px(96.),
                        move |_, _, cx| {
                            crate::app::with_launcher(cx, move |launcher, _window, cx| {
                                launcher.toggle_tool_recording(id, cx)
                            });
                        },
                        move |_, _, cx| {
                            crate::app::with_launcher(cx, move |launcher, _window, cx| {
                                launcher
                                    .clear_shortcut(crate::ui::launcher::RecordTarget::Tool(id), cx)
                            });
                        },
                    )),
            )
            // 开关：这一条上不上主页。
            .child(
                div().flex_shrink_0().occlude().child(
                    Switch::new(SharedString::from(format!(
                        "user-tool-switch-{}",
                        self.index
                    )))
                    .checked(self.tool.enabled)
                    .on_change(move |checked, _window, cx| {
                        let checked = *checked;
                        crate::app::with_launcher(cx, move |launcher, _window, cx| {
                            launcher.set_tool_enabled(id, checked, cx)
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
                    SharedString::from(format!("user-tool-mode-{}", self.index)),
                    mode.into(),
                    colors,
                    move |_, _window, cx| {
                        crate::app::with_launcher(cx, move |launcher, _window, cx| {
                            launcher.toggle_tool_open_default(id, cx)
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
                    SharedString::from(format!("user-tool-mode-{}", self.index)),
                    mode.into(),
                    colors,
                    move |_, _window, cx| {
                        crate::app::with_launcher(cx, move |launcher, _window, cx| {
                            launcher.toggle_tool_quiet(id, cx)
                        });
                    },
                ))
            })
            // 改：弹窗从这一行旁边点开，比先选中再找按钮快。
            .child(row_button(
                SharedString::from(format!("user-tool-edit-{}", self.index)),
                strings::tools_page::EDIT.into(),
                colors,
                move |_, _window, cx| {
                    crate::app::with_launcher(cx, move |launcher, window, cx| {
                        launcher.start_tool_editor(kind, Some(id), window, cx)
                    });
                },
            ))
            // 删：直接删，没有二次确认——设置里那条也是这样。
            .child(row_button(
                SharedString::from(format!("user-tool-delete-{}", self.index)),
                strings::tools_page::DELETE.into(),
                colors,
                move |_, _window, cx| {
                    crate::app::with_launcher(cx, move |launcher, _window, cx| {
                        launcher.delete_tool(id, cx)
                    });
                },
            ))
    }
}

/// 行尾那种小文字按钮。
fn row_button(
    id: SharedString,
    label: SharedString,
    colors: UiColors,
    on_click: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .occlude()
        .flex_shrink_0()
        .px_1p5()
        .py_0p5()
        .rounded(px(4.))
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
