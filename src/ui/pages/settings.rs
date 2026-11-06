//! 设置页：左边是子模块，右边是选中那一页的内容。

use gpui_kit::component::input::Input;
use gpui_kit::component::slider::Slider;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Entity, IntoElement, MouseDownEvent, Pixels,
    SharedString, Window, div, prelude::*, px,
};
use tools::firefox;
use tools::matcher::MatchKind;
use tools::settings::{HomeLayout, Settings, Theme as ColorScheme, ToolKind};

use crate::actions;
use crate::app::stored_settings;
use crate::shortcuts::shortcut_label;
use crate::strings;
use crate::ui::colors::UiColors;
use crate::ui::commands_page;
use crate::ui::commands_page::shortcut_button;
use crate::ui::launcher::{
    Launcher, MODULE_WIDTH, RecordTarget, SIDEBAR_WIDTH, SettingsTab, ToolTab,
};

/// 画出整个设置页。
pub(crate) fn render(
    launcher: &Launcher,
    tab: SettingsTab,
    cx: &Context<Launcher>,
) -> impl IntoElement {
    let colors = UiColors::current(cx);
    let settings = stored_settings(cx);
    let border = cx.theme().border;

    let sections =
        v_flex()
            .w(px(SIDEBAR_WIDTH))
            .flex_shrink_0()
            .p_2()
            .gap_1()
            .border_r_1()
            .border_color(border)
            .children(SettingsTab::ALL.into_iter().enumerate().map(|(ix, item)| {
                let selected = item == tab;
                h_flex()
                    .id(SharedString::from(format!("section-tab-{ix}")))
                    .gap_2()
                    .px_3()
                    .py_2()
                    .rounded(px(6.))
                    .text_sm()
                    .text_color(if selected {
                        colors.accent_foreground
                    } else {
                        colors.foreground
                    })
                    .cursor_pointer()
                    .when(selected, move |this| this.bg(colors.accent))
                    .when(!selected, move |this| {
                        this.hover(move |style| style.bg(colors.hover))
                    })
                    .on_click(cx.listener(move |launcher, _, window, cx| {
                        launcher.select_tab(item, window, cx)
                    }))
                    .child(Icon::new(item.icon()).size_4().text_color(if selected {
                        colors.accent_foreground
                    } else {
                        colors.muted_foreground
                    }))
                    .child(div().child(item.label()))
            }));

    let content = match tab {
        SettingsTab::System => system_page(launcher, settings, colors, cx),
        SettingsTab::Commands => commands_page::render(launcher, colors, cx).into_any_element(),
        SettingsTab::Appearance => appearance_page(launcher, settings, colors, cx),
        // 「工具」是容器标签：它自己不画设置项，而是把计算器等工具收在下面的
        // 子标签页里，布局和命令页一样（左列列工具，右列列内容）。
        SettingsTab::Tools => tools_page(launcher, colors, cx),
        SettingsTab::Matchers => matchers_page(settings, colors, cx),
        SettingsTab::About => about_page(colors, cx),
    };

    // `h_flex` 默认在交叉轴居中，这样会把整页浮在面板中间；这里要顶对齐。
    //
    // **必须是 `h_full` 而不是 `flex_1`**：外层 `body` 是 `div()`，而 gpui 的 `div` 默认
    // `display: block`，不是 flex 容器——`flex_1` 在这儿什么也不做，整页会按内容长到
    // 2904px 高、被 `overflow_hidden` 裁掉，右侧那一列于是永远「和内容一样高」，
    // 没有溢出也就没有滚动（`max_offset` 恒为 0）。
    h_flex()
        .h_full()
        .flex_1()
        .min_h_0()
        .min_w_0()
        .items_stretch()
        .overflow_hidden()
        .child(sections)
        .child(
            // 面板可以被拖大拖小，内容比窗口高时就地滚，绝不给裁掉半截。
            v_flex()
                .id("settings-content")
                .flex_1()
                .min_w_0()
                .min_h_0()
                // 只用分轴的两个：和主页方格区完全一致的写法。`overflow_hidden()` 会把两个轴
                // 一起设成 Hidden 再加一层裁剪，跟滚动容器混着用容易两端都不灵。
                .overflow_x_hidden()
                .overflow_y_scroll()
                // 和主页方格区一样的写法：挂句柄，滚轮交给 gpui 自己处理。
                // **不要再自己接 `on_scroll_wheel`**：那会跟内置的滚动处理抢同一个偏移量，
                // 两边各写一次，结果谁也滚不动（日志里 `当前` 一直卡在同一个值）。
                .track_scroll(&launcher.settings_scroll)
                .px_4()
                .py_4()
                // **内容层不许被压缩**：gpui 的 `flex_shrink` 默认是 1，直接放进去会被压扁去
                // 适应容器高度——永远不「超出」，也就永远滚不动。包一层 `flex_shrink_0`，
                // 它的高度按内容算，溢出才是真的溢出。
                .child(div().w_full().flex_shrink_0().child(content)),
        )
}

