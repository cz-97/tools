//! 应用外壳：启动流程、全局状态、面板的显示与隐藏、托盘。

pub(crate) mod actions;
pub(crate) mod tray_theme;

use std::sync::Mutex;

use futures::StreamExt as _;

use gpui_kit::base::Root;
use gpui_kit::component::{ActiveTheme as _, Theme, ThemeMode};
use gpui_kit::{
    AnyWindowHandle, App, Bounds, Context, Focusable, Hsla, QuitMode, StyleRefinement,
    Subscription, WeakEntity, Window, WindowBackgroundAppearance, WindowBounds, WindowKind,
    WindowOptions, actions, prelude::*, px, size,
};
use gpui_tools::autostart;
use gpui_tools::instance;
use gpui_tools::logo;
use gpui_tools::settings::{Settings, Theme as ColorScheme};
use gpui_tools::vsync;
use gpui_tools::window::NativeWindow;
use gpui_tray::{Icon as TrayIcon, Tray};

use crate::shortcuts;
use crate::strings;
use crate::ui::launcher::Launcher;

/// 面板窗口的标题，同时也是找窗口句柄用的键。
pub const WINDOW_TITLE: &str = "GPUI Tools Launcher";
/// `build.rs` 嵌进 exe 的图标资源 id，窗口和外壳都用它。
const APP_ICON_ID: u16 = 1;
/// 托盘图标的边长，单位像素。
const TRAY_ICON_SIZE: u32 = 32;
/// 等事件循环转起来再跟 GPUI 对一次窗口尺寸，等多久都行，给足一两帧的余量。
const VIEWPORT_SETTLE: std::time::Duration = std::time::Duration::from_millis(200);
/// 面板收起多久之后才去挂起渲染线程；太短会让「关了马上又开」白挂一次。
const RENDER_SUSPEND_DELAY: std::time::Duration = std::time::Duration::from_secs(3);

actions!(
    gpui_tools,
    [
        ShowLauncher,
        OpenSettings,
        RestartApplication,
        QuitApplication
    ]
);

