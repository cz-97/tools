//! 全局快捷键：唤出面板的那一个，以及每条命令自己绑定的那些。
//!
//! 按键组合在文件里存成 `keyboard-types` 的键名（`ctrl+alt+KeyP`），界面显示时才
//! 转成人读的样子（`Ctrl + Alt + P`）。
//!
//! 唤出热键和命令热键平起平坐：都可以改、都可以不绑，冲突也只认一份设置
//! （`owner_of_chord`）。

use std::cell::RefCell;
use std::str::FromStr as _;
use std::time::Duration;

use futures::StreamExt as _;
use global_hotkey::GlobalHotKeyManager;
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use gpui_kit::{App, Keystroke};

use crate::app::{run_command, stored_settings, with_launcher};
use crate::catalogue;
use crate::strings;

/// 唤出热键注册失败时的重试次数。
const SUMMON_RETRIES: u8 = 15;
/// 每次重试之间隔多久。
const SUMMON_RETRY_DELAY: Duration = Duration::from_millis(200);

/// 一个热键现在归谁。录制时拿它判断「这组键已经被占了」。
#[derive(Clone, Copy)]
pub(crate) enum ShortcutOwner {
    /// 唤出面板的那个热键。
    Summon,
    /// 某一条静态命令。
    Command(&'static catalogue::Command),
    /// 用户自己加的一条工具（文件快捷方式 / 快捷终端），按 id 认。
    Tool(i64),
}

impl ShortcutOwner {
    /// 界面上怎么称呼这个占用者。
    ///
    /// 工具的名字是用户起的、存在数据库里，所以这里要拿 `App` 去查。
    pub(crate) fn label(self, cx: &App) -> String {
        match self {
            Self::Summon => strings::settings_page::HOTKEY_NAME.to_string(),
            Self::Command(command) => command.name.to_string(),
            Self::Tool(id) => stored_settings(cx).tool(id).map_or_else(
                || strings::tools_page::UNKNOWN_TOOL.to_string(),
                |tool| tool.name.clone(),
            ),
        }
    }
}

/// 本进程占着的全局热键。
pub struct Shortcuts {
    manager: GlobalHotKeyManager,
    /// 唤出面板的热键，以及它对应的组合键。
    ///
    /// 存一份组合键是为了判断「要不要重绑」：`HotKey` 反解不回字符串，而重绑要先注销
    /// 旧的，判断错了就会把好好的热键弄丢。
    summon: RefCell<Option<(String, HotKey)>>,
    /// 各命令（含用户工具）当前绑定的组合键，以及这组键驱动的是谁。
    pub(crate) slots: RefCell<Vec<(HotKey, ShortcutOwner)>>,
}

impl gpui_kit::Global for Shortcuts {}

/// 按设置注册唤出热键，然后开始收全局热键事件。
///
/// 注册可能被拒（同一个键被别的程序占着，或者「重启」时旧的进程还没退干净），
/// 所以失败会等一会儿重试，重试期间面板仍然能从托盘打开。
pub fn install(cx: &mut App) {
    let manager = match GlobalHotKeyManager::new() {
        Ok(manager) => manager,
        Err(error) => {
            eprintln!("tools: 无法初始化全局热键管理器：{error}");
            return;
        }
    };

    cx.set_global(Shortcuts {
        manager,
        summon: RefCell::new(None),
        slots: RefCell::new(Vec::new()),
    });
    // 先按当前的设置绑一遍，免得第一下按键落空。
    let summon_ready = apply_summon_shortcut(cx);
    apply_shortcuts(cx);

    // 启动时那次注册可能被拒（旧进程还没退干净、键被别人占着），隔一会儿重试几轮。
    // 这段只在启动后三秒内醒几次，过了就再也不碰计时器了。
    if !summon_ready {
        cx.spawn(async move |cx: &mut gpui_kit::AsyncApp| {
            for _ in 0..SUMMON_RETRIES {
                cx.background_executor().timer(SUMMON_RETRY_DELAY).await;
                if cx.update(apply_summon_shortcut) {
                    return;
                }
            }
            eprintln!("tools: 唤出热键注册失败，可能被别的程序占用了");
        })
        .detach();
    }

    // 热键事件从一条后台线程阻塞着收：系统把组合键从焦点窗口截走之后会送到那个
    // 隐藏窗口的 `WM_HOTKEY`，`global_hotkey` 再把它塞进这个 channel。
    //
    // **不要改成定时 `try_recv`**：80ms 醒一次看着便宜，却是挂机时的主要 CPU 来源，
    // 而这里本来就有一个会阻塞的 channel 可以用。
    let (sender, mut receiver) = futures::channel::mpsc::unbounded::<u32>();
    std::thread::Builder::new()
        .name("hotkey".into())
        .spawn(move || {
            let events = global_hotkey::GlobalHotKeyEvent::receiver();
            while let Ok(event) = events.recv() {
                if event.state != global_hotkey::HotKeyState::Pressed {
                    continue;
                }
                if sender.unbounded_send(event.id).is_err() {
                    break;
                }
            }
        })
        .ok();

    cx.spawn(async move |cx: &mut gpui_kit::AsyncApp| {
        while let Some(id) = receiver.next().await {
            cx.update(|cx| dispatch_hotkey(id, cx));
        }
    })
    .detach();
}

/// 一个热键落到了谁身上：执行它，或者（录制中）报一声冲突。
fn dispatch_hotkey(id: u32, cx: &mut App) {
    let owner = hotkey_owner(id, cx);
    // 录制期间一个热键都不许落地：系统把已注册的组合键从焦点窗口那里截走了，
    // 真执行起来就是「想录的键跑去执行了别人的命令」，面板还可能被它关掉。
    // 所以这里只把占用者报回去，录制继续，等用户换一组键。
    if crate::app::recording_shortcut(cx) {
        with_launcher(cx, move |launcher, _window, cx| {
            launcher.report_conflict(owner, cx);
        });
        return;
    }
    match owner {
        Some(ShortcutOwner::Summon) => crate::app::show_launcher(cx),
        Some(ShortcutOwner::Command(command)) => run_command(command, cx),
        Some(ShortcutOwner::Tool(id)) => crate::app::run_tool(id, cx),
        None => {}
    }
}

/// 这个热键 id 现在由谁占着。
fn hotkey_owner(id: u32, cx: &App) -> Option<ShortcutOwner> {
    let shortcuts = cx.try_global::<Shortcuts>()?;
    if shortcuts
        .summon
        .borrow()
        .as_ref()
        .is_some_and(|(_, hotkey)| hotkey.id() == id)
    {
        return Some(ShortcutOwner::Summon);
    }
    shortcuts
        .slots
        .borrow()
        .iter()
        .find(|(hotkey, _)| hotkey.id() == id)
        .map(|(_, owner)| *owner)
}

/// 设置里这组组合键归谁——录制时先查冲突，别等绑定完才发现键被占了。
///
/// 只看设置、不看注册结果：热键被系统拒绝时注册表里没有它，但用户的意图仍然是
/// 「这组键属于那条命令」。
pub(crate) fn owner_of_chord(chord: &str, cx: &App) -> Option<ShortcutOwner> {
    let settings = stored_settings(cx);
    find_owner(
        chord,
        settings.summon_shortcut.as_deref(),
        catalogue::COMMANDS.iter().map(|command| {
            (
                ShortcutOwner::Command(command),
                settings.command_shortcut(command.section.key(), command.id),
            )
        }),
        settings
            .tools
            .iter()
            .map(|tool| (ShortcutOwner::Tool(tool.id), tool.shortcut.as_deref())),
    )
}

/// `find_owner` 的纯逻辑部分，不碰 App，好在测试里直接喂数据。
fn find_owner<'a>(
    chord: &str,
    summon: Option<&str>,
    commands: impl Iterator<Item = (ShortcutOwner, Option<&'a str>)>,
    tools: impl Iterator<Item = (ShortcutOwner, Option<&'a str>)>,
) -> Option<ShortcutOwner> {
    if summon == Some(chord) {
        return Some(ShortcutOwner::Summon);
    }
    commands
        .chain(tools)
        .find(|(_, shortcut)| *shortcut == Some(chord))
        .map(|(owner, _)| owner)
}

/// 按设置重新绑定唤出热键，返回「当前状态是否已经就位」。
///
/// 不绑（`None`）也算就位：那种情况下托盘才是入口。
pub fn apply_summon_shortcut(cx: &mut App) -> bool {
    let wanted = stored_settings(cx).summon_shortcut;
    let Some(shortcuts) = cx.try_global::<Shortcuts>() else {
        return false;
    };
    let mut summon = shortcuts.summon.borrow_mut();
    if summon.as_ref().map(|(chord, _)| chord.as_str()) == wanted.as_deref() {
        return true;
    }
    if let Some((_, hotkey)) = summon.take()
        && let Err(error) = shortcuts.manager.unregister(hotkey)
    {
        eprintln!("tools: 释放旧唤出热键失败：{error}");
    }
    let Some(chord) = wanted else {
        return true;
    };
    let Some(hotkey) = parse_chord(&chord) else {
        eprintln!("tools: 认不出唤出热键 {chord}");
        return false;
    };
    match shortcuts.manager.register(hotkey) {
        Ok(()) => {
            *summon = Some((chord, hotkey));
            true
        }
        Err(error) => {
            eprintln!("tools: 唤出热键 {chord} 注册失败：{error}");
            false
        }
    }
}

/// 按设置重建命令快捷键（静态命令和用户工具一起）。
///
/// 全部注销再全量注册：集合很小，局部更新反而要维护「哪个键属于哪条命令」的账。
/// 某个键被系统拒绝只跳过那一条，不影响其它。
pub fn apply_shortcuts(cx: &mut App) {
    let settings = stored_settings(cx);
    let mut wanted = Vec::new();
    for command in catalogue::COMMANDS {
        if let Some(chord) = settings.command_shortcut(command.section.key(), command.id) {
            push_wanted(&mut wanted, chord, ShortcutOwner::Command(command));
        }
    }
    for tool in &settings.tools {
        if let Some(chord) = tool.shortcut.as_deref() {
            push_wanted(&mut wanted, chord, ShortcutOwner::Tool(tool.id));
        }
    }

    let Some(shortcuts) = cx.try_global::<Shortcuts>() else {
        return;
    };
    let mut held = shortcuts.slots.borrow_mut();
    for (hotkey, _) in held.drain(..) {
        if let Err(error) = shortcuts.manager.unregister(hotkey) {
            tools_platform::admin::log(&format!("释放旧快捷键失败：{error}"));
        }
    }
    for (owner, hotkey) in wanted {
        match shortcuts.manager.register(hotkey) {
            Ok(()) => held.push((hotkey, owner)),
            Err(error) => tools_platform::admin::log(&format!(
                "快捷键注册失败：{}（{error}）",
                owner.label(cx)
            )),
        }
    }
}

/// 把一个组合键解析成热键并排进待注册的队列；认不出的只报告，不拦别的。
fn push_wanted(wanted: &mut Vec<(ShortcutOwner, HotKey)>, chord: &str, owner: ShortcutOwner) {
    match parse_chord(chord) {
        Some(hotkey) => wanted.push((owner, hotkey)),
        None => tools_platform::admin::log(&format!("认不出快捷键 {chord}")),
    }
}

/// 按下的一次按键对应的组合键，没有修饰键时返回 `None`。
///
/// 没有修饰键的组合会被面板自己吃掉，做全局快捷键没有意义，所以直接不收。
pub fn chord_from_keystroke(keystroke: &Keystroke) -> Option<String> {
    let mut parts = Vec::new();
    if keystroke.modifiers.control {
        parts.push("ctrl".to_string());
    }
    if keystroke.modifiers.alt {
        parts.push("alt".to_string());
    }
    if keystroke.modifiers.shift {
        parts.push("shift".to_string());
    }
    if keystroke.modifiers.platform {
        parts.push("super".to_string());
    }
    if parts.is_empty() {
        return None;
    }
    parts.push(code_name(&keystroke.key)?);
    Some(parts.join("+"))
}

/// GPUI 报出来的键名对应的 `keyboard-types` 键名。
fn code_name(key: &str) -> Option<String> {
    let mut characters = key.chars();
    if let (Some(character), None) = (characters.next(), characters.next()) {
        if character.is_ascii_alphabetic() {
            return Some(format!("Key{}", character.to_ascii_uppercase()));
        }
        if character.is_ascii_digit() {
            return Some(format!("Digit{character}"));
        }
    }
    if let Some(number) = key.strip_prefix('f')
        && number.parse::<u8>().is_ok_and(|n| (1..=24).contains(&n))
    {
        return Some(format!("F{number}"));
    }
    let named = match key {
        "space" => "Space",
        "tab" => "Tab",
        "comma" => "Comma",
        "period" => "Period",
        "slash" => "Slash",
        "semicolon" => "Semicolon",
        "quote" => "Quote",
        "backquote" => "Backquote",
        "backslash" => "Backslash",
        "minus" => "Minus",
        "equal" => "Equal",
        "bracketleft" => "BracketLeft",
        "bracketright" => "BracketRight",
        "insert" => "Insert",
        "delete" => "Delete",
        "home" => "Home",
        "end" => "End",
        "pageup" => "PageUp",
        "pagedown" => "PageDown",
        "left" => "ArrowLeft",
        "right" => "ArrowRight",
        "up" => "ArrowUp",
        "down" => "ArrowDown",
        _ => return None,
    };
    Some(named.to_string())
}

/// 把存下来的组合键解析成系统能注册的热键。
fn parse_chord(chord: &str) -> Option<HotKey> {
    let mut modifiers = Modifiers::empty();
    let mut code = None;
    for part in chord.split('+') {
        match part.trim() {
            "ctrl" => modifiers |= Modifiers::CONTROL,
            "alt" => modifiers |= Modifiers::ALT,
            "shift" => modifiers |= Modifiers::SHIFT,
            "super" => modifiers |= Modifiers::SUPER,
            other => code = Some(Code::from_str(other).ok()?),
        }
    }
    Some(HotKey::new(
        (!modifiers.is_empty()).then_some(modifiers),
        code?,
    ))
}

/// 组合键在人眼里的样子：`ctrl+alt+KeyP` 写成 `Ctrl + Alt + P`。
pub fn shortcut_label(chord: &str) -> String {
    chord
        .split('+')
        .map(|part| match part {
            "ctrl" => "Ctrl".to_string(),
            "alt" => "Alt".to_string(),
            "shift" => "Shift".to_string(),
            "super" => "Win".to_string(),
            other => part_label(other),
        })
        .collect::<Vec<_>>()
        .join(" + ")
}

/// 单个键的显示名：字母和数字去掉 `keyboard-types` 的前缀，方向键画成箭头。
fn part_label(code: &str) -> String {
    if let Some(rest) = code.strip_prefix("Key") {
        return rest.to_string();
    }
    if let Some(rest) = code.strip_prefix("Digit") {
        return rest.to_string();
    }
    match code {
        "ArrowLeft" => "\u{2190}".to_string(),
        "ArrowRight" => "\u{2192}".to_string(),
        "ArrowUp" => "\u{2191}".to_string(),
        "ArrowDown" => "\u{2193}".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalogue;

    fn none_tools() -> impl Iterator<Item = (ShortcutOwner, Option<&'static str>)> {
        std::iter::empty()
    }

    #[test]
    fn a_chord_belongs_to_the_first_claimant() {
        let first = &catalogue::COMMANDS[0];
        let second = &catalogue::COMMANDS[1];
        let owners = [
            (ShortcutOwner::Command(first), Some("ctrl+KeyP")),
            (ShortcutOwner::Command(second), Some("ctrl+KeyP")),
        ];
        match find_owner("ctrl+KeyP", None, owners.into_iter(), none_tools()) {
            Some(ShortcutOwner::Command(command)) => assert_eq!(command.id, first.id),
            _ => panic!("这组键应该归先占住的那条命令"),
        }
    }

    #[test]
    fn the_summon_hotkey_claims_its_chord_before_commands() {
        let owners = [(
            ShortcutOwner::Command(&catalogue::COMMANDS[0]),
            Some("ctrl+Space"),
        )];
        assert!(matches!(
            find_owner(
                "ctrl+Space",
                Some("ctrl+Space"),
                owners.into_iter(),
                none_tools()
            ),
            Some(ShortcutOwner::Summon)
        ));
    }

    #[test]
    fn a_free_chord_has_no_owner() {
        let owners = [(ShortcutOwner::Command(&catalogue::COMMANDS[0]), None)];
        assert!(find_owner("ctrl+KeyP", None, owners.into_iter(), none_tools()).is_none());
        assert!(
            find_owner(
                "ctrl+KeyP",
                Some("ctrl+Space"),
                owners.into_iter(),
                none_tools()
            )
            .is_none()
        );
    }

    #[test]
    fn a_user_tool_can_claim_a_chord_too() {
        let owners = [(ShortcutOwner::Command(&catalogue::COMMANDS[0]), None)];
        let tools = [(ShortcutOwner::Tool(7), Some("ctrl+alt+KeyT"))];
        match find_owner("ctrl+alt+KeyT", None, owners.into_iter(), tools.into_iter()) {
            Some(ShortcutOwner::Tool(id)) => assert_eq!(id, 7),
            _ => panic!("这组键应该归那条用户工具"),
        }
    }
}
