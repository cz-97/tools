//! 计算器页：一行一个公式，右边跟着算出来的数。
//!
//! 规矩都在这几行里：
//! - 结果是从公式**现算**的，不另存一份——所以公式怎么改，结果立刻跟着改；
//! - 最后一行永远是空行，它有内容就在下面补一个（`Launcher::calculator_row_changed`）；
//! - 行高跟着文字长（`TextareaState::auto_grow`），长公式自动换行；
//! - 点数复制，清空本行 / 清空所有在底部。

use gpui_kit::component::button::Button;
use gpui_kit::component::input::Textarea;
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::{Context, FontWeight, IntoElement, SharedString, div, prelude::*, px};

use crate::strings;
use crate::ui::colors::UiColors;
use crate::ui::launcher::{CalculatorRow, Launcher};

/// 画出整个计算器页。
pub(crate) fn render(
    launcher: &Launcher,
    colors: UiColors,
    cx: &Context<Launcher>,
) -> impl IntoElement {
    let rows = launcher
        .calculator
        .iter()
        .map(|row| calculator_row(row, launcher, colors, cx).into_any_element())
        .collect::<Vec<_>>();

    v_flex()
        .size_full()
        .min_w_0()
        .gap_2()
        .child(
            // 行多了就地滚：面板高度是固定的，公式却可以一直加。
            v_flex()
                .id("calculator-rows")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .gap_1()
                .children(rows),
        )
        .child(footer(colors, cx))
}

/// 一行公式：左边是输入框，右边是结果。
fn calculator_row(
    row: &CalculatorRow,
    launcher: &Launcher,
    colors: UiColors,
    cx: &Context<Launcher>,
) -> impl IntoElement {
    let id = row.id;
    h_flex()
        .id(SharedString::from(format!("calculator-row-{id}")))
        .w_full()
        .min_w_0()
        // 结果贴着最后一行文字：长公式折成几行，等号还在末行旁边。
        .items_end()
        .gap_2()
        .px_2()
        .py_1p5()
        .rounded(px(6.))
        .bg(colors.surface)
        .child(div().flex_1().min_w_0().child(
            // 多行、自动长高、自动换行；不要组件自己的底和边——行已经有一层纱了。
            Textarea::new(&row.input).appearance(false).bordered(false),
        ))
        .child(result_label(id, launcher, colors, cx))
}

/// 右边的「= 结果」：有结果就能点，点一下复制走。
fn result_label(
    id: usize,
    launcher: &Launcher,
    colors: UiColors,
    cx: &Context<Launcher>,
) -> impl IntoElement {
    let label: SharedString = if launcher.calculator_copied(id) {
        strings::calculator::COPIED.into()
    } else {
        match launcher.calculator_result(id, cx) {
            Some(value) => format!("= {value}").into(),
            // 算不出（空行、打了一半、除零）就只剩一个等号，也不给点的念想。
            None => "=".into(),
        }
    };
    let copied = launcher.calculator_copied(id);
    let has_value = launcher.calculator_result(id, cx).is_some();
    let text_color = if copied {
        colors.accent_foreground
    } else if has_value {
        colors.foreground
    } else {
        colors.faint
    };

    div()
        .id(SharedString::from(format!("calculator-result-{id}")))
        .flex_shrink_0()
        .px_1()
        .rounded(px(4.))
        .text_sm()
        .font_weight(FontWeight::MEDIUM)
        .text_color(text_color)
        .when(copied, move |this| this.bg(colors.accent))
        .when(!copied && has_value, move |this| {
            this.cursor_pointer()
                .hover(move |style| style.bg(colors.hover))
                .on_click(
                    cx.listener(move |launcher, _, _, cx| launcher.copy_calculator_result(id, cx)),
                )
        })
        .child(label)
}

/// 底部：左边一句说明，右边两个清空按钮。
fn footer(colors: UiColors, cx: &Context<Launcher>) -> impl IntoElement {
    h_flex()
        .flex_shrink_0()
        .justify_between()
        .items_center()
        .gap_2()
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child(strings::calculator::HINT),
        )
        .child(
            h_flex()
                .flex_shrink_0()
                .gap_2()
                .child(
                    Button::new("clear-calculator-row")
                        .label(strings::calculator::CLEAR_ROW)
                        .outline()
                        .small()
                        .on_click(cx.listener(move |launcher, _, window, cx| {
                            launcher.clear_calculator_row(window, cx)
                        })),
                )
                .child(
                    Button::new("clear-calculator")
                        .label(strings::calculator::CLEAR_ALL)
                        .outline()
                        .small()
                        .on_click(cx.listener(move |launcher, _, window, cx| {
                            launcher.clear_calculator(window, cx)
                        })),
                ),
        )
}
