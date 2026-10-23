//! 工具列表页：书签、历史、个人项目、星标、查询仓库共用这一张列表。
//!
//! 五个页面的骨架一样：图标 + 标题 + 描述 + 右侧元信息，差别只在数据从哪儿来、怎么排版。
//! 数据由 `launcher` 装进来，这里只管过滤、画行和处理点击：
//!
//! - **单击**：选中这一行（和 `↑↓` 是同一件事），顺手把链接复制走；
//! - **双击**：打开。
//!
//! 行自己挂 `on_click`（而不是交给 `List` 的 `confirm`）：`confirm` 分不出单击和双击。

use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::component::list::{List, ListDelegate, ListState};
use gpui_kit::component::{Icon, IndexPath, Selectable, h_flex, v_flex};
// 图标用全集：书签、历史这些不在组件自带的那一小套里。
use gpui_kit::assets::IconName;
use gpui_kit::{
    AnyElement, ClickEvent, Context, FontWeight, Image, IntoElement, ObjectFit, RenderOnce,
    SharedString, Task, WeakEntity, Window, div, img, prelude::*, px,
};
use gpui_tools::firefox::{Favicon, IconKind};
use gpui_tools::shell;

use crate::strings;
use crate::ui::colors::UiColors;
use crate::ui::launcher::{Launcher, ListKind};

/// 「已复制」亮多久。
const COPY_FEEDBACK: Duration = Duration::from_millis(1200);

/// 列表里的一行。
pub(crate) struct Entry {
    pub(crate) title: String,
    /// 副行：网址，或者仓库简介。
    pub(crate) detail: String,
    /// 右侧那点信息：访问次数、主语言。
    pub(crate) meta: String,
    /// 回车/双击要打开的东西，一律交给 shell 解析。
    pub(crate) target: String,
    /// 站点图标；没有就用 `tile` 画个默认图标。
    pub(crate) icon: Option<Arc<Image>>,
    pub(crate) tile: IconName,
}

impl Entry {
    /// 一行只有文字、用默认图标。
    pub(crate) fn new(
        title: impl Into<String>,
        detail: impl Into<String>,
        meta: impl Into<String>,
        target: impl Into<String>,
        tile: IconName,
    ) -> Self {
        Self {
            title: title.into(),
            detail: detail.into(),
            meta: meta.into(),
            target: target.into(),
            icon: None,
            tile,
        }
    }

    /// 挂上站点图标。
    pub(crate) fn with_icon(mut self, icon: Option<Arc<Image>>) -> Self {
        self.icon = icon;
        self
    }

    /// 搜索用的一段文本：标题、描述、右侧信息都算。
    fn haystack(&self) -> String {
        format!("{} {} {}", self.title, self.detail, self.meta).to_lowercase()
    }
}

/// 火狐的图标转成 GPUI 能画的图片。
///
/// 图片按内容哈希去重（`Image` 自己就是这么做的），所以同一张图标被几十个书签引用，
/// 也只解码一次。
pub(crate) fn favicon_image(favicon: &Favicon) -> Arc<Image> {
    let format = match favicon.kind {
        IconKind::Png => gpui_kit::ImageFormat::Png,
        IconKind::Jpeg => gpui_kit::ImageFormat::Jpeg,
        IconKind::Gif => gpui_kit::ImageFormat::Gif,
        IconKind::Webp => gpui_kit::ImageFormat::Webp,
        IconKind::Bmp => gpui_kit::ImageFormat::Bmp,
        IconKind::Ico => gpui_kit::ImageFormat::Ico,
        IconKind::Svg => gpui_kit::ImageFormat::Svg,
    };
    Arc::new(Image::from_bytes(format, favicon.bytes.clone()))
}

/// 列表里的一行怎么画。
#[derive(IntoElement)]
pub(crate) struct EntryRow {
    entry: Entry,
    selected: bool,
    index: usize,
    /// 双行排版：仓库用（第一行名字 + 语言，第二行星数和简介）。
    two_lines: bool,
    /// 刚被单击复制过，右侧短暂变成「已复制」。
    copied: bool,
    /// 点一下要通知谁。
    view: Option<WeakEntity<Launcher>>,
}

impl Selectable for EntryRow {
    fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    fn is_selected(&self) -> bool {
        self.selected
    }
}

