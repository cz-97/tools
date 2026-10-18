//! The settings the host remembers between runs.
//!
//! Stored as TOML at `%APPDATA%\gpui-tools\settings.toml`. Every field falls
//! back to its default when missing, so a file written by an older build still
//! loads, and a hand-edited file only loses the entry it got wrong.
//!
//! Per-command state is keyed by the command's stable id, not by position:
//! adding or reordering commands leaves what the user already decided about the
//! others alone.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The colour scheme the host draws with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    /// Follow the operating system.
    #[default]
    System,
    Light,
    Dark,
}

impl Theme {
    /// Every choice, in the order the settings page offers them.
    pub const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    /// The label shown to the user.
    pub fn label(self) -> &'static str {
        match self {
            Self::System => "跟随系统",
            Self::Light => "浅色",
            Self::Dark => "深色",
        }
    }
}

/// What the user has decided about one command.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CommandSettings {
    /// Whether the command shows up on the home list.
    pub enabled: bool,
    /// How many times the command has been run.
    pub uses: u64,
    /// The global shortcut that runs the command, as a chord such as
    /// `ctrl+alt+KeyP`, or nothing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shortcut: Option<String>,
}

impl CommandSettings {
    /// Whether this record states anything of its own.
    ///
    /// A command the user never touched is left out of the file rather than
    /// written as a block of defaults.
    fn is_default(&self) -> bool {
        self.enabled && self.uses == 0 && self.shortcut.is_none()
    }
}

impl Default for CommandSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            uses: 0,
            shortcut: None,
        }
    }
}

/// Everything the host persists, in one record.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Which colour scheme to draw with.
    pub theme: Theme,
    /// The launcher panel's top edge in physical pixels, or `None` before the
    /// panel has ever been placed.
    ///
    /// The horizontal position is deliberately not remembered: the panel is
    /// re-centred on its monitor on every run instead.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_y: Option<i32>,
    /// The height of the search field, in logical pixels.
    pub search_height: u32,
    /// Whether a login startup entry is in place. The registry is the source of
    /// truth; this mirrors it so the settings page can draw without touching
    /// the registry while rendering.
    pub launch_at_login: bool,
    /// How long, in seconds, a hidden panel remembers the settings section it
    /// was showing. Zero always reopens on the command list.
    pub resume_seconds: u32,
    /// Per-command state, keyed by the command's stable id.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub commands: BTreeMap<String, CommandSettings>,
}

impl Settings {
    /// The shortest search field the appearance page offers.
    pub const SEARCH_HEIGHT_MIN: u32 = 24;
    /// The tallest search field the appearance page offers.
    pub const SEARCH_HEIGHT_MAX: u32 = 56;
    /// What one step of the height control changes.
    pub const SEARCH_HEIGHT_STEP: u32 = 4;
    /// The height a fresh install uses: the small input plus a little slack.
    pub const SEARCH_HEIGHT_DEFAULT: u32 = 26;

    /// How long a sub-page may stay remembered, and how each choice reads.
    pub const RESUME_CHOICES: [(u32, &'static str); 5] = [
        (0, "不保持"),
        (15, "15 秒"),
        (30, "30 秒"),
        (60, "1 分钟"),
        (300, "5 分钟"),
    ];

    /// Read the settings file, or the defaults when there is none.
    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        if let Ok(text) = std::fs::read_to_string(&path) {
            match toml::from_str::<Self>(&text) {
                Ok(mut settings) => {
                    settings.tidy();
                    return settings;
                }
                Err(error) => eprintln!(
                    "gpui-tools: {} is damaged ({error}); falling back to its defaults",
                    path.display()
                ),
            }
        }
        Settings::adopt_legacy()
    }

    /// Store the settings, reporting whether they were written.
    pub fn save(&self) -> bool {
        let Some(path) = Self::path() else {
            return false;
        };
        if let Some(parent) = path.parent()
            && std::fs::create_dir_all(parent).is_err()
        {
            return false;
        }
        let mut record = self.clone();
        record.prune();
        let rendered = match toml::to_string_pretty(&record) {
            Ok(rendered) => rendered,
            Err(error) => {
                eprintln!("gpui-tools: could not render the settings: {error}");
                return false;
            }
        };
        std::fs::write(&path, rendered).is_ok()
    }

    /// The file the settings live in, inside the user's profile.
    fn path() -> Option<PathBuf> {
        Some(Self::directory()?.join("settings.toml"))
    }

    /// The folder the settings live in, for the host to point the user at.
    ///
    /// The folder is not created on demand: a first run with no settings has
    /// nothing worth showing, and the caller decides what to do about that.
    pub fn directory() -> Option<PathBuf> {
        let profile = std::env::var_os("APPDATA")?;
        Some(PathBuf::from(profile).join("gpui-tools"))
    }

    /// What the user has decided about `command`, with its defaults filled in.
    pub fn command(&self, id: &str) -> CommandSettings {
        self.commands.get(id).cloned().unwrap_or_default()
    }

    /// Whether `command` belongs on the home list.
    pub fn command_enabled(&self, id: &str) -> bool {
        self.commands.get(id).is_none_or(|state| state.enabled)
    }

    /// How many times `command` has been run.
    pub fn command_uses(&self, id: &str) -> u64 {
        self.commands.get(id).map_or(0, |state| state.uses)
    }

    /// The global shortcut bound to `command`, if any.
    pub fn command_shortcut(&self, id: &str) -> Option<&str> {
        self.commands
            .get(id)
            .and_then(|state| state.shortcut.as_deref())
    }

    /// Show or hide a command on the home list.
    pub fn set_command_enabled(&mut self, id: &str, enabled: bool) {
        self.state_mut(id).enabled = enabled;
    }

