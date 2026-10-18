//! The settings the host remembers between runs.
//!
//! The file is a flat `key = value` list so a damaged or hand-edited file is
//! never fatal: an unreadable key falls back to its default and the rest of the
//! record is still used.

use std::path::PathBuf;

/// The colour scheme the host draws with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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

    /// The value this choice is stored as.
    pub fn key(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    /// The choice stored under `key`, if any.
    fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|theme| theme.key() == key)
    }
}

/// The key the colour scheme is stored under.
const THEME_KEY: &str = "theme";
/// The key the remembered panel position is stored under.
const WINDOW_Y_KEY: &str = "window_y";
/// The key the search field's height is stored under.
const SEARCH_HEIGHT_KEY: &str = "search_height";
/// The key the login startup entry is mirrored under.
const LAUNCH_AT_LOGIN_KEY: &str = "launch_at_login";
/// The key the sub-page retention window is stored under.
const RESUME_SECONDS_KEY: &str = "resume_seconds";

/// Everything the host persists, in one record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    /// Which colour scheme to draw with.
    pub theme: Theme,
    /// The launcher panel's top edge in physical pixels, or `None` before the
    /// panel has ever been placed.
    ///
    /// The horizontal position is deliberately not remembered: the panel is
    /// re-centred on its monitor on every run instead.
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
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        Self::parse(&text)
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
        std::fs::write(&path, self.render()).is_ok()
    }

    /// The file the settings live in, inside the user's profile.
    fn path() -> Option<PathBuf> {
        let profile = std::env::var_os("APPDATA")?;
        Some(
            PathBuf::from(profile)
                .join("gpui-tools")
                .join("settings.ini"),
        )
    }

    /// The folder the settings live in, for the host to point the user at.
    ///
    /// The folder is not created on demand: a first run with no settings has
    /// nothing worth showing, and the caller decides what to do about that.
    pub fn directory() -> Option<PathBuf> {
        Self::path()?.parent().map(std::path::Path::to_path_buf)
    }

    /// Parse a stored record. Unknown keys and unusable values are ignored.
    fn parse(text: &str) -> Self {
        let mut settings = Self::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                THEME_KEY => {
                    if let Some(theme) = Theme::from_key(value) {
                        settings.theme = theme;
                    }
                }
                WINDOW_Y_KEY => settings.window_y = value.parse().ok(),
                SEARCH_HEIGHT_KEY => {
                    if let Ok(height) = value.parse::<u32>() {
                        settings.search_height =
                            height.clamp(Self::SEARCH_HEIGHT_MIN, Self::SEARCH_HEIGHT_MAX);
                    }
                }
                LAUNCH_AT_LOGIN_KEY => settings.launch_at_login = Self::parse_flag(value),
                RESUME_SECONDS_KEY => {
                    if let Ok(seconds) = value.parse::<u32>() {
                        settings.resume_seconds = seconds;
                    }
                }
                _ => {}
            }
        }
        settings
    }

    /// Render the record the way it is stored.
    fn render(&self) -> String {
        let mut text = format!("{THEME_KEY}={}\n", self.theme.key());
        text.push_str(&format!("{SEARCH_HEIGHT_KEY}={}\n", self.search_height));
        text.push_str(&format!(
            "{LAUNCH_AT_LOGIN_KEY}={}\n",
            u8::from(self.launch_at_login)
        ));
        text.push_str(&format!("{RESUME_SECONDS_KEY}={}\n", self.resume_seconds));
        if let Some(y) = self.window_y {
            text.push_str(&format!("{WINDOW_Y_KEY}={y}\n"));
        }
        text
    }

    /// Read a boolean written as either a number or a word.
    fn parse_flag(value: &str) -> bool {
        matches!(
            value.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Settings, Theme};

    #[test]
    fn a_record_survives_a_round_trip() {
        let settings = Settings {
            theme: Theme::Dark,
            window_y: Some(-120),
            search_height: 34,
            launch_at_login: true,
            resume_seconds: 30,
        };
        assert_eq!(Settings::parse(&settings.render()), settings);
    }

    #[test]
    fn a_record_without_a_position_survives_a_round_trip() {
        let settings = Settings::default();
        assert_eq!(Settings::parse(&settings.render()), settings);
    }

    #[test]
    fn every_theme_survives_a_round_trip() {
        for theme in Theme::ALL {
            let settings = Settings {
                theme,
                ..Settings::default()
            };
            assert_eq!(Settings::parse(&settings.render()).theme, theme);
        }
    }

    #[test]
    fn damaged_lines_are_ignored_rather_than_fatal() {
        let settings = Settings::parse(
            "theme=purple\nwindow_y=not a number\nnonsense\n\nwindow_y=42\ntheme=light\n",
        );
        assert_eq!(settings.theme, Theme::Light);
        assert_eq!(settings.window_y, Some(42));
    }

    #[test]
    fn spacing_around_an_entry_does_not_matter() {
        let settings = Settings::parse("  theme = dark \n window_y = 300 \n");
        assert_eq!(settings.theme, Theme::Dark);
        assert_eq!(settings.window_y, Some(300));
    }

    #[test]
    fn a_field_height_stays_within_what_the_page_offers() {
        let settings = Settings::parse("search_height=400\n");
        assert_eq!(settings.search_height, Settings::SEARCH_HEIGHT_MAX);
        let settings = Settings::parse("search_height=1\n");
        assert_eq!(settings.search_height, Settings::SEARCH_HEIGHT_MIN);
    }

    #[test]
    fn a_flag_reads_as_either_a_number_or_a_word() {
        assert!(Settings::parse("launch_at_login=1\n").launch_at_login);
        assert!(Settings::parse("launch_at_login=true\n").launch_at_login);
        assert!(!Settings::parse("launch_at_login=0\n").launch_at_login);
        assert!(!Settings::parse("launch_at_login=maybe\n").launch_at_login);
    }
}
