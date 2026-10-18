//! 全局快捷键：唤出面板的那一个，以及每条命令自己绑定的那些。
//!
//! 按键组合在文件里存成 `keyboard-types` 的键名（`ctrl+alt+KeyP`），界面显示时才
//! 转成人读的样子（`Ctrl + Alt + P`）。

use std::cell::RefCell;
use std::str::FromStr as _;
use std::time::Duration;

use global_hotkey::GlobalHotKeyManager;
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use gpui_kit::{App, Keystroke};

use crate::app::{run_command, stored_settings};
use crate::catalogue;

/// 唤出面板的热键，设置页里按这个字符串显示。
pub const SUMMON_LABEL: &str = "Ctrl + Space";

/// 轮询全局热键事件的间隔。
const EVENT_POLL: Duration = Duration::from_millis(80);
/// 唤出热键注册失败时的重试次数与间隔。
const SUMMON_RETRIES: u8 = 15;
const SUMMON_RETRY_DELAY: Duration = Duration::from_millis(200);

/// 本进程占着的全局热键。
pub struct Shortcuts {
    manager: GlobalHotKeyManager,
    /// 各命令当前绑定的组合键，以及它驱动的命令。
    pub(crate) commands: RefCell<Vec<(HotKey, &'static catalogue::Command)>>,
}

impl gpui_kit::Global for Shortcuts {}

/// 注册唤出热键，然后开始收全局热键事件。
///
/// 注册可能被拒（同一个键被别的程序占着，或者「重启」时旧的进程还没退干净），
/// 所以失败会等一会儿重试，重试期间面板仍然能从托盘打开。
pub fn install(cx: &mut App) {
    let manager = match GlobalHotKeyManager::new() {
        Ok(manager) => manager,
        Err(error) => {
            eprintln!("gpui-tools: 无法初始化全局热键管理器：{error}");
            return;
        }
    };

    let summon = HotKey::new(Some(Modifiers::CONTROL), Code::Space);
    let summon_id = summon.id();
    let registered = manager.register(summon).is_ok();
    cx.set_global(Shortcuts {
        manager,
        commands: RefCell::new(Vec::new()),
    });
    // 先按当前的设置绑一遍命令快捷键，免得第一下按键落空。
    apply_command_shortcuts(cx);

    cx.spawn(async move |cx: &mut gpui_kit::AsyncApp| {
        let mut attempts = u8::from(!registered);
        while attempts > 0 && attempts < SUMMON_RETRIES {
            cx.background_executor().timer(SUMMON_RETRY_DELAY).await;
            attempts += 1;
            let taken = cx.update(|cx| {
                cx.try_global::<Shortcuts>()
                    .is_some_and(|shortcuts| shortcuts.manager.register(summon).is_ok())
            });
            if taken {
                break;
            }
        }
        if attempts >= SUMMON_RETRIES {
            eprintln!("gpui-tools: {SUMMON_LABEL} 注册失败，可能被别的程序占用了");
        }

        loop {
            cx.background_executor().timer(EVENT_POLL).await;
            while let Ok(event) = global_hotkey::GlobalHotKeyEvent::receiver().try_recv() {
                if event.state != global_hotkey::HotKeyState::Pressed {
                    continue;
                }
                if event.id == summon_id {
                    cx.update(crate::app::show_launcher);
                    continue;
                }
                cx.update(|cx| {
                    let command = cx.try_global::<Shortcuts>().and_then(|shortcuts| {
                        shortcuts
                            .commands
                            .borrow()
                            .iter()
                            .find(|(hotkey, _)| hotkey.id() == event.id)
                            .map(|(_, command)| *command)
                    });
                    if let Some(command) = command {
                        run_command(command, cx);
                    }
                });
            }
        }
    })
    .detach();
}

/// 按设置重建命令快捷键。
///
/// 全部注销再全量注册：集合很小，局部更新反而要维护「哪个键属于哪条命令」的账。
/// 某个键被系统拒绝只跳过那一条，不影响其它。
pub fn apply_command_shortcuts(cx: &mut App) {
    let settings = stored_settings(cx);
    let wanted = catalogue::COMMANDS
        .iter()
        .filter_map(|command| {
            let chord = settings.command_shortcut(command.section.key(), command.id)?;
            match parse_chord(chord) {
                Some(hotkey) => Some((command, hotkey)),
                None => {
                    eprintln!("gpui-tools: 认不出快捷键 {chord}");
                    None
                }
            }
        })
        .collect::<Vec<_>>();

    let Some(shortcuts) = cx.try_global::<Shortcuts>() else {
        return;
    };
    let mut held = shortcuts.commands.borrow_mut();
    for (hotkey, _) in held.drain(..) {
        if let Err(error) = shortcuts.manager.unregister(hotkey) {
            eprintln!("gpui-tools: 释放旧快捷键失败：{error}");
        }
    }
    for (command, hotkey) in wanted {
        match shortcuts.manager.register(hotkey) {
            Ok(()) => held.push((hotkey, command)),
            Err(error) => eprintln!(
                "gpui-tools: 绑定 {}/{} 的快捷键失败：{error}",
                command.section.key(),
                command.id
            ),
        }
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