/// 启动器。
pub fn launch() {
    // **先抢单例，再碰任何别的东西。** 第二个实例不许抢热键、抢托盘、抢数据库；
    // 它只把「亮一下面板」的信号丢给已经在跑的那个，然后立刻退出。
    let Some(singleton) = instance::Instance::acquire() else {
        // 叫不醒也不纠缠：多半是在跑的是个更老的版本，没有这个事件。
        instance::Instance::wake_existing();
        return;
    };

    gpui_kit::application()
        // `Assets` 只内嵌 `default-icons.txt` 那一组图标，计算器图标不在其中，
        // 注册它会让 `Icon::new(Calculator)` 加载失败、图标整片空白。`AllAssets`
        // 内嵌全部 Lucide 图标，程序用到的图标只要不出这个全集就都画得出来。
        .with_assets(gpui_kit::assets::AllAssets)
        .with_quit_mode(QuitMode::Explicit)
        .run(|cx: &mut App| {
            gpui_kit::init(cx);
            cx.set_global(Singleton(Mutex::new(Some(singleton))));

            let mut settings = Settings::load();
            if settings.retain_known_commands(
                crate::catalogue::COMMANDS
                    .iter()
                    .map(|command| (command.section.key(), command.id)),
            ) {
                settings.save();
            }
            // 是否开机启动以注册表为准：用户在任务管理器里删掉那一项后，
            // 设置文件里的镜像不能把它「复活」。
            settings.launch_at_login = autostart::enabled();
            cx.set_global(SettingsStore(settings.clone()));

            // 面板多大由用户拖出来，尺寸记在设置里（逻辑像素）。
            let (panel_width, panel_height) =
                (settings.window_width as f32, settings.window_height as f32);
            let bounds = Bounds::centered(None, size(px(panel_width), px(panel_height)), cx);
            let (window_handle, launcher) = gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    // 这三个选项与喝水提醒窗一致；默认 titlebar 会让 GPUI 走系统标题栏路径。
                    titlebar: None,
                    kind: WindowKind::PopUp,
                    // 边缘拖动由 GPUI 自己算命中区，但样式里的拉伸边框仍要由宿主补上
                    // （见 `NativeWindow::apply_panel_chrome`）。
                    is_resizable: true,
                    window_min_size: Some(size(
                        px(Settings::WINDOW_WIDTH_MIN as f32),
                        px(Settings::WINDOW_HEIGHT_MIN as f32),
                    )),
                    // 亚克力由外观页的「半透明」开关决定，见 `apply_panel_appearance`。
                    window_background: panel_appearance(settings.translucent),
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
            .expect("启动器窗口创建失败");

            // 只拿到 `&mut App` 的回调靠这两个全局句柄回到视图和窗口。
            cx.set_global(LauncherHandle {
                view: launcher.downgrade(),
            });
            cx.set_global(LauncherWindow(window_handle));

            // GPUI 不管窗口尺寸和位置，无边框外壳要自己上，失败也只报告、不致命。
            match NativeWindow::find(WINDOW_TITLE) {
                Some(window) => {
                    if !window.apply_panel_chrome(panel_width, panel_height) {
                        eprintln!("gpui-tools: 无边框外壳没有完全生效");
                    }
                    // Windows 11 的默认圆角：就是系统设置里「圆角」那一档的半径。
                    if !window.round_corners() {
                        // eprintln!("gpui-tools: 系统没采用默认圆角，窗口保持方角");
                    }
                    // 任务栏和 Alt-Tab 的图标取自窗口，GPUI 不管，得自己挂资源。
                    if !window.set_icon(APP_ICON_ID) {
                        eprintln!("gpui-tools: 窗口图标挂载失败");
                    }
                    // 横坐标故意用居中算出来的结果，只恢复记住的顶边。
                    if let Some(y) = settings.window_y
                        && !window.center_x_at(y)
                    {
                        eprintln!("gpui-tools: 面板位置恢复失败");
                    }
                    cx.set_global(LauncherPanel { window });
                }
                None => eprintln!("gpui-tools: 创建后找不到面板窗口，保留系统边框"),
            }

            // 上面改样式、发 SWP_FRAMECHANGED 会重启一次 DWM 边框计算，把创建时设好的
            // 亚克力一起洗掉，所以外壳上完再报一次背景外观。
            apply_panel_appearance(cx);
            // Root 的底色也要在这一刻定下来：默认那层是不透的主题色，会盖住亚克力。
            apply_root_background(cx);

            cx.on_action(show_launcher_action)
                .on_action(open_settings_action)
                .on_action(restart_application_action)
                .on_action(quit_application_action);

            // 按键在键位映射之前就被拦下来：输入框会吞掉回车，而列表已经不是
            // 焦点视图，上下键和 Esc 永远轮不到它。
            let interceptor = cx.intercept_keystrokes(intercept_key);
            cx.set_global(KeyInterceptor {
                _subscription: interceptor,
            });

            build_tray(cx);
            // 系统主题变了，托盘和 Root 底色都要跟着换，所以盯着主题这个全局值。
            let theme_watch = cx.observe_global::<Theme>(refresh_on_theme_change);
            cx.set_global(ThemeWatch {
                _subscription: theme_watch,
            });

            shortcuts::install(cx);
            // 别的进程又启动了一次，就是在叫我们把面板亮出来。
            watch_wakeup(cx);
            // 窗口尺寸改在事件循环起来之后再跟 GPUI 对一次账（见函数说明）。
            settle_viewport(cx);
        });
}

/// 启动时改窗口尺寸这件事，GPUI 要等事件循环转起来才认账。
///
/// 它的视口尺寸只在自己的平台回调里更新，而我们在 `run` 的回调里就把窗口摆好了——
/// 那时候消息循环还没开始，回调落空，GPUI 会一直按创建时的显示器尺寸排版：界面右下角
/// 被裁掉，页脚和行尾的元信息根本画不出来。
///
/// `Window::bounds_changed` 就是让窗口重新从平台读一次客户区。等循环跑起来调一次，
/// 视口就跟真实客户区对上了；窗口还没显示，用户看不见这一步。
fn settle_viewport(cx: &mut App) {
    cx.spawn(async move |cx: &mut gpui_kit::AsyncApp| {
        cx.background_executor().timer(VIEWPORT_SETTLE).await;
        cx.update(|cx| {
            let Some(handle) = cx.try_global::<LauncherWindow>().map(|handle| handle.0) else {
                return;
            };
            // 不能悄悄失败：视口没同步的表现是「界面右下角缺一块」。
            if let Err(error) = handle.update(cx, |_, window, cx| window.bounds_changed(cx)) {
                eprintln!("gpui-tools: 视口尺寸没能同步：{error:?}");
            }
        });
    })
    .detach();
}