/// 工具标签：左边列出全部工具子标签，右边画选中那一项的配置。
///
/// 布局和命令页一致（左列列子标签，右列列内容）；子标签都点选，不用键盘。
fn tools_page(launcher: &Launcher, colors: UiColors, cx: &Context<Launcher>) -> AnyElement {
    let border = colors.border;
    let selected = launcher.tool_tab;

    let tabs = v_flex()
        .w(px(MODULE_WIDTH))
        .flex_shrink_0()
        .gap_1()
        .pr_3()
        .border_r_1()
        .border_color(border)
        .children(ToolTab::ALL.iter().map(|tool| {
            let current = *tool == selected;
            h_flex()
                .id(SharedString::from(format!("tool-tab-{}", tool.label())))
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
                    this.hover(move |style| style.bg(colors.hover))
                })
                .on_click(cx.listener(move |launcher, _, window, cx| {
                    launcher.select_tool_tab(*tool, window, cx)
                }))
                .child(Icon::new(tool.icon()).size_4())
                .child(div().flex_1().min_w_0().text_ellipsis().child(tool.label()))
        }));

    let content = match selected {
        ToolTab::Calculator => calculator_settings_page(stored_settings(cx), colors, cx),
        ToolTab::Firefox => firefox_settings_page(launcher, colors, cx),
        ToolTab::Github => github_settings_page(launcher, colors, cx),
    };

    h_flex()
        .size_full()
        .min_w_0()
        .items_stretch()
        .overflow_hidden()
        .child(tabs)
        .child(
            v_flex()
                .id("tool-content")
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .overflow_y_scroll()
                .pl_3()
                .gap_1()
                .child(
                    div()
                        .px_2()
                        .pb_1()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(strings::settings_page::tool_caption(selected.label())),
                )
                .child(content),
        )
        .into_any_element()
}

