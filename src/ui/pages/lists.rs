//! 工具列表页：书签、历史、个人项目、星标、查询仓库共用这一张列表。
//!
//! 五个页面的行长得一样（图标 + 标题 + 副行 + 右侧元信息），差别只在数据从哪儿来、
//! 回车干什么，所以只有一份 `Entry` 和一份 delegate。数据由 `launcher` 装进来
//! （见 `Launcher::open_list` 及几个 `load_*`），这里只管画和打开。

use std::sync::Arc;

use gpui_kit::component::list::{List, ListDelegate, ListState};
use gpui_kit::component::{Icon, IndexPath, Selectable, h_flex, v_flex};
// 图标用全集：书签、历史这些不在组件自带的那一小套里。
use gpui_kit::assets::IconName;
use gpui_kit::{
    AnyElement, Context, FontWeight, Image, IntoElement, ObjectFit, RenderOnce, SharedString, Task,
    Window, div, img, prelude::*, px,
};
use gpui_tools::firefox::{Favicon, IconKind};
use gpui_tools::shell;

use crate::strings;
use crate::ui::colors::UiColors;
use crate::ui::launcher::ListKind;

/// 列表里的一行。
pub(crate) struct Entry {
    pub(crate) title: String,
    /// 副行：网址，或者仓库简介。
    pub(crate) detail: String,
    /// 右侧那点信息：访问次数、星数、主语言。
    pub(crate) meta: String,
    /// 回车/点一下要打开的东西，一律交给 shell 解析。
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

        div().w_full().py(px(1.)).child(
            h_flex()
                .id(SharedString::from(format!("entry-row-{}", self.index)))
                .w_full()
                .h(px(50.))
                .px_3()
                .gap_3()
                .rounded(px(8.))
                .when(selected, move |this| this.bg(colors.accent))
                .when(!selected, move |this| {
                    this.hover(move |style| style.bg(colors.hover))
                })
                .child(tile)
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
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(title_color)
                                .child(self.entry.title),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .overflow_hidden()
                                .text_ellipsis()
                                .text_xs()
                                .text_color(detail_color)
                                .child(self.entry.detail),
                        ),
                )
                .when(!self.entry.meta.is_empty(), move |this| {
                    this.child(
                        div()
                            .flex_shrink_0()
                            .max_w(px(200.))
                            .overflow_hidden()
                            .text_ellipsis()
                            .text_xs()
                            .text_color(detail_color)
                            .child(self.entry.meta),
                    )
                }),
        )
    }
}

/// 列表页的数据与状态。
pub(crate) struct EntryDelegate {
    /// 这一页是五页里的哪一页，决定空状态说什么。
    kind: ListKind,
    items: Vec<Entry>,
    selected: usize,
    /// 正在读库或者正在抓网络。
    loading: bool,
    /// 出错时的说明；有内容就顶掉空状态。
    message: Option<String>,
    /// 查询仓库页：有没有搜过（搜过且空才是「没搜到」）。
    searched: bool,
}

impl EntryDelegate {
    pub(crate) fn new(kind: ListKind) -> Self {
        Self {
            kind,
            items: Vec::new(),
            selected: 0,
            loading: false,
            message: None,
            searched: false,
        }
    }

    /// 换一页重新开始：清空数据、清掉提示。
    pub(crate) fn reset(&mut self, kind: ListKind) {
        self.kind = kind;
        self.items.clear();
        self.selected = 0;
        self.loading = false;
        self.message = None;
        self.searched = false;
    }

    /// 装一批数据进来，高亮回到第一行。
    pub(crate) fn set_items(&mut self, items: Vec<Entry>) {
        self.items = items;
        self.selected = 0;
        self.loading = false;
        self.message = None;
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

    /// 这一页现在有多少条。
    pub(crate) fn len(&self) -> usize {
        self.items.len()
    }

    /// 高亮的是第几行。
    pub(crate) fn selected(&self) -> usize {
        self.selected
    }

    /// 是不是正在读库 / 抓网络。
    pub(crate) fn is_loading(&self) -> bool {
        self.loading
    }

    /// 高亮的那一行。
    pub(crate) fn selected_entry(&self) -> Option<&Entry> {
        self.items.get(self.selected)
    }

    /// 打开高亮那一行。
    pub(crate) fn open_selected(&self) {
        if let Some(entry) = self.selected_entry() {
            shell::open(&entry.target);
        }
    }

    /// 列表空着的时候说什么。
    fn empty_text(&self) -> String {
        if self.loading {
            return strings::lists::LOADING.to_string();
        }
        if let Some(message) = &self.message {
            return message.clone();
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
        self.items.len()
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _window: &mut Window,
        _cx: &mut Context<ListState<Self>>,
    ) -> Option<Self::Item> {
        let entry = self.items.get(ix.row)?;
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
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_2()
            .child(Icon::new(IconName::Inbox).size_6().text_color(muted))
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
        .child(List::new(entries).px_1().py_1())
        .into_any_element()
}