/// 单例身份，活到进程结束。丢掉它就等于放行第二个实例。
///
/// 包一层 `Mutex<Option<_>>` 是为了**能交出去**：重启时旧进程先 `take()` 掉，
/// 新进程立刻就能抢到，不用等在单例上。
struct Singleton(Mutex<Option<instance::Instance>>);
impl gpui_kit::Global for Singleton {}

/// 交还单例身份。重启前必须调，否则新进程会被自己这个还没退干净的旧进程挡住。
pub(crate) fn release_singleton(cx: &mut App) {
    if let Some(singleton) = cx.try_global::<Singleton>() {
        singleton.0.lock().expect("单例锁不会中毒").take();
    }
}

/// 盯着「有人又启动了一次」这个信号：收到就把面板亮出来。
///
/// 一条后台线程**阻塞**等那个自动复位事件，事件到手再把它丢进 channel；前台任务被
/// 唤醒后才执行（GPUI 的 `App` 只能在主线程上碰）。
///
/// **这里不许轮询**：挂机时别的进程可能一整天都不叫我们，而每 200ms 醒一次看事件
/// 是之前这个程序唯一的空闲开销。
fn watch_wakeup(cx: &mut App) {
    let event = cx
        .try_global::<Singleton>()
        .and_then(|singleton| {
            singleton
                .0
                .lock()
                .expect("单例锁不会中毒")
                .as_ref()
                .map(instance::Instance::wake_handle)
        })
        .filter(|handle| *handle != 0);
    let Some(event) = event else {
        return;
    };

    let (sender, mut receiver) = futures::channel::mpsc::unbounded::<()>();
    std::thread::Builder::new()
        .name("wakeup".into())
        .spawn(move || {
            while instance::wait_wakeup(event) {
                if sender.unbounded_send(()).is_err() {
                    break;
                }
            }
        })
        .ok();

    cx.spawn(async move |cx: &mut gpui_kit::AsyncApp| {
        while receiver.next().await.is_some() {
            cx.update(|cx| show_launcher(cx));
        }
    })
    .detach();
}

/// 半透明时窗口该有的背景外观。
///
/// `Blurred` 在 Windows 上由 GPUI 映射成 `ACCENT_ENABLE_ACRYLICBLURBEHIND`：背后的
/// 画面被糊掉之后透上来（见 `src/bin/transparency_probe.rs`）。不半透明就退回实色，
/// 免得白付一份合成开销。
fn panel_appearance(translucent: bool) -> WindowBackgroundAppearance {
    if translucent {
        WindowBackgroundAppearance::Blurred
    } else {
        WindowBackgroundAppearance::Opaque
    }
}

/// 把当前的背景外观报给面板窗口。
///
/// 创建窗口、动窗口样式、以及用户翻「半透明」开关，这三件事都会洗掉或需要重设背景
/// 外观，所以统一走这里。
pub(crate) fn apply_panel_appearance(cx: &mut App) {
    let translucent = stored_settings(cx).translucent;
    let Some(window) = cx.try_global::<LauncherWindow>().map(|handle| handle.0) else {
        return;
    };
    // 调用点多半在面板自己的窗口更新里（点开关、换主题），当场 update 是重入、
    // 只会拿到 Err，所以推到这一轮结束之后再做。
    cx.defer(move |cx| {
        if let Err(error) = window.update(cx, |_, window, _| {
            window.set_background_appearance(panel_appearance(translucent))
        }) {
            eprintln!("gpui-tools: 拿不到面板窗口，背景外观没有更新：{error:?}");
        }
    });
}

/// 托盘菜单里的「打开启动器」。
fn show_launcher_action(_: &ShowLauncher, cx: &mut App) {
    show_launcher(cx);
}

/// 托盘菜单里的「设置」：把面板打开到设置页。
fn open_settings_action(_: &OpenSettings, cx: &mut App) {
    open_settings(cx);
}

/// 打开面板并切换到设置页，供托盘与主页命令共用。
pub fn open_settings(cx: &mut App) {
    show_launcher(cx);
    with_launcher(cx, |launcher, window, cx| {
        launcher.open_settings(window, cx)
    });
}

