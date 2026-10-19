//! 主题 × 亚克力实验。
//!
//! 同一个「亚克力背景」在四种窗口里各开一扇，用来回答一件事：主程序那块不透明白色
//! （深色主题下是黑色）到底是不是主题底色和亚克力打架。
//!
//! 四扇窗：
//!
//! 1. `raw-light`  / `raw-dark`：像 `transparency_probe` 那样直接用 `cx.open_window`，
//!    视图自己不画背景。这是亚克力的本来样子。
//! 2. `root-light` / `root-dark`：走主程序那条路（`gpui_kit::open_window`，外面包了
//!    gpui-component 的 Root 插件），并把 Root 的底色改成半透明。
//!
//! 截图时 BitBlt 能看到「透过窗口的壁纸」，所以：
//! 壁纸色 = 亚克力生效；一片纯白/纯黑 = 被底色盖住了。

use gpui_kit::base::Root;
use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{
    App, Bounds, Context, Hsla, IntoElement, Render, StyleRefinement, Window,
    WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, div, point, prelude::*,
    px, size,
};

/// 四个实验窗的标题，也是 `cap.py` 按标题抓窗口用的键。
const TITLES: [&str; 4] = ["raw-light", "root-light", "raw-dark", "root-dark"];

struct Probe {
    title: &'static str,
    note: &'static str,
}

impl Render for Probe {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_2()
            .child(div().text_lg().child(self.title))
            .child(div().text_sm().child(self.note))
    }
}

/// 一扇实验窗。
///
/// `through_root` 为真时走 `gpui_kit::open_window`（带 Root 插件），并把 Root 的底色
/// 调成半透明；为假时像探针那样直接开窗、自己不画背景。
fn open_probe(
    cx: &mut App,
    title: &'static str,
    note: &'static str,
    dark: bool,
    through_root: bool,
    x: f32,
    y: f32,
) {
    let bounds = Bounds::new(point(px(x), px(y)), size(px(300.), px(200.)));
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: None,
        kind: WindowKind::PopUp,
        window_background: WindowBackgroundAppearance::Blurred,
        show: true,
        focus: false,
        ..Default::default()
    };

    if through_root {
        let opened = gpui_kit::open_window(options, cx, |window, cx| {
            Theme::change(
                if dark {
                    ThemeMode::Dark
                } else {
                    ThemeMode::Light
                },
                Some(window),
                cx,
            );
            window.set_window_title(title);
            cx.new(|_| Probe { title, note })
        });
        match opened {
            Ok((handle, _view)) => {
                // 关键一步：把 Root（gpui-component 插件）铺的那层主题底色改成半透明，
                // 否则它实色盖在整个窗口上，亚克力永远看不见。
                // Root 是 open_window 返回之后才建好的，所以这一步必须在窗口创建完之后做。
                let tint = if dark {
                    Hsla {
                        h: 0.,
                        s: 0.,
                        l: 0.07,
                        a: 0.72,
                    }
                } else {
                    Hsla {
                        h: 0.,
                        s: 0.,
                        l: 1.,
                        a: 0.72,
                    }
                };
                let _ = handle.update(cx, |_, window, cx| {
                    Root::update(window, cx, |root, _window, _cx| {
                        root.style().refine(&StyleRefinement::default().bg(tint));
                    });
                });
            }
            Err(error) => eprintln!("gpui-tools: {title} 开窗失败：{error}"),
        }
    } else {
        let _ = cx.open_window(options, |window, cx| {
            Theme::change(
                if dark {
                    ThemeMode::Dark
                } else {
                    ThemeMode::Light
                },
                Some(window),
                cx,
            );
            window.set_window_title(title);
            cx.new(|_| Probe { title, note })
        });
    }
}

fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx: &mut App| {
            gpui_kit::init(cx);

            let notes = [
                "直接开窗，不画背景：亚克力本来该有的样子",
                "走主程序的路，Root 底色调成半透明",
                "直接开窗，不画背景：亚克力本来该有的样子",
                "走主程序的路，Root 底色调成半透明",
            ];
            for (index, (title, note)) in TITLES.iter().zip(notes).enumerate() {
                let dark = index >= 2;
                let through_root = index % 2 == 1;
                open_probe(
                    cx,
                    title,
                    note,
                    dark,
                    through_root,
                    120. + 340. * (index % 2) as f32,
                    80. + 260. * (index / 2) as f32,
                );
            }
        });
}