    /// Count one run of `command`.
    pub fn count_use(&mut self, id: &str) {
        let state = self.state_mut(id);
        state.uses = state.uses.saturating_add(1);
    }

    /// Bind a command to a global shortcut, or clear it.
    pub fn set_command_shortcut(&mut self, id: &str, shortcut: Option<String>) {
        self.state_mut(id).shortcut = shortcut;
    }

    /// The record for `command`, creating it if this is the first change.
    fn state_mut(&mut self, id: &str) -> &mut CommandSettings {
        self.commands.entry(id.to_string()).or_default()
    }

    /// Bring a freshly read record back inside the ranges the pages offer, so a
    /// hand-edited file cannot produce an unusable layout.
    fn tidy(&mut self) {
        self.search_height = self
            .search_height
            .clamp(Self::SEARCH_HEIGHT_MIN, Self::SEARCH_HEIGHT_MAX);
    }

    /// Drop records that say nothing, so the file only holds real choices.
    fn prune(&mut self) {
        self.commands.retain(|_, state| !state.is_default());
    }

    /// Take over the `key = value` file earlier versions wrote.
    ///
    /// Read once: the reworked file is written straight back, so the old one is
    /// never consulted again.
    fn adopt_legacy() -> Self {
        let Some(directory) = Self::directory() else {
            return Self::default();
        };
        let Ok(text) = std::fs::read_to_string(directory.join("settings.ini")) else {
            return Self::default();
        };

        let mut settings = Self::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                "theme" => {
                    settings.theme = match value {
                        "light" => Theme::Light,
                        "dark" => Theme::Dark,
                        _ => Theme::System,
                    };
                }
                "window_y" => settings.window_y = value.parse().ok(),
                "search_height" => {
                    if let Ok(height) = value.parse::<u32>() {
                        settings.search_height = height;
                    }
                }
                "launch_at_login" => {
                    settings.launch_at_login =
                        matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "on");
                }
                "resume_seconds" => {
                    if let Ok(seconds) = value.parse::<u32>() {
                        settings.resume_seconds = seconds;
                    }
                }
                _ => {}
            }
        }
        settings.tidy();
        if !settings.save() {
            eprintln!("gpui-tools: could not move the old settings to TOML");
        }
        settings
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            window_y: None,
            search_height: Self::SEARCH_HEIGHT_DEFAULT,
            launch_at_login: false,
            resume_seconds: 0,
            commands: BTreeMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CommandSettings, Settings, Theme};

    fn sample() -> Settings {
        let mut settings = Settings {
            theme: Theme::Dark,
            window_y: Some(-120),
            search_height: 34,
            launch_at_login: true,
            resume_seconds: 30,
            ..Settings::default()
        };
        settings.set_command_enabled("repository", false);
        settings.set_command_shortcut("program-folder", Some("ctrl+alt+KeyP".to_string()));
        for _ in 0..3 {
            settings.count_use("program-folder");
        }
        settings
    }

    #[test]
    fn a_record_survives_a_round_trip() {
        let settings = sample();
        let text = toml::to_string_pretty(&settings).expect("renders");
        let read: Settings = toml::from_str(&text).expect("parses");
        assert_eq!(read, settings);
    }

    #[test]
    fn a_record_without_a_position_survives_a_round_trip() {
        let settings = Settings::default();
        let text = toml::to_string_pretty(&settings).expect("renders");
        let read: Settings = toml::from_str(&text).expect("parses");
        assert_eq!(read, settings);
    }

    #[test]
    fn missing_keys_fall_back_to_their_defaults() {
        let read: Settings = toml::from_str("theme = \"light\"\n").expect("parses");
        assert_eq!(read.theme, Theme::Light);
        assert_eq!(read.search_height, Settings::SEARCH_HEIGHT_DEFAULT);
        assert!(!read.launch_at_login);
        assert!(read.command_enabled("anything"));
    }

    #[test]
    fn unknown_keys_do_not_sink_the_file() {
        let read: Settings = toml::from_str("theme = \"dark\"\nmystery = 1\n").expect("parses");
        assert_eq!(read.theme, Theme::Dark);
    }

    #[test]
    fn a_stored_field_height_is_pulled_back_into_range() {
        let mut read: Settings = toml::from_str("search_height = 400\n").expect("parses");
        read.tidy();
        assert_eq!(read.search_height, Settings::SEARCH_HEIGHT_MAX);
    }

    #[test]
    fn a_command_defaults_to_shown_and_unused() {
        let settings = Settings::default();
        assert!(settings.command_enabled("program-folder"));
        assert_eq!(settings.command_uses("program-folder"), 0);
        assert_eq!(settings.command_shortcut("program-folder"), None);
        assert_eq!(
            settings.command("program-folder"),
            CommandSettings::default()
        );
    }

    #[test]
    fn pruning_leaves_only_records_that_say_something() {
        let mut settings = sample();
        settings.set_command_enabled("quit", true);
        settings.prune();
        assert!(settings.commands.contains_key("repository"));
        assert!(settings.commands.contains_key("program-folder"));
        assert!(!settings.commands.contains_key("quit"));
    }

    #[test]
    fn the_record_written_out_carries_every_choice() {
        let settings = sample();
        let text = toml::to_string_pretty(&settings).expect("renders");
        assert!(text.contains("theme = \"dark\""), "{text}");
        assert!(text.contains("[commands.repository]"), "{text}");
        assert!(text.contains("enabled = false"), "{text}");
        assert!(text.contains("uses = 3"), "{text}");
        assert!(text.contains("shortcut = \"ctrl+alt+KeyP\""), "{text}");
    }
}
