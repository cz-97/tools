//! The settings the host remembers between runs.
//!
//! Kept in a small SQLite database at `%APPDATA%\gpui-tools\gpui-tools.db`:
//! one `settings` table of named values, and one `commands` table holding what
//! the user decided about each command together with its usage counter.
//!
//! A command is addressed by its section *and* its id, which is also the
//! primary key of its row: two sections may both hold a command called `quit`,
//! and they are different commands. The section comes from the catalogue, so
//! this module only ever sees the section's stable key as a string.
//!
//! The record is small enough to be read and written whole, in a transaction,
//! so an interrupted write leaves the previous state intact instead of half a
//! file. Every value falls back to its default when missing or unreadable: a
//! damaged database must never keep the launcher from starting.

use std::collections::BTreeMap;
use std::path::PathBuf;

use rusqlite::{Connection, params};

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
}

/// What the user has decided about one command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandSettings {
    /// Whether the command shows up on the home list.
    ///
    /// Missing means the default, which is "shown": `Deserialize` fills it in
    /// from `Default`, so the field can keep its plain `bool` type.
    pub enabled: bool,
    /// How many times the command has been run.
    pub uses: u64,
    /// The global shortcut that runs the command, as a chord such as
    /// `ctrl+alt+KeyP`, or nothing.
    pub shortcut: Option<String>,
}

impl CommandSettings {
    /// Whether this record states anything of its own.
    ///
    /// A command the user never touched is not stored at all rather than
    /// written as a row of defaults.
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
#[derive(Clone, Debug, PartialEq)]
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
    /// Per-command state, keyed by the section key and then the command id.
    pub commands: BTreeMap<(String, String), CommandSettings>,
}

