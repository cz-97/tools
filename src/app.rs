//! 应用外壳：启动流程、全局状态、面板的显示与隐藏、托盘。

use gpui_kit::base::Root;
use gpui_kit::component::{ActiveTheme as _, Theme, ThemeMode};
use gpui_kit::{
    AnyWindowHandle, App, Bounds, Context, Hsla, QuitMode, StyleRefinement, Subscription,
    WeakEntity, Window, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions,
    actions, prelude::*, px, size,
};
use gpui_tools::autostart;
use gpui_tools::logo;
use gpui_tools::settings::{Settings, Theme as ColorScheme};
use gpui_tools::window::NativeWindow;
use gpui_tray::{Icon as TrayIcon, Tray};

use crate::shortcuts;
use crate::strings;
use crate::ui::launcher::Launcher;

/// 面板窗口的标题，同时也是找窗口句柄用的键。
pub const WINDOW_TITLE: &str = "GPUI Tools Launcher";
/// 面板宽（逻辑像素）。
const WINDOW_WIDTH: f32 = 720.;
/// 面板高（逻辑像素）。
const WINDOW_HEIGHT: f32 = 464.;
/// `build.rs` 嵌进 exe 的图标资源 id，窗口和外壳都用它。
const APP_ICON_ID: u16 = 1;
/// 托盘图标的边长，单位像素。
const TRAY_ICON_SIZE: u32 = 32;

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
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .with_quit_mode(QuitMode::Explicit)
        .run(|cx: &mut App| {
            gpui_kit::init(cx);

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

            let bounds = Bounds::centered(None, size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx);
            let (window_handle, launcher) = gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    // 这三个选项与喝水提醒窗一致；默认 titlebar 会让 GPUI 走系统标题栏路径。
                    titlebar: None,
                    kind: WindowKind::PopUp,
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
                    if !window.apply_panel_chrome(WINDOW_WIDTH, WINDOW_HEIGHT) {
                        eprintln!("gpui-tools: 无边框外壳没有完全生效");
                    }
                    // Windows 11 的默认圆角：就是系统设置里「圆角」那一档的半径。
                    if !window.round_corners() {
                        eprintln!("gpui-tools: 系统没采用默认圆角，窗口保持方角");
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
        });
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
    window
        .update(cx, |_, window, _| {
            window.set_background_appearance(panel_appearance(translucent))
        })
        .ok();
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
    if recording_command(cx).is_some() {
        let keystroke = event.keystroke.clone();
        with_launcher(cx, move |launcher, _window, cx| {
            launcher.capture_shortcut(&keystroke, cx)
        });
        cx.stop_propagation();
        return;
    }
    match event.keystroke.key.as_str() {
        "enter" => {
            with_launcher(cx, |launcher, _window, cx| launcher.activate_selected(cx));
        }
        "up" => {
            with_launcher(cx, |launcher, window, cx| launcher.navigate(-1, window, cx));
            cx.stop_propagation();
        }
        "down" => {
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

/// 正在录制的那个命令，直接从面板上读。
///
/// 按键拦截要当场决定收不收这个键，走不了其余回调那套延后一跳。
fn recording_command(cx: &App) -> Option<&'static crate::catalogue::Command> {
    let handle = cx.try_global::<LauncherHandle>()?;
    handle
        .view
        .read_with(cx, |launcher, _| launcher.recording())
        .unwrap_or(None)
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
    // 窗口收起来之前先记下离开的是哪一页，保留期内再唤出要回到那儿。
    with_launcher(cx, |launcher, _window, _cx| launcher.remember_section());
    if let Some(panel) = cx.try_global::<LauncherPanel>() {
        panel.window.hide();
    }
    remember_window_position(cx);
}

/// 让存下来的配色在所有窗口里生效。
pub fn apply_theme(choice: ColorScheme, window: Option<&mut Window>, cx: &mut App) {
    match choice {
        ColorScheme::System => Theme::sync_system_appearance(window, cx),
        ColorScheme::Light => Theme::change(ThemeMode::Light, window, cx),
        ColorScheme::Dark => Theme::change(ThemeMode::Dark, window, cx),
    }
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

/// 记住面板停的位置，下次从那儿打开。
///
/// 只记顶边：横坐标每次重新居中，面板就不会总贴着屏幕某一侧。
///
/// 调用次数也是在这里落盘的：它在内存里累加，收面板的这一刻才结账。
pub fn remember_window_position(cx: &mut App) {
    let mut settings = stored_settings(cx);
    if let Some(panel) = cx.try_global::<LauncherPanel>() {
        settings.window_y = Some(panel.window.position().1);
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
    // Root 是 open_window 返回之后才建好的，所以这一步不能塞进建窗闭包。
    if window
        .update(cx, |_, window, cx| {
            Root::update(window, cx, |root, _window, _cx| {
                root.style()
                    .refine(&StyleRefinement::default().bg(backdrop));
            })
        })
        .is_err()
    {
        eprintln!("gpui-tools: 拿不到面板窗口，Root 底色没有更新");
    }
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