/// 系统页。
fn system_page(
    launcher: &Launcher,
    settings: Settings,
    colors: UiColors,
    cx: &Context<Launcher>,
) -> AnyElement {
    v_flex()
        .gap_1()
        .child(settings_row(
            strings::settings_page::HOTKEY_NAME,
            strings::settings_page::HOTKEY_DESCRIPTION,
            colors,
            summon_button(launcher, settings.summon_shortcut.as_deref(), colors, cx),
        ))
        .child(settings_row(
            strings::settings_page::AUTOSTART_NAME,
            // 管理员模式下开机启动是那条计划任务的登录触发器在管，说明得跟着变，
            // 否则用户会以为「开机启动」这一项自己在写注册表。
            if settings.run_as_admin {
                strings::settings_page::AUTOSTART_BY_TASK
            } else {
                strings::settings_page::AUTOSTART_DESCRIPTION
            },
            colors,
            Switch::new("launch-at-login")
                .checked(settings.launch_at_login)
                .on_change(cx.listener(|launcher, checked: &bool, _window, cx| {
                    launcher.set_launch_at_login(*checked, cx)
                })),
        ))
        // 管理员模式：装/卸那条计划任务时占位的那句话顶掉原来的说明；
        // 任务建好了、但眼前这个进程还不是管理员时也要说清楚——不然用户会以为没生效。
        .child(settings_row(
            strings::settings_page::ADMIN_NAME,
            launcher.admin_notice.unwrap_or({
                if settings.run_as_admin && !tools::admin::is_elevated() {
                    strings::settings_page::ADMIN_NOT_ELEVATED
                } else {
                    strings::settings_page::ADMIN_DESCRIPTION
                }
            }),
            colors,
            h_flex()
                .gap_2()
                .items_center()
                .when(
                    settings.run_as_admin && !tools::admin::is_elevated(),
                    |this| {
                        this.child(small_button(
                            SharedString::from("admin-restart"),
                            strings::settings_page::ADMIN_RESTART.into(),
                            colors,
                            false,
                            cx.listener(|_launcher, _, _, cx| crate::actions::restart_launcher(cx)),
                        ))
                    },
                )
                .child(
                    Switch::new("run-as-admin")
                        .checked(settings.run_as_admin)
                        .on_change(cx.listener(|launcher, checked: &bool, _window, cx| {
                            launcher.set_run_as_admin(*checked, cx)
                        })),
                ),
        ))
        .child(settings_row(
            strings::settings_page::RESUME_NAME,
            strings::settings_page::RESUME_DESCRIPTION,
            colors,
            segmented(
                "resume",
                Settings::RESUME_CHOICES
                    .into_iter()
                    .map(|seconds| (seconds, strings::settings_page::resume_label(seconds))),
                settings.resume_seconds,
                colors,
                cx,
                |launcher, seconds, cx| launcher.set_resume_seconds(seconds, cx),
            ),
        ))
        .child(action_row(
            "open-config",
            strings::settings_page::CONFIG_FOLDER,
            crate::app::about_directory(),
            strings::settings_page::CONFIG_OPEN,
            colors,
            cx.listener(|_, _, _, cx| actions::open_settings_folder(cx)),
        ))
        .child(action_row(
            "quit",
            strings::settings_page::QUIT_NAME,
            strings::settings_page::QUIT_DESCRIPTION,
            strings::settings_page::QUIT_ACTION,
            colors,
            cx.listener(|_, _, _, cx| actions::quit_launcher(cx)),
        ))
        .into_any_element()
}

/// 唤出面板的那个热键：和命令页的快捷键按钮一模一样，只是记到唤出热键上。
fn summon_button(
    launcher: &Launcher,
    chord: Option<&str>,
    colors: UiColors,
    cx: &Context<Launcher>,
) -> impl IntoElement {
    let recording = launcher.recording_summon();
    let label: SharedString = if recording {
        launcher
            .recording_conflict()
            .map_or_else(
                || strings::commands_page::RECORDING.to_string(),
                str::to_string,
            )
            .into()
    } else {
        chord
            .map_or_else(
                || strings::settings_page::HOTKEY_UNBOUND.to_string(),
                shortcut_label,
            )
            .into()
    };
    shortcut_button(
        SharedString::from("summon-shortcut"),
        label,
        recording,
        colors,
        px(240.),
        cx.listener(|launcher, _, _, cx| launcher.toggle_summon_recording(cx)),
        cx.listener(|launcher, _: &MouseDownEvent, _, cx| {
            launcher.clear_shortcut(RecordTarget::Summon, cx)
        }),
    )
}