impl RenderOnce for EntryRow {
    fn render(self, _window: &mut Window, cx: &mut gpui_kit::App) -> impl IntoElement {
        let colors = UiColors::current(cx);
        let selected = self.selected;
        let title_color = if selected {
            colors.accent_foreground
        } else {
            colors.foreground
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
        let icon_color = if selected {
            colors.accent_foreground
        } else {
            colors.foreground
        };

        let tile = div()
            .flex_shrink_0()
            .size_8()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(8.))
            .bg(tile_bg)
            .child(match self.entry.icon {
                Some(image) => img(image)
                    .size(px(18.))
                    .object_fit(ObjectFit::Contain)
                    .into_any_element(),
                None => Icon::new(self.entry.tile.clone())
                    .size_4()
                    .text_color(icon_color)
                    .into_any_element(),
            });

        let title = div()
            .flex_shrink_0()
            .max_w(px(320.))
            .min_w_0()
            .overflow_hidden()
            .text_ellipsis()
            .text_sm()
            .font_weight(FontWeight::MEDIUM)
            .text_color(title_color)
            .child(self.entry.title.clone());
        let detail = div()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .text_ellipsis()
            .text_xs()
            .text_color(detail_color)
            .child(self.entry.detail.clone());
        let meta_text: SharedString = if self.copied {
            strings::common::COPIED.into()
        } else {
            self.entry.meta.clone().into()
        };
        let meta = div()
            .flex_shrink_0()
            .max_w(px(200.))
            .overflow_hidden()
            .text_ellipsis()
            .text_xs()
            .text_color(detail_color)
            .child(meta_text);

        // 单行：名称和描述并排，名称不缩、描述吃剩下的地方（描述尽量长，但不压名称）。
        // 双行：第一行名字 + 语言，第二行星数 + 简介。
        let body = if self.two_lines {
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .child(
                    h_flex()
                        .min_w_0()
                        .w_full()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .text_ellipsis()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(title_color)
                                .child(self.entry.title.clone()),
                        )
                        .child(meta),
                )
                .child(detail)
                .into_any_element()
        } else {
            h_flex()
                .flex_1()
                .min_w_0()
                .gap_3()
                .child(title)
                .child(detail)
                .when(!self.entry.meta.is_empty(), |this| this.child(meta))
                .into_any_element()
        };

        let index = self.index;
        let view = self.view.clone();
        let height = if self.two_lines { px(52.) } else { px(44.) };

        div().w_full().py(px(1.)).child(
            h_flex()
                .id(SharedString::from(format!("entry-row-{index}")))
                .w_full()
                .h(height)
                .px_3()
                .gap_3()
                .rounded(px(8.))
                .when(selected, move |this| this.bg(colors.accent))
                .when(!selected, move |this| {
                    this.hover(move |style| style.bg(colors.hover))
                })
                .cursor_pointer()
                .on_click(move |event: &ClickEvent, window, cx| {
                    // 行自己处理点击：单击只选中并复制，双击才打开。
                    cx.stop_propagation();
                    let Some(view) = view.clone() else {
                        return;
                    };
                    let clicked = event.click_count();
                    view.update(cx, |launcher, cx| {
                        launcher.entry_clicked(index, clicked, window, cx)
                    })
                    .ok();
                })
                .child(tile)
                .child(body),
        )
    }
}

/// 列表页的数据与状态。
pub(crate) struct EntryDelegate {
    /// 这一页是五页里的哪一页，决定空状态说什么、行怎么排。
    kind: ListKind,
    /// 全部行，过滤只改变可见下标，不动它。
    all: Vec<Entry>,
    /// 过滤之后能看见的行（`all` 的下标）。
    visible: Vec<usize>,
    /// 当前的过滤词。
    query: String,
    /// 高亮的是 `visible` 里的第几行。
    selected: usize,
    /// 正在读库或者正在抓网络。
    loading: bool,
    /// 出错时的说明；有内容就顶掉空状态。
    message: Option<String>,
    /// 查询仓库页：有没有搜过（搜过且空才是「没搜到」）。
    searched: bool,
    /// 刚被单击复制过的那一行，以及时间。
    copied: Option<(usize, Instant)>,
    /// 行被点一下要回到谁那儿（复制、换焦点、打开）。
    pub(crate) view: Option<WeakEntity<Launcher>>,
}

impl EntryDelegate {
    pub(crate) fn new(kind: ListKind) -> Self {
        Self {
            kind,
            all: Vec::new(),
            visible: Vec::new(),
            query: String::new(),
            selected: 0,
            loading: false,
            message: None,
            searched: false,
            copied: None,
            view: None,
        }
    }

    /// 换一页重新开始：清空数据、清掉提示。
    pub(crate) fn reset(&mut self, kind: ListKind) {
        self.kind = kind;
        self.all.clear();
        self.visible.clear();
        self.query.clear();
        self.selected = 0;
        self.loading = false;
        self.message = None;
        self.searched = false;
        self.copied = None;
    }

    /// 装一批数据进来，按当前过滤词重算可见行，高亮回到第一行。
    pub(crate) fn set_items(&mut self, items: Vec<Entry>) {
        self.all = items;
        self.loading = false;
        self.message = None;
        self.refine();
    }

    /// 只改提示，不动数据（刷新失败时把缓存留着）。
    pub(crate) fn set_message(&mut self, message: Option<String>) {
        self.message = message;
        self.loading = false;
    }

    pub(crate) fn set_loading(&mut self, loading: bool) {
        self.loading = loading;
        if loading {
            self.message = None;
        }
    }

    pub(crate) fn mark_searched(&mut self) {
        self.searched = true;
    }

    /// 换过滤词；真的变了才返回 `true`。
    pub(crate) fn set_query(&mut self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        if self.query == query {
            return false;
        }
        self.query = query;
        self.selected = 0;
        self.refine();
        true
    }