/// 打开面板并切换到计算器页，供主页命令共用。
pub fn open_calculator(cx: &mut App) {
    show_launcher(cx);
    with_launcher(cx, |launcher, window, cx| {
        launcher.open_calculator(window, cx)
    });
}

/// 打开面板并切到某个工具列表页（书签 / 历史 / 个人项目 / 星标 / 查询仓库）。
pub fn open_list(kind: crate::ui::launcher::ListKind, cx: &mut App) {
    show_launcher(cx);
    with_launcher(cx, move |launcher, window, cx| {
        launcher.open_list(kind, window, cx)
    });
}

/// 托盘菜单里的「重启」。
fn restart_application_action(_: &RestartApplication, cx: &mut App) {
    crate::actions::restart_launcher(cx);
}

/// 托盘菜单里的「退出」。
fn quit_application_action(_: &QuitApplication, cx: &mut App) {
    remember_window_position(cx);
    cx.quit();
}

/// 面板自己的按键分发。
fn intercept_key(event: &gpui_kit::KeystrokeEvent, window: &mut Window, cx: &mut App) {
    if !owns_window(window, cx) {
        return;
    }
    // 录制快捷键时所有按键都归录制：否则字符会漏进藏着的搜索框。
    if recording_target(cx).is_some() {
        let keystroke = event.keystroke.clone();
        with_launcher(cx, move |launcher, _window, cx| {
            launcher.capture_shortcut(&keystroke, cx)
        });
        cx.stop_propagation();
        return;
    }
    // 计算器行里正在打字：上下键归光标移动、回车归换行。不放过的话，用户在公式里
    // 按一下上箭头，面板就切到「关于」页去了。
    let typing = calculator_typing(window, cx);
    // 焦点绝不能被 `Tab` 带走：gpui-base 的 `Root` 把 `tab` 绑成了切焦点，切走之后
    // 敲字母数字全都没反应（用户报过两次）。这里直接吞掉，再把焦点还给本页输入框。
    if event.keystroke.key == "tab" && !typing {
        with_launcher(cx, |launcher, window, cx| {
            launcher.focus_active_input(window, cx)
        });
        cx.stop_propagation();
        return;
    }
    // 保险：焦点不知怎么跑到别处了（点按钮之后的极端情况），敲字时先把它拽回来。
    // 这一下字符会丢，但后面接着敲就正常了——总比整页没反应强。
    if types_text(&event.keystroke) && !active_input_focused(window, cx) {
        with_launcher(cx, |launcher, window, cx| {
            launcher.focus_active_input(window, cx)
        });
        cx.stop_propagation();
        return;
    }

    match event.keystroke.key.as_str() {
        "enter" if !typing => {
            with_launcher(cx, |launcher, window, cx| {
                launcher.activate_selected(window, cx)
            });
        }
        "up" if !typing => {
            with_launcher(cx, |launcher, window, cx| launcher.navigate(-1, window, cx));
            cx.stop_propagation();
        }
        "down" if !typing => {
            with_launcher(cx, |launcher, window, cx| launcher.navigate(1, window, cx));
            cx.stop_propagation();
        }
        "escape" => {
            with_launcher(cx, |launcher, window, cx| launcher.escape(window, cx));
            cx.stop_propagation();
        }
        _ => {}
    }
}

/// 这一下按键会不会往输入框里送一个字。
fn types_text(keystroke: &gpui_kit::Keystroke) -> bool {
    let plain = !keystroke.modifiers.control
        && !keystroke.modifiers.alt
        && !keystroke.modifiers.platform
        && !keystroke.modifiers.function;
    plain
        && keystroke
            .key_char
            .as_deref()
            .is_some_and(|text| !text.is_empty() && !text.chars().any(char::is_control))
}

/// 面板里是不是有输入框正拿着焦点（字符键的兜底要用）。
fn active_input_focused(window: &Window, cx: &App) -> bool {
    cx.try_global::<LauncherHandle>()
        .and_then(|handle| {
            handle
                .view
                .read_with(cx, |launcher, cx| launcher.active_input_focused(window, cx))
                .ok()
        })
        .unwrap_or(true)
}

/// 计算器里是不是有一行正打着字（焦点在它的公式框里）。
fn calculator_typing(window: &Window, cx: &App) -> bool {
    let Some(handle) = cx.try_global::<LauncherHandle>() else {
        return false;
    };
    handle
        .view
        .read_with(cx, |launcher, cx| {
            launcher
                .calculator
                .iter()
                .any(|row| row.input.read(cx).focus_handle(cx).is_focused(window))
        })
        .unwrap_or(false)
}