/// 外观页。
fn appearance_page(
    launcher: &Launcher,
    settings: Settings,
    colors: UiColors,
    cx: &Context<Launcher>,
) -> AnyElement {
    v_flex()
        .gap_1()
        .child(settings_row(
            strings::settings_page::THEME_NAME,
            strings::settings_page::THEME_DESCRIPTION,
            colors,
            segmented(
                "theme",
                ColorScheme::ALL
                    .into_iter()
                    .map(|theme| (theme, theme_label(theme))),
                settings.theme,
                colors,
                cx,
                |launcher, choice, cx| launcher.choose_theme(choice, None, cx),
            ),
        ))
        .child(settings_row(
            strings::settings_page::LAYOUT_NAME,
            strings::settings_page::LAYOUT_DESCRIPTION,
            colors,
            segmented(
                "home-layout",
                HomeLayout::ALL
                    .into_iter()
                    .map(|layout| (layout, layout_label(layout))),
                settings.home_layout,
                colors,
                cx,
                |launcher, choice, cx| launcher.set_home_layout(choice, cx),
            ),
        ))
        .child(settings_row(
            strings::settings_page::TRANSLUCENT_NAME,
            strings::settings_page::TRANSLUCENT_DESCRIPTION,
            colors,
            Switch::new("translucent-background")
                .checked(settings.translucent)
                .on_change(cx.listener(|launcher, checked: &bool, _, cx| {
                    launcher.set_translucent(*checked, cx)
                })),
        ))
        .child(settings_row(
            strings::settings_page::OPACITY_NAME,
            strings::settings_page::OPACITY_DESCRIPTION,
            colors,
            h_flex()
                .flex_shrink_0()
                .items_center()
                .gap_3()
                .child(
                    // 滑动条的状态挂在面板上，所以拖动的每一帧都直接变成面板的染色深浅。
                    Slider::new(&launcher.opacity).w(px(160.)).flex_shrink_0(),
                )
                .child(
                    div()
                        .w(px(48.))
                        .text_center()
                        .text_sm()
                        .text_color(colors.foreground)
                        .child(strings::settings_page::opacity_value(
                            settings.panel_opacity,
                        )),
                ),
        ))
        .child(settings_row(
            strings::settings_page::HEIGHT_NAME,
            strings::settings_page::HEIGHT_DESCRIPTION,
            colors,
            stepper(
                "search-height",
                settings.search_height,
                StepperRange {
                    lowest: Settings::SEARCH_HEIGHT_MIN,
                    highest: Settings::SEARCH_HEIGHT_MAX,
                    step: Settings::SEARCH_HEIGHT_STEP,
                    label: strings::settings_page::height_value(settings.search_height).into(),
                },
                colors,
                cx,
                |launcher, height, cx| launcher.set_search_height(height, cx),
            ),
        ))
        .into_any_element()
}

/// 关于页。
fn about_page(colors: UiColors, cx: &Context<Launcher>) -> AnyElement {
    v_flex()
        .gap_1()
        .child(settings_row(
            strings::settings_page::VERSION_NAME,
            strings::settings_page::VERSION_DESCRIPTION,
            colors,
            value_label(env!("CARGO_PKG_VERSION"), colors),
        ))
        .child(action_row(
            "open-repository",
            strings::settings_page::REPOSITORY_NAME,
            actions::REPOSITORY_URL,
            strings::settings_page::REPOSITORY_OPEN,
            colors,
            cx.listener(|_, _, _, cx| actions::open_repository(cx)),
        ))
        .child(action_row(
            "open-folder",
            strings::settings_page::PROGRAM_FOLDER_NAME,
            actions::program_dir().display().to_string(),
            strings::settings_page::FOLDER_OPEN,
            colors,
            cx.listener(|_, _, _, cx| actions::open_program_folder(cx)),
        ))
        .into_any_element()
}

/// 计算器设置页：计算器本体的设置，目前只有结果的精度。
fn calculator_settings_page(
    settings: Settings,
    colors: UiColors,
    cx: &Context<Launcher>,
) -> AnyElement {
    v_flex()
        .gap_1()
        .child(settings_row(
            strings::settings_page::CALCULATOR_NAME,
            strings::settings_page::CALCULATOR_DESCRIPTION,
            colors,
            stepper(
                "calculator-precision",
                settings.calculator_precision,
                StepperRange {
                    lowest: Settings::CALCULATOR_PRECISION_MIN,
                    highest: Settings::CALCULATOR_PRECISION_MAX,
                    step: Settings::CALCULATOR_PRECISION_STEP,
                    label: strings::settings_page::precision_value(settings.calculator_precision)
                        .into(),
                },
                colors,
                cx,
                |launcher, precision, cx| launcher.set_calculator_precision(precision, cx),
            ),
        ))
        .into_any_element()
}

