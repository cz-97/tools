//! 「自定义条目」页：文件 / 文件夹 / 快捷终端 三种用户条目的列表。
//!
//! 数据和书签、历史那几页同一个套路：内容交给 gpui-component 的 `List`，滚动、
//! 选中、键盘都由它管——手写 `div` + `overflow_y_scroll` 在设置页里试过两轮，
//! 高度约束和滚动句柄总有对不齐的地方，这里直接用现成组件省掉那一整类问题。
//!
//! 行只做三件事：看一眼（图标 + 名字 + 目标）、点一下就跑、右上角新增弹窗。

use gpui_kit::component::list::{List, ListDelegate, ListState};
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
}

impl UserToolsDelegate {
    pub(crate) fn new(kind: ToolKind) -> Self {
        Self {
            kind,
            query: String::new(),
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
        Some(UserToolRow {
            tool,
            index: ix.row,
        })
    }

    fn set_selected_index(
        &mut self,
        _ix: Option<IndexPath>,
        _window: &mut Window,
        _cx: &mut Context<ListState<Self>>,
    ) {
    }

    fn confirm(
        &mut self,
        _secondary: bool,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        let app: &mut App = cx;
        let Some(id) = self.rows(app).first().map(|tool| tool.id) else {
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
}

impl Selectable for UserToolRow {
    fn selected(self, _selected: bool) -> Self {
        self
    }

    fn is_selected(&self) -> bool {
        false
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
            .hover(move |style| style.bg(colors.hover))
            .on_click(move |_, _window, cx| crate::app::run_tool(id, cx))
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
            // 改：弹窗从这一行旁边点开，比先选中再找按钮快。
            .child(
                div()
                    .id(SharedString::from(format!("user-tool-edit-{}", self.index)))
                    .flex_shrink_0()
                    .px_1p5()
                    .py_0p5()
                    .rounded(px(4.))
                    .text_xs()
                    .cursor_pointer()
                    .text_color(colors.muted_foreground)
                    .hover(move |style| style.text_color(colors.foreground))
                    .child(strings::tools_page::EDIT)
                    .on_click(move |_, window, cx| {
                        let kind = kind;
                        crate::app::with_launcher(cx, move |launcher, window, cx| {
                            launcher.start_tool_editor(kind, Some(id), window, cx)
                        });
                    }),
            )
    }
}

/// 把某一类条目画成一张列表。`entries` 的数据由 `Launcher::sync_user_list` 装好。
pub(crate) fn render(entries: &Entity<ListState<UserToolsDelegate>>) -> AnyElement {
    List::new(entries).px_1().py_1().into_any_element()
}