/// The tables the host needs, for a database that does not exist yet.
const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS commands (
    section  TEXT NOT NULL,
    id       TEXT NOT NULL,
    enabled  INTEGER NOT NULL,
    uses     INTEGER NOT NULL,
    shortcut TEXT,
    PRIMARY KEY (section, id)
);
";

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

    /// Read the database, or the defaults when there is nothing stored yet.
    pub fn load() -> Self {
        let Some(connection) = Self::open() else {
            return Self::default();
        };
        match Self::read(&connection) {
            Ok(Some(mut settings)) => {
                settings.tidy();
                settings
            }
            Ok(None) => Self::default(),
            Err(error) => {
                eprintln!("gpui-tools: could not read the settings ({error}); using defaults");
                Self::default()
            }
        }
    }

    /// Store the settings, reporting whether they were written.
    pub fn save(&self) -> bool {
        let Some(connection) = Self::open() else {
            return false;
        };
        match self.write(&connection) {
            Ok(()) => true,
            Err(error) => {
                eprintln!("gpui-tools: could not store the settings ({error})");
                false
            }
        }
    }

    /// The database file, inside the user's profile.
    fn path() -> Option<PathBuf> {
        Some(Self::directory()?.join("gpui-tools.db"))
    }

    /// The folder the settings live in, for the host to point the user at.
    ///
    /// The folder is not created on demand: a first run with no settings has
    /// nothing worth showing, and the caller decides what to do about that.
    pub fn directory() -> Option<PathBuf> {
        let profile = std::env::var_os("APPDATA")?;
        Some(PathBuf::from(profile).join("gpui-tools"))
    }

    /// Open the database, creating the tables the first time.
    fn open() -> Option<Connection> {
        let path = Self::path()?;
        if let Some(parent) = path.parent()
            && std::fs::create_dir_all(parent).is_err()
        {
            return None;
        }
        let connection = match Connection::open(&path) {
            Ok(connection) => connection,
            Err(error) => {
                eprintln!("gpui-tools: could not open {} ({error})", path.display());
                return None;
            }
        };
        match connection.execute_batch(SCHEMA) {
            Ok(()) => Some(connection),
            Err(error) => {
                eprintln!("gpui-tools: could not prepare the settings table ({error})");
                None
            }
        }
    }

    /// Read every stored value, or `None` when the database is still empty.
    fn read(connection: &Connection) -> rusqlite::Result<Option<Self>> {
        let mut settings = Self::default();
        let mut found = false;

        {
            let mut statement = connection.prepare("SELECT key, value FROM settings")?;
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                found = true;
                let key: String = row.get(0)?;
                let value: String = row.get(1)?;
                match key.as_str() {
                    "theme" => {
                        settings.theme = match value.as_str() {
                            "light" => Theme::Light,
                            "dark" => Theme::Dark,
                            _ => Theme::System,
                        };
                    }
                    "window_y" => settings.window_y = value.parse().ok(),
                    "search_height" => {
                        if let Ok(height) = value.parse() {
                            settings.search_height = height;
                        }
                    }
                    "launch_at_login" => settings.launch_at_login = value == "1",
                    "resume_seconds" => {
                        if let Ok(seconds) = value.parse() {
                            settings.resume_seconds = seconds;
                        }
                    }
                    _ => {}
                }
            }
        }

        {
            let mut statement =
                connection.prepare("SELECT section, id, enabled, uses, shortcut FROM commands")?;
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                found = true;
                let section: String = row.get(0)?;
                let id: String = row.get(1)?;
                settings.commands.insert(
                    (section, id),
                    CommandSettings {
                        enabled: row.get::<_, i64>(2)? != 0,
                        uses: row.get::<_, i64>(3)?.max(0) as u64,
                        shortcut: row.get(4)?,
                    },
                );
            }
        }

        Ok(found.then_some(settings))
    }

    /// Replace every stored value in one transaction.
    fn write(&self, connection: &Connection) -> rusqlite::Result<()> {
        let transaction = connection.unchecked_transaction()?;
        transaction.execute("DELETE FROM settings", [])?;
        {
            let mut insert =
                transaction.prepare("INSERT INTO settings (key, value) VALUES (?1, ?2)")?;
            insert.execute(params!["theme", self.theme.key()])?;
            insert.execute(params!["search_height", self.search_height.to_string()])?;
            insert.execute(params![
                "launch_at_login",
                u8::from(self.launch_at_login).to_string()
            ])?;
            insert.execute(params!["resume_seconds", self.resume_seconds.to_string()])?;
            if let Some(y) = self.window_y {
                insert.execute(params!["window_y", y.to_string()])?;
            }
        }

        transaction.execute("DELETE FROM commands", [])?;
        {
            let mut insert = transaction.prepare(
                "INSERT INTO commands (section, id, enabled, uses, shortcut) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for ((section, id), state) in &self.commands {
                if state.is_default() {
                    continue;
                }
                insert.execute(params![
                    section,
                    id,
                    i64::from(state.enabled),
                    state.uses as i64,
                    state.shortcut.as_deref(),
                ])?;
            }
        }
        transaction.commit()
    }

    /// What the user has decided about `id` in `section`, with defaults filled
    /// in.
    pub fn command(&self, section: &str, id: &str) -> CommandSettings {
        self.commands
            .get(&(section.to_string(), id.to_string()))
            .cloned()
            .unwrap_or_default()
    }

    /// Whether the command belongs on the home list.
    pub fn command_enabled(&self, section: &str, id: &str) -> bool {
        self.lookup(section, id).is_none_or(|state| state.enabled)
    }

    /// How many times the command has been run.
    pub fn command_uses(&self, section: &str, id: &str) -> u64 {
        self.lookup(section, id).map_or(0, |state| state.uses)
    }

    /// The global shortcut bound to the command, if any.
    pub fn command_shortcut(&self, section: &str, id: &str) -> Option<&str> {
        self.lookup(section, id)
            .and_then(|state| state.shortcut.as_deref())
    }

    /// Show or hide a command on the home list.
    pub fn set_command_enabled(&mut self, section: &str, id: &str, enabled: bool) {
        self.state_mut(section, id).enabled = enabled;
    }

    /// Count one run of a command.
    pub fn count_use(&mut self, section: &str, id: &str) {
        let state = self.state_mut(section, id);
        state.uses = state.uses.saturating_add(1);
    }

    /// Bind a command to a global shortcut, or clear it.
    pub fn set_command_shortcut(&mut self, section: &str, id: &str, shortcut: Option<String>) {
        self.state_mut(section, id).shortcut = shortcut;
    }

    /// The stored record for a command, if the user has ever touched it.
    fn lookup(&self, section: &str, id: &str) -> Option<&CommandSettings> {
        self.commands.get(&(section.to_string(), id.to_string()))
    }

    /// The record for a command, creating it if this is the first change.
    fn state_mut(&mut self, section: &str, id: &str) -> &mut CommandSettings {
        self.commands
            .entry((section.to_string(), id.to_string()))
            .or_default()
    }

    /// Bring a freshly read record back inside the ranges the pages offer, so a
    /// hand-edited database cannot produce an unusable layout.
    fn tidy(&mut self) {
        self.search_height = self
            .search_height
            .clamp(Self::SEARCH_HEIGHT_MIN, Self::SEARCH_HEIGHT_MAX);
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
    use super::{CommandSettings, SCHEMA, Settings, Theme};
    use rusqlite::Connection;

    fn database() -> Connection {
        let connection = Connection::open_in_memory().expect("in-memory database");
        connection.execute_batch(SCHEMA).expect("schema");
        connection
    }

    fn sample() -> Settings {
        let mut settings = Settings {
            theme: Theme::Dark,
            window_y: Some(-120),
            search_height: 34,
            launch_at_login: true,
            resume_seconds: 30,
            ..Settings::default()
        };
        settings.set_command_enabled("system", "repository", false);
        settings.set_command_shortcut(
            "system",
            "program-folder",
            Some("ctrl+alt+KeyP".to_string()),
        );
        for _ in 0..3 {
            settings.count_use("system", "program-folder");
        }
        settings
    }

    #[test]
    fn a_record_survives_a_round_trip() {
        let settings = sample();
        let connection = database();
        settings.write(&connection).expect("writes");
        let read = Settings::read(&connection)
            .expect("reads")
            .expect("has rows");
        assert_eq!(read, settings);
    }

    #[test]
    fn a_record_without_a_position_survives_a_round_trip() {
        let settings = Settings::default();
        let connection = database();
        settings.write(&connection).expect("writes");
        let read = Settings::read(&connection)
            .expect("reads")
            .expect("has rows");
        assert_eq!(read, settings);
    }

    #[test]
    fn an_untouched_database_reads_as_empty() {
        let connection = database();
        assert!(Settings::read(&connection).expect("reads").is_none());
    }

    #[test]
    fn two_sections_may_hold_a_command_of_the_same_name() {
        let mut settings = Settings::default();
        settings.count_use("system", "quit");
        settings.count_use("system", "quit");
        settings.count_use("about", "quit");
        let connection = database();
        settings.write(&connection).expect("writes");
        let read = Settings::read(&connection)
            .expect("reads")
            .expect("has rows");
        assert_eq!(read.command_uses("system", "quit"), 2);
        assert_eq!(read.command_uses("about", "quit"), 1);
        assert_eq!(read.command_uses("appearance", "quit"), 0);
    }

    #[test]
    fn a_command_defaults_to_shown_and_unused() {
        let settings = Settings::default();
        assert!(settings.command_enabled("system", "program-folder"));
        assert_eq!(settings.command_uses("system", "program-folder"), 0);
        assert_eq!(settings.command_shortcut("system", "program-folder"), None);
        assert_eq!(
            settings.command("system", "program-folder"),
            CommandSettings::default()
        );
    }

    #[test]
    fn untouched_commands_are_not_stored() {
        let mut settings = sample();
        settings.set_command_enabled("system", "quit", true);
        let connection = database();
        settings.write(&connection).expect("writes");
        let read = Settings::read(&connection)
            .expect("reads")
            .expect("has rows");
        assert!(
            read.commands
                .contains_key(&("system".into(), "repository".into()))
        );
        assert!(
            read.commands
                .contains_key(&("system".into(), "program-folder".into()))
        );
        assert!(
            !read
                .commands
                .contains_key(&("system".into(), "quit".into()))
        );
    }

    #[test]
    fn a_stored_field_height_is_pulled_back_into_range() {
        let connection = database();
        connection
            .execute(
                "INSERT INTO settings (key, value) VALUES ('search_height', '400')",
                [],
            )
            .expect("insert");
        let mut settings = Settings::read(&connection)
            .expect("reads")
            .expect("has rows");
        settings.tidy();
        assert_eq!(settings.search_height, Settings::SEARCH_HEIGHT_MAX);
    }
}