/// 火狐系浏览器的设置：profile 目录在哪儿。
///
/// 留空就自动探测（Firefox / Zen / LibreWolf / Waterfox / Floorp 的默认 profile），
/// 右边那行小字写的是「现在实际用的是哪一个」——用户要知道程序到底读了谁的库。
fn firefox_settings_page(
    launcher: &Launcher,
    colors: UiColors,
    cx: &Context<Launcher>,
) -> AnyElement {
    let settings = stored_settings(cx);
    let resolved = firefox::resolve_profile(settings.firefox_profile.as_deref())
        .map(|path| path.display().to_string());
    let description = strings::settings_page::firefox_profile_value(resolved.as_deref());

    v_flex()
        .gap_1()
        .child(settings_row(
            strings::settings_page::FIREFOX_PROFILE_NAME,
            description,
            colors,
            h_flex()
                .flex_shrink_0()
                .items_center()
                .gap_2()
                .child(text_field(&launcher.firefox_input, px(190.)))
                .child(text_button(
                    "firefox-auto",
                    strings::common::AUTO,
                    colors,
                    cx.listener(|launcher, _, window, cx| {
                        launcher.clear_firefox_profile(window, cx)
                    }),
                )),
        ))
        .into_any_element()
}

/// GitHub 的设置：用户名和令牌。两行都在失焦或按回车时自动保存。
fn github_settings_page(
    launcher: &Launcher,
    colors: UiColors,
    _cx: &Context<Launcher>,
) -> AnyElement {
    v_flex()
        .gap_1()
        .child(settings_row(
            strings::settings_page::GITHUB_USER_NAME,
            strings::settings_page::GITHUB_USER_DESCRIPTION,
            colors,
            text_field(&launcher.github_user_input, px(200.)),
        ))
        .child(settings_row(
            strings::settings_page::GITHUB_TOKEN_NAME,
            strings::settings_page::GITHUB_TOKEN_DESCRIPTION,
            colors,
            text_field(&launcher.github_token_input, px(200.)),
        ))
        .into_any_element()
}

/// 编辑区：名字一行、路径（命令）一行，终端再多一行起始目录，下面是保存/取消。
pub(crate) fn tool_editor_block(
    launcher: &Launcher,
    colors: UiColors,
    cx: &Context<Launcher>,
) -> AnyElement {
    let Some(editor) = launcher.tool_editor.as_ref() else {
        return div().into_any_element();
    };
    let kind = editor.kind;
    let (target_label, pick_label) = match kind {
        ToolKind::File => (
            strings::tools_page::EDIT_PATH,
            strings::tools_page::PICK_FILE,
        ),
        ToolKind::Folder => (
            strings::tools_page::EDIT_FOLDER,
            strings::tools_page::PICK_FOLDER,
        ),
        ToolKind::Terminal => (
            strings::tools_page::EDIT_COMMAND,
            strings::tools_page::PICK_FOLDER,
        ),
    };

    v_flex()
        .gap_2()
        .p_3()
        .mb_1()
        .rounded(px(8.))
        .bg(colors.control)
        .child(div().text_xs().text_color(colors.muted_foreground).child(
            strings::tools_page::editor_heading(kind, editor.id.is_some()),
        ))
        .child(
            h_flex()
                .w_full()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .w(px(56.))
                        .flex_shrink_0()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(strings::tools_page::EDIT_NAME),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(filled_text_field(&editor.name)),
                ),
        )
        .child(
            h_flex()
                .w_full()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .w(px(56.))
                        .flex_shrink_0()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(target_label),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(filled_text_field(&editor.target)),
                )
                // 终端不挑文件，只有文件和文件夹条目才给这枚「选择…」。
                .when(kind.is_path(), |this| {
                    this.child(div().flex_shrink_0().child(small_button(
                        SharedString::from("tool-editor-pick"),
                        pick_label.into(),
                        colors,
                        false,
                        cx.listener(|launcher, _, _, cx| launcher.pick_editor_target(cx)),
                    )))
                }),
        )
        // 终端的工作目录：留空就是用户文件夹。
        .when(kind == ToolKind::Terminal, |this| {
            this.child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(56.))
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(strings::tools_page::EDIT_START_DIR),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(filled_text_field(&editor.start_dir)),
                    )
                    .child(div().flex_shrink_0().child(small_button(
                        SharedString::from("tool-editor-start-dir"),
                        strings::tools_page::PICK_FOLDER.into(),
                        colors,
                        false,
                        cx.listener(|launcher, _, _, cx| launcher.pick_editor_start_dir(cx)),
                    ))),
            )
        })
        .child(
            h_flex()
                .w_full()
                .items_center()
                .gap_2()
                .child(small_button(
                    SharedString::from("tool-editor-save"),
                    strings::tools_page::SAVE.into(),
                    colors,
                    true,
                    cx.listener(|launcher, _, window, cx| launcher.save_tool_editor(window, cx)),
                ))
                .child(small_button(
                    SharedString::from("tool-editor-cancel"),
                    strings::tools_page::CANCEL.into(),
                    colors,
                    false,
                    cx.listener(|launcher, _, _, cx| launcher.cancel_tool_editor(cx)),
                ))
                .child(div().flex_1())
                .when_some(launcher.tool_notice, |this, notice| {
                    this.child(
                        div()
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(notice),
                    )
                }),
        )
        .into_any_element()
}