/// 正在录制的目标，直接从面板上读。
///
/// 按键拦截要当场决定收不收这个键，走不了其余回调那套延后一跳。
fn recording_target(cx: &App) -> Option<crate::ui::launcher::RecordTarget> {
    let handle = cx.try_global::<LauncherHandle>()?;
    handle
        .view
        .read_with(cx, |launcher, _| launcher.recording())
        .unwrap_or(None)
}

/// 面板是不是正在录快捷键。全局热键分发靠它决定「执行」还是「报冲突」。
pub(crate) fn recording_shortcut(cx: &App) -> bool {
    recording_target(cx).is_some()
}

/// `window` 是不是面板，而不是别的窗口。
///
/// 拦截器听得见应用里所有窗口的按键，动手前得先问一句。
pub fn owns_window(window: &Window, cx: &App) -> bool {
    cx.try_global::<LauncherWindow>()
        .is_some_and(|handle| handle.0 == window.window_handle())
}

/// 对面板视图执行一段操作，延后到当前这一轮 effect 之后。
///
/// GPUI 只给窗口的根视图解析窗口，所以只拿到 `&mut App` 的回调靠启动时存下的
/// 两个句柄跳过去。延后是必须的：在按键分发里更新窗口会重入并 panic。
pub fn with_launcher(
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
        // 不能悄悄失败：这一跳出问题是「按键全都没反应」的样子。
        if let Err(error) = window.update(cx, move |_root, window, cx| {
            if let Err(error) = view.update(cx, move |launcher, launcher_cx| {
                action(launcher, window, launcher_cx);
            }) {
                eprintln!("gpui-tools: 面板视图已不存在：{error:?}");
            }
        }) {
            eprintln!("gpui-tools: 拿不到面板窗口：{error:?}");
        }
    });
}

/// 显示并抬到前面，每次都从干净状态开始。
pub fn show_launcher(cx: &mut App) {
    // 挂机时 GPUI 的渲染线程是被挂起的（见 `platform::vsync`），**必须**先放它出来，
    // 否则面板亮起来是黑的一块。
    if !vsync::resume() {
        eprintln!("gpui-tools: 渲染线程没能恢复，面板可能不刷新");
    }
    with_launcher(cx, |launcher, window, cx| launcher.reset(window, cx));
    if let Some(panel) = cx.try_global::<LauncherPanel>() {
        panel.window.show();
        panel.window.focus();
    }
    // Windows 可能拒绝前台切换，而 GPUI 自己另记一份激活状态，所以再要一次。
    cx.activate(true);
    // 显示/隐藏窗口会把窗口合成属性打回默认，亚克力得跟着重报一次。
    apply_panel_appearance(cx);
}

/// 收起面板，但不销毁。
pub fn hide_launcher(cx: &mut App) {
    // 窗口收起来之前先记下离开的是哪一页，并结束还在进行的录制。
    with_launcher(cx, |launcher, _window, cx| launcher.leave(cx));
    if let Some(panel) = cx.try_global::<LauncherPanel>() {
        panel.window.hide();
    }
    remember_window_position(cx);
    schedule_render_suspend(cx);
}

/// 面板收起来一小会儿之后，把 GPUI 的渲染线程挂起。
///
/// GPUI 的 vsync 线程每 16.6ms 醒一次，跟窗口可不可见无关；面板挂机时它是唯一的
/// CPU 开销，而 GPUI 没有开关能停它。这里是唯一能做干净的地方：等一会儿（万一是
/// 顺手关一下马上又开），确认窗口还藏着，再挂起。
fn schedule_render_suspend(cx: &mut App) {
    cx.spawn(async move |cx: &mut gpui_kit::AsyncApp| {
        cx.background_executor().timer(RENDER_SUSPEND_DELAY).await;
        cx.update(|cx| {
            // 这中间又唤出来了就不挂。
            let visible = cx
                .try_global::<LauncherPanel>()
                .is_some_and(|panel| panel.window.is_visible());
            if visible {
                return;
            }
            if !vsync::suspend() {
                eprintln!("gpui-tools: 没找到 GPUI 的渲染线程，挂机时省不下那点 CPU");
            }
        });
    })
    .detach();
}

