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

/// Everything the host persists, in one record.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    /// Which colour scheme to draw with.
    pub theme: Theme,
    /// The launcher panel's top edge in physical pixels, or `None` before the
    /// panel has ever been placed.
    ///
    /// The horizontal position is deliberately not remembered: the panel is
    /// re-centred on its monitor on every run instead.
    pub window_y: Option<i32>,
}

impl Settings {
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

    /// Parse a stored record. Unknown keys and unusable values are ignored.
    fn parse(text: &str) -> Self {
        let mut settings = Self::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key.trim() {
                THEME_KEY => {
                    if let Some(theme) = Theme::from_key(value.trim()) {
                        settings.theme = theme;
                    }
                }
                WINDOW_Y_KEY => settings.window_y = value.trim().parse().ok(),
                _ => {}
            }
        }
        settings
    }

    /// Render the record the way it is stored.
    fn render(&self) -> String {
        let mut text = format!("{THEME_KEY}={}\n", self.theme.key());
        if let Some(y) = self.window_y {
            text.push_str(&format!("{WINDOW_Y_KEY}={y}\n"));
        }
        text
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
                window_y: None,
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
    fn spacing_around_a_entry_does_not_matter() {
        let settings = Settings::parse("  theme = dark \n window_y = 300 \n");
        assert_eq!(settings.theme, Theme::Dark);
        assert_eq!(settings.window_y, Some(300));
    }
}