    /// 按过滤词重算可见行。搜索词按空白切段，每一段都要在行里出现。
    fn refine(&mut self) {
        if self.query.is_empty() {
            self.visible = (0..self.all.len()).collect();
        } else {
            let tokens = self
                .query
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>();
            self.visible = self
                .all
                .iter()
                .enumerate()
                .filter(|(_, entry)| {
                    let haystack = entry.haystack();
                    tokens.iter().all(|token| haystack.contains(token.as_str()))
                })
                .map(|(index, _)| index)
                .collect();
        }
        if self.selected >= self.visible.len() {
            self.selected = 0;
        }
    }

    /// 能看见的行数。
    pub(crate) fn len(&self) -> usize {
        self.visible.len()
    }

    /// 高亮的是第几行。
    pub(crate) fn selected(&self) -> usize {
        self.selected
    }

    /// 是不是正在读库 / 抓网络。
    pub(crate) fn is_loading(&self) -> bool {
        self.loading
    }

    /// 高亮那一行对应的数据。
    pub(crate) fn selected_entry(&self) -> Option<&Entry> {
        self.visible
            .get(self.selected)
            .and_then(|index| self.all.get(*index))
    }

    /// 打开高亮那一行。
    pub(crate) fn open_selected(&self) {
        if let Some(entry) = self.selected_entry() {
            shell::open(&entry.target);
        }
    }

    /// 打开第 `visible_index` 行（双击用）。
    pub(crate) fn open(&self, visible_index: usize) {
        if let Some(entry) = self
            .visible
            .get(visible_index)
            .and_then(|index| self.all.get(*index))
        {
            shell::open(&entry.target);
        }
    }

    /// 记住某一行刚被复制过（`visible_index` 是可见行号）。
    pub(crate) fn mark_copied(&mut self, visible_index: usize) {
        if let Some(index) = self.visible.get(visible_index).copied() {
            self.copied = Some((index, Instant::now()));
        }
    }

    /// 「已复制」到点收回去。
    pub(crate) fn clear_copied(&mut self) {
        self.copied = None;
    }

    /// 这一行是不是刚被复制过。
    fn is_copied(&self, index: usize) -> bool {
        self.copied
            .is_some_and(|(row, at)| row == index && at.elapsed() < COPY_FEEDBACK)
    }

    /// 列表空着的时候说什么。
    fn empty_text(&self) -> String {
        if self.loading {
            return strings::lists::LOADING.to_string();
        }
        if let Some(message) = &self.message {
            return message.clone();
        }
        if !self.query.is_empty() && !self.all.is_empty() && self.visible.is_empty() {
            return strings::lists::no_match(&self.query);
        }
        match self.kind {
            ListKind::Bookmarks => strings::lists::NO_BOOKMARKS.to_string(),
            ListKind::History => strings::lists::NO_HISTORY.to_string(),
            ListKind::Projects | ListKind::Stars => strings::lists::NO_CACHE.to_string(),
            ListKind::Search if self.searched => strings::lists::NO_RESULT.to_string(),
            ListKind::Search => strings::lists::NO_QUERY.to_string(),
        }
    }
}

impl ListDelegate for EntryDelegate {
    type Item = EntryRow;

    fn items_count(&self, _section: usize, _cx: &gpui_kit::App) -> usize {
        self.visible.len()
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _window: &mut Window,
        _cx: &mut Context<ListState<Self>>,
    ) -> Option<Self::Item> {
        let index = *self.visible.get(ix.row)?;
        let entry = self.all.get(index)?;
        Some(EntryRow {
            entry: Entry {
                title: entry.title.clone(),
                detail: entry.detail.clone(),
                meta: entry.meta.clone(),
                target: entry.target.clone(),
                icon: entry.icon.clone(),
                tile: entry.tile.clone(),
            },
            selected: ix.row == self.selected,
            index: ix.row,
            two_lines: self.kind.is_repos(),
            copied: self.is_copied(index),
            view: self.view.clone(),
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
        _cx: &mut Context<ListState<Self>>,
    ) {
        self.open_selected();
    }

    fn cancel(&mut self, _window: &mut Window, cx: &mut Context<ListState<Self>>) {
        crate::app::hide_launcher(cx);
    }

    fn render_empty(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        let muted = UiColors::current(cx).muted_foreground;
        let label = self.empty_text();
        let icon = if self.kind == ListKind::Search {
            IconName::Search
        } else {
            IconName::Inbox
        };
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_2()
            .child(Icon::new(icon).size_6().text_color(muted))
            .child(div().px_6().text_sm().text_color(muted).child(label))
    }

    fn perform_search(
        &mut self,
        _query: &str,
        _window: &mut Window,
        _cx: &mut Context<ListState<Self>>,
    ) -> Task<()> {
        Task::ready(())
    }
}

/// 画出这一页的列表。
pub(crate) fn render(entries: &gpui_kit::Entity<ListState<EntryDelegate>>) -> AnyElement {
    div()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        // 点击由行自己处理（要分单击和双击）；`ListState` 建的时候就把 selectable 关了。
        .child(List::new(entries).px_1().py_1())
        .into_any_element()
}