/// 让存下来的配色在所有窗口里生效。
pub fn apply_theme(choice: ColorScheme, window: Option<&mut Window>, cx: &mut App) {
    match choice {
        ColorScheme::System => Theme::sync_system_appearance(window, cx),
        ColorScheme::Light => Theme::change(ThemeMode::Light, window, cx),
        ColorScheme::Dark => Theme::change(ThemeMode::Dark, window, cx),
    }
    // 换主题会按内置主题重建颜色，我们自己的基础色得再盖一遍。
    crate::ui::colors::apply(cx);
    crate::tray_theme::apply(choice);
    refresh_tray_icon(cx);
    refresh_tray_menu(cx);
}

/// 当前生效的设置。
pub fn stored_settings(cx: &App) -> Settings {
    cx.try_global::<SettingsStore>()
        .map_or_else(Settings::default, |store| store.0.clone())
}

/// 写进内存，同时落盘。
pub fn store_settings(settings: Settings, cx: &mut App) {
    if !settings.save() {
        eprintln!("gpui-tools: 设置没能写进数据库");
    }
    cx.set_global(SettingsStore(settings));
}

/// 记住面板停的位置和大小，下次从那儿打开。
///
/// 只记顶边：横坐标每次重新居中，面板就不会总贴着屏幕某一侧。
///
/// 尺寸也在这儿回写：用户拖边框的过程中不动数据库，面板一收才结一次账。存的是
/// **逻辑像素**（客户区 ÷ 缩放），换一台不同 DPI 的屏打开，面板看起来还是同样大。
///
/// 调用次数也是在这里落盘的：它在内存里累加，收面板的这一刻才结账。
pub fn remember_window_position(cx: &mut App) {
    let mut settings = stored_settings(cx);
    if let Some(panel) = cx.try_global::<LauncherPanel>() {
        settings.window_y = Some(panel.window.position().1);
        let scale = panel.window.scale_factor();
        let (width, height) = panel.window.client_size();
        if scale > 0.0 && width > 0 && height > 0 {
            settings.window_width = ((width as f32 / scale).round() as u32)
                .clamp(Settings::WINDOW_WIDTH_MIN, Settings::WINDOW_WIDTH_MAX);
            settings.window_height = ((height as f32 / scale).round() as u32)
                .clamp(Settings::WINDOW_HEIGHT_MIN, Settings::WINDOW_HEIGHT_MAX);
        }
    }
    store_settings(settings, cx);
}

/// 执行一条命令并计数。
///
/// 计数只进内存，等面板收起或程序退出时才写库；每敲一次键就写一次盘，是这个程序
/// 唯一的磁盘开销。
pub fn run_command(command: &'static crate::catalogue::Command, cx: &mut App) {
    let mut settings = stored_settings(cx);
    settings.count_use(command.section.key(), command.id);
    cx.set_global(SettingsStore(settings));
    (command.action)(cx);
}

/// 建托盘图标与菜单。
fn build_tray(cx: &mut App) {
    crate::tray_theme::apply(stored_settings(cx).theme);
    match Tray::builder()
        .icon(tray_icon(cx))
        .title(strings::tray::TOOLTIP)
        .tooltip(strings::tray::TOOLTIP)
        .menu(|_| {
            vec![
                gpui_kit::MenuItem::action(strings::tray::OPEN, ShowLauncher),
                gpui_kit::MenuItem::action(strings::tray::SETTINGS, OpenSettings),
                gpui_kit::MenuItem::separator(),
                gpui_kit::MenuItem::action(strings::tray::RESTART, RestartApplication),
                gpui_kit::MenuItem::action(strings::tray::QUIT, QuitApplication),
            ]
        })
        .build(cx)
    {
        Ok(tray) => cx.set_global(TrayRegistration { tray }),
        Err(error) => eprintln!("gpui-tools: 托盘创建失败：{error}"),
    }
}

/// 主题变了就换托盘图标：深色主题用深底白字，浅色主题用浅底黑字。
fn refresh_tray_icon(cx: &mut App) {
    let icon = tray_icon(cx);
    let Some(tray) = cx.try_global::<TrayRegistration>().map(|r| r.tray.clone()) else {
        return;
    };
    if let Err(error) = tray.set_icon(Some(icon), cx) {
        eprintln!("gpui-tools: 托盘图标更新失败：{error}");
    }
}