/// 一枚小按钮：`primary` 时是实心的（保存、新增），否则只是一圈淡边框。
/// 页头那一排控件（分段选择、小按钮）的统一高度。
///
/// 页头里控件挨着排，高度差一两个像素就会显得歪；宽度同理，所以成对的按钮用
/// [`small_button_fixed`] 定宽。以「新增」按钮为准——它是这排里最窄的基准。
pub(crate) const HEADER_CONTROL_HEIGHT: f32 = 26.;

pub(crate) fn small_button(
    id: SharedString,
    label: SharedString,
    colors: UiColors,
    primary: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    small_button_inner(id, label, None, colors, primary, on_click)
}

/// 定宽的小按钮：文字长短不一样时，页头那排不会跟着左右抖。
pub(crate) fn small_button_fixed(
    id: SharedString,
    label: SharedString,
    width: f32,
    colors: UiColors,
    primary: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    small_button_inner(id, label, Some(width), colors, primary, on_click)
}

fn small_button_inner(
    id: SharedString,
    label: SharedString,
    width: Option<f32>,
    colors: UiColors,
    primary: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .flex_shrink_0()
        .h(px(HEADER_CONTROL_HEIGHT))
        .when_some(width, |this, width| this.w(px(width)))
        .flex()
        .items_center()
        .justify_center()
        .px_2()
        .rounded(px(6.))
        .border_1()
        .border_color(if primary {
            colors.accent
        } else {
            colors.surface
        })
        .text_xs()
        .cursor_pointer()
        .text_color(if primary {
            colors.accent_foreground
        } else {
            colors.on_surface_muted
        })
        .when(primary, move |this| this.bg(colors.accent))
        .when(!primary, move |this| {
            this.hover(move |style| style.bg(colors.hover))
        })
        .on_click(on_click)
        .child(label)
}

/// 设置行右边的一个文本框：固定宽度、小号，和别的控件高度对得上。
fn text_field(
    input: &Entity<gpui_kit::component::input::InputState>,
    width: Pixels,
) -> impl IntoElement {
    div()
        .w(width)
        .h(px(28.))
        .flex()
        .items_center()
        .child(Input::new(input).small())
}

/// 铺满一行（不限宽）的文本框，给编辑区用。
fn filled_text_field(input: &Entity<gpui_kit::component::input::InputState>) -> impl IntoElement {
    div()
        .w_full()
        .h(px(28.))
        .flex()
        .items_center()
        .child(Input::new(input).small())
}

/// 一枚朴素的文字按钮（设置行右侧那种「自动」「打开」）。
fn text_button(
    id: &'static str,
    label: &'static str,
    colors: UiColors,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .px_2()
        .py_1()
        .rounded(px(6.))
        .border_1()
        .border_color(colors.surface)
        .text_xs()
        .cursor_pointer()
        .text_color(colors.on_surface_muted)
        .hover(move |style| style.bg(colors.hover))
        .on_click(on_click)
        .child(label)
}

