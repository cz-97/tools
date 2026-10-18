use gpui_kit::{
    App, Bounds, Context, IntoElement, Render, Window, WindowBackgroundAppearance, WindowBounds,
    WindowKind, WindowOptions, div, hsla, prelude::*, px, rgb, size,
};

struct TransparencyProbe;

impl Render for TransparencyProbe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .size_full()
            .text_color(rgb(0xffffff))
            .child(div().text_lg().child("Windows Desktop Acrylic probe"))
            .child(div().child("背景由 DWM 绘制；透明区域应呈现亚克力模糊材质。"))
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(
                        div()
                            .w(px(160.))
                            .h(px(36.))
                            .bg(hsla(0., 0., 0., 0.35))
                            .child("半透明黑色对照"),
                    )
                    .child(
                        div()
                            .w(px(160.))
                            .h(px(36.))
                            .bg(rgb(0x2050a0))
                            .child("不透明色块对照"),
                    ),
            )
    }
}

fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx: &mut App| {
            gpui_kit::init(cx);

            let bounds = Bounds::centered(None, size(px(520.), px(220.)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: None,
                    kind: WindowKind::PopUp,
                    // On Windows GPUI maps Blurred to ACCENT_ENABLE_ACRYLICBLURBEHIND.
                    // Transparent instead selects a clear desktop surface with no blur.
                    window_background: WindowBackgroundAppearance::Blurred,
                    show: true,
                    focus: true,
                    is_resizable: false,
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("GPUI Acrylic Probe");
                    cx.new(|_| TransparencyProbe)
                },
            )
            .expect("failed to create transparency probe window");
        });
}