/// 主题换了一次之后要重刷的东西：托盘，以及 Root 的那层底色。
fn refresh_on_theme_change(cx: &mut App) {
    crate::ui::colors::apply(cx);
    crate::tray_theme::apply(stored_settings(cx).theme);
    refresh_tray_icon(cx);
    refresh_tray_menu(cx);
    apply_root_background(cx);
}

/// 让 Root 不要用一层**不透**的主题底色把亚克力盖死。
///
/// gpui-component 的 Root 插件每一帧都会把整扇窗铺上 `theme.background`（见它的
/// root.rs），亚克力被这层盖得死死的——`src/bin/theme_probe.rs` 的四扇窗实验里，
/// 不 refin 的那两扇就是一片纯白/纯黑。这里把 Root 的底色换成透明，背后的模糊才
/// 透得上来；面板自己的染色由 `UiColors::panel` 负责。
///
/// 换主题、翻「半透明」开关都会洗掉这次改动，所以那几处都要重刷。
pub(crate) fn apply_root_background(cx: &mut App) {
    let backdrop = if stored_settings(cx).translucent {
        Hsla {
            a: 0.,
            ..cx.theme().background
        }
    } else {
        cx.theme().background
    };
    let Some(window) = cx.try_global::<LauncherWindow>().map(|handle| handle.0) else {
        return;
    };
    // Root 是 open_window 返回之后才建好的，这一步不能塞进建窗闭包；而且调用点多半
    // 在面板自己的窗口更新里，当场 update 是重入，同样推到下一拍。
    cx.defer(move |cx| {
        if let Err(error) = window.update(cx, |_, window, cx| {
            Root::update(window, cx, |root, _window, _cx| {
                root.style()
                    .refine(&StyleRefinement::default().bg(backdrop));
            })
        }) {
            eprintln!("gpui-tools: 拿不到面板窗口，Root 底色没有更新：{error:?}");
        }
    });
}

fn refresh_tray_menu(cx: &mut App) {
    let Some(tray) = cx
        .try_global::<TrayRegistration>()
        .map(|registration| registration.tray.clone())
    else {
        return;
    };
    if let Err(error) = tray.refresh_menu(cx) {
        eprintln!("gpui-tools: 托盘菜单主题更新失败：{error}");
    }
}

/// 按当前主题画一枚托盘图标。
fn tray_icon(cx: &App) -> TrayIcon {
    let dark = cx.theme().is_dark();
    TrayIcon::from_rgba(
        logo::tray_rgba(TRAY_ICON_SIZE, dark),
        TRAY_ICON_SIZE,
        TRAY_ICON_SIZE,
    )
    .expect("托盘图标数据有效")
}

/// 启动器所在目录。
pub fn about_directory() -> String {
    Settings::directory().map_or_else(
        || strings::settings_page::NO_CONFIG_DIRECTORY.to_string(),
        |dir| dir.display().to_string(),
    )
}

/// 面板窗口的原生句柄，所有回调共用一个，不必每次按标题重找。
#[derive(Clone, Copy)]
pub struct LauncherPanel {
    pub(crate) window: NativeWindow,
}
impl gpui_kit::Global for LauncherPanel {}

/// 面板视图，给只拿到 `&mut App` 的回调指路。
pub struct LauncherHandle {
    pub(crate) view: WeakEntity<Launcher>,
}
impl gpui_kit::Global for LauncherHandle {}

/// 面板的 GPUI 窗口。
pub struct LauncherWindow(pub(crate) AnyWindowHandle);
impl gpui_kit::Global for LauncherWindow {}

/// 当前生效的设置，内存里那一份。
pub struct SettingsStore(pub(crate) Settings);
impl gpui_kit::Global for SettingsStore {}

/// 托盘本体，留着它托盘才在。
pub struct TrayRegistration {
    tray: Tray,
}
impl gpui_kit::Global for TrayRegistration {}

/// 主题观察者，丢掉就没有人在系统换主题时更新图标了。
struct ThemeWatch {
    _subscription: Subscription,
}
impl gpui_kit::Global for ThemeWatch {}

/// 按键拦截器的订阅，留着它拦截才一直有效。
pub struct KeyInterceptor {
    _subscription: Subscription,
}
impl gpui_kit::Global for KeyInterceptor {}