/// 指令匹配页：四个匹配各一行开关。
fn matchers_page(settings: Settings, colors: UiColors, cx: &Context<Launcher>) -> AnyElement {
    v_flex()
        .gap_1()
        .child(settings_row(
            strings::matcher::CALCULATOR_NAME,
            strings::matcher::CALCULATOR_DESCRIPTION,
            colors,
            Switch::new("matcher-calculator")
                .checked(settings.matcher_calculator)
                .on_change(cx.listener(|launcher, checked: &bool, window, cx| {
                    launcher.set_matcher(MatchKind::Calculator, *checked, window, cx)
                })),
        ))
        .child(settings_row(
            strings::matcher::AMOUNT_NAME,
            strings::matcher::AMOUNT_DESCRIPTION,
            colors,
            Switch::new("matcher-amount")
                .checked(settings.matcher_amount)
                .on_change(cx.listener(|launcher, checked: &bool, window, cx| {
                    launcher.set_matcher(MatchKind::Amount, *checked, window, cx)
                })),
        ))
        .child(settings_row(
            strings::matcher::URL_NAME,
            strings::matcher::URL_DESCRIPTION,
            colors,
            Switch::new("matcher-url")
                .checked(settings.matcher_url)
                .on_change(cx.listener(|launcher, checked: &bool, window, cx| {
                    launcher.set_matcher(MatchKind::Url, *checked, window, cx)
                })),
        ))
        .child(settings_row(
            strings::matcher::PATH_NAME,
            strings::matcher::PATH_DESCRIPTION,
            colors,
            Switch::new("matcher-path")
                .checked(settings.matcher_path)
                .on_change(cx.listener(|launcher, checked: &bool, window, cx| {
                    launcher.set_matcher(MatchKind::Path, *checked, window, cx)
                })),
        ))
        .into_any_element()
}

/// 主题选项的名字。
/// 主页排版那两个选项的名字。
fn layout_label(layout: HomeLayout) -> &'static str {
    match layout {
        HomeLayout::List => strings::settings_page::LAYOUT_LIST,
        HomeLayout::Grid => strings::settings_page::LAYOUT_GRID,
    }
}

fn theme_label(theme: ColorScheme) -> &'static str {
    match theme {
        ColorScheme::System => strings::settings_page::THEME_SYSTEM,
        ColorScheme::Light => strings::settings_page::THEME_LIGHT,
        ColorScheme::Dark => strings::settings_page::THEME_DARK,
    }
}

/// 一行设置：左边是名字和说明，右边是控件。
///
/// 每个设置项都是这副骨架，控件换成什么都行，名字和说明的位置永远一致。
///
/// 子项目基础布局；具体可点击区域由各控件自身负责。
pub(crate) fn settings_row(
    name: &'static str,
    description: impl Into<SharedString>,
    colors: UiColors,
    control: impl IntoElement,
) -> gpui_kit::Div {
    h_flex()
        .w_full()
        .min_w_0()
        .justify_between()
        .items_center()
        .gap_4()
        .px_3()
        .py_2()
        .rounded(px(6.))
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

/// 只读行右侧显示的那个值。
pub(crate) fn value_label(value: impl Into<SharedString>, colors: UiColors) -> impl IntoElement {
    div()
        .text_sm()
        .text_color(colors.muted_foreground)
        .child(value.into())
}

/// 点一下就执行的那种设置行。
pub(crate) fn action_row(
    id: &'static str,
    name: &'static str,
    description: impl Into<SharedString>,
    hint: &'static str,
    colors: UiColors,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    settings_row(
        name,
        description,
        colors,
        h_flex()
            .id(id)
            .gap_1()
            .py_1()
            .rounded(px(6.))
            .cursor_pointer()
            .hover(move |style| style.bg(colors.hover))
            .on_click(on_click)
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
}

/// 从几个值里挑一个的控件，画成一串胶囊。
pub(crate) fn segmented<T: Copy + PartialEq + 'static>(
    id: &'static str,
    options: impl IntoIterator<Item = (T, &'static str)>,
    selected: T,
    colors: UiColors,
    cx: &Context<Launcher>,
    on_select: impl Fn(&mut Launcher, T, &mut Context<Launcher>) + Copy + 'static,
) -> impl IntoElement {
    segmented_with(id, options, selected, colors, cx, on_select, false)
}

/// 页头用的紧凑版：高度对齐 [`HEADER_CONTROL_HEIGHT`]，和小按钮并排时一样高。
///
/// 设置页里的分段控件不动：那边一行只有它一个控件，用常规字号更好看。
pub(crate) fn segmented_compact<T: Copy + PartialEq + 'static>(
    id: &'static str,
    options: impl IntoIterator<Item = (T, &'static str)>,
    selected: T,
    colors: UiColors,
    cx: &Context<Launcher>,
    on_select: impl Fn(&mut Launcher, T, &mut Context<Launcher>) + Copy + 'static,
) -> impl IntoElement {
    segmented_with(id, options, selected, colors, cx, on_select, true)
}

fn segmented_with<T: Copy + PartialEq + 'static>(
    id: &'static str,
    options: impl IntoIterator<Item = (T, &'static str)>,
    selected: T,
    colors: UiColors,
    cx: &Context<Launcher>,
    on_select: impl Fn(&mut Launcher, T, &mut Context<Launcher>) + Copy + 'static,
    compact: bool,
) -> impl IntoElement {
    h_flex()
        .gap_0p5()
        .p(px(if compact { 3. } else { 2. }))
        .when(compact, |this| this.h(px(HEADER_CONTROL_HEIGHT)))
        .rounded(px(8.))
        .bg(colors.control)
        .children(
            options
                .into_iter()
                .enumerate()
                .map(move |(ix, (value, label))| {
                    let current = value == selected;
                    div()
                        .id(SharedString::from(format!("{id}-{ix}")))
                        .px_2()
                        .when(compact, |this| {
                            // 紧凑版：外层高度是定死的，里面的胶囊撑满，字居中。
                            this.h_full().flex().items_center().text_xs()
                        })
                        .when(!compact, |this| this.py_1().text_sm())
                        .rounded(px(6.))
                        .text_color(if current {
                            colors.accent_foreground
                        } else {
                            // 未选中的胶囊画在不透明的 `control` 底上，用
                            // `on_surface_muted`，不随半透明面板的醒目度校验走。
                            colors.on_surface_muted
                        })
                        .cursor_pointer()
                        .when(current, move |this| this.bg(colors.accent))
                        .when(!current, move |this| {
                            this.hover(move |style| style.bg(colors.hover))
                        })
                        .on_click(cx.listener(move |launcher, _: &ClickEvent, _window, cx| {
                            on_select(launcher, value, cx)
                        }))
                        .child(label)
                }),
        )
}

/// 数值步进控件：减、值、加。
/// 步进控件的档位：下限、上限、步长，以及中间显示的字。
pub(crate) struct StepperRange {
    pub(crate) lowest: u32,
    pub(crate) highest: u32,
    pub(crate) step: u32,
    pub(crate) label: SharedString,
}

pub(crate) fn stepper(
    id: &'static str,
    value: u32,
    range: StepperRange,
    colors: UiColors,
    cx: &Context<Launcher>,
    on_change: impl Fn(&mut Launcher, u32, &mut Context<Launcher>) + Copy + 'static,
) -> impl IntoElement {
    let StepperRange {
        lowest,
        highest,
        step,
        label,
    } = range;
    let step = step as isize;
    let at_bottom = value <= lowest;
    let at_top = value >= highest;

    let button = move |ix: usize, icon: IconName, disabled: bool| {
        let button = div()
            .id(SharedString::from(format!("{id}-{ix}")))
            .size_6()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(6.))
            .text_color(colors.foreground);
        if disabled {
            return button.opacity(crate::ui::colors::DISABLED_OPACITY);
        }
        let delta = if ix == 0 { -step } else { step };
        let next = (value as isize + delta).clamp(lowest as isize, highest as isize) as u32;
        button
            .cursor_pointer()
            .hover(move |style| style.bg(colors.hover))
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
                .child(label),
        )
        .child(button(1, IconName::Plus, at_top))
}
