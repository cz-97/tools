//! 跨次运行记下来的设置。
//!
//! 存在 SQLite 数据库 `%APPDATA%\gpui-tools\gpui-tools.db` 里：一张 `settings` 表放
//! 键值对，一张 `commands` 表放每条命令的开关、次数和快捷键。
//!
//! 命令由「子模块 + id」定位，这两列也是那一行的主键：两个子模块可以各有一条叫 `quit`
//! 的命令，它们是两条不同的命令。子模块在目录里定，本模块只见到它的稳定键（字符串）。
//!
//! 记录很小，所以整份一次读、一次事务写，写到一半中断也只会留下上一次的完整状态。
//! 任何一项读不出来都退回默认值：数据库坏了也绝不能让启动器起不来。

use std::collections::BTreeMap;
use std::path::PathBuf;

use rusqlite::{Connection, params};

/// 配色方案。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Theme {
    /// 跟随系统。
    #[default]
    System,
    Light,
    Dark,
}

impl Theme {
    /// 全部选项，顺序就是设置页里的顺序。
    pub const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    /// 这一项存进数据库时写的值。
    pub fn key(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

/// 用户对某一条命令的决定。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandSettings {
    /// 这条命令要不要出现在主页上。
    pub enabled: bool,
    /// 跑过多少次。
    pub uses: u64,
    /// 唤起它的全局快捷键，形如 `ctrl+alt+KeyP`；没绑就是 `None`。
    pub shortcut: Option<String>,
}

impl CommandSettings {
    /// 这条记录有没有说出什么。
    ///
    /// 用户没动过的命令根本不会落库，而不是写一行默认值进去。
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

/// 程序要记住的所有东西，就是这一份记录。
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// 用哪套配色。
    pub theme: Theme,
    /// 是否启用半透明面板背景。
    pub translucent: bool,
    /// 半透明时面板底色的不透明度，百分比（100 = 全实，盖住亚克力）。
    pub panel_opacity: u32,
    /// 面板顶边在物理像素下的位置；面板还没被摆过时是 `None`。
    ///
    /// 横坐标故意不记：每次启动都按显示器重新居中。
    pub window_y: Option<i32>,
    /// 搜索框高度，逻辑像素。
    pub search_height: u32,
    /// 登录启动项在不在。**注册表才是事实**，这里只是镜像一份，好在画设置页时
    /// 不用去读注册表。
    pub launch_at_login: bool,
    /// 收起面板后还能记住原页面多少秒。0 表示每次都回命令列表。
    pub resume_seconds: u32,
    /// 每条命令的状态，键是（子模块键, 命令 id）。
    pub commands: BTreeMap<(String, String), CommandSettings>,
}

/// 程序需要的表，第一次用时建好，之后不再动。
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
    /// 外观页能给的搜索框最低高度。
    pub const SEARCH_HEIGHT_MIN: u32 = 24;
    /// 外观页能给的搜索框最高高度。
    pub const SEARCH_HEIGHT_MAX: u32 = 56;
    /// 高度控件一步走多少。
    pub const SEARCH_HEIGHT_STEP: u32 = 4;
    /// 全新安装用的高度：小号输入框再加一点余量。
    pub const SEARCH_HEIGHT_DEFAULT: u32 = 26;

    /// 子页面能记多久，界面上按顺序给出这几档。
    pub const RESUME_CHOICES: [u32; 5] = [0, 15, 30, 60, 300];
    /// 面板底色最淡能调到多淡。
    pub const PANEL_OPACITY_MIN: u32 = 30;
    /// 面板底色最深能调到多深（也就是全实）。
    pub const PANEL_OPACITY_MAX: u32 = 100;
    /// 透明度控件一步走多少个百分点。
    pub const PANEL_OPACITY_STEP: u32 = 2;
    /// 半透明底色默认的不透明度：够看清字，又明显看得出背后在动。
    pub const PANEL_OPACITY_DEFAULT: u32 = 78;

    /// Remove stored command state whose `(section, id)` no longer exists in the catalogue.
    ///
    /// Returns whether the in-memory database changed so startup can persist the cleanup.
    pub fn retain_known_commands(
        &mut self,
        known: impl IntoIterator<Item = (&'static str, &'static str)>,
    ) -> bool {
        let known = known.into_iter().collect::<std::collections::BTreeSet<_>>();
        let original_len = self.commands.len();
        self.commands
            .retain(|(section, id), _| known.contains(&(section.as_str(), id.as_str())));
        self.commands.len() != original_len
    }

    /// 读数据库；还没有库就用默认值。
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
                eprintln!("gpui-tools: 设置读不出来（{error}），改用默认值");
                Self::default()
            }
        }
    }

    /// 写回数据库，返回是否写成功。
    pub fn save(&self) -> bool {
        let Some(connection) = Self::open() else {
            return false;
        };
        match self.write(&connection) {
            Ok(()) => true,
            Err(error) => {
                eprintln!("gpui-tools: 设置没能写进数据库（{error}）");
                false
            }
        }
    }

    /// 数据库文件的位置，在用户配置目录下。
    fn path() -> Option<PathBuf> {
        Some(Self::directory()?.join("gpui-tools.db"))
    }

    /// 设置所在的文件夹，供界面指给用户看。
    ///
    /// 这个函数不负责建目录：什么都没配过的时候这里也没东西可看，由调用方决定怎么办。
    pub fn directory() -> Option<PathBuf> {
        let profile = std::env::var_os("APPDATA")?;
        Some(PathBuf::from(profile).join("gpui-tools"))
    }

    /// 打开数据库，第一次用就把表建好。
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
                eprintln!("gpui-tools: 打不开 {}（{error}）", path.display());
                return None;
            }
        };
        match connection.execute_batch(SCHEMA) {
            Ok(()) => Some(connection),
            Err(error) => {
                eprintln!("gpui-tools: 建表失败（{error}）");
                None
            }
        }
    }

    /// 读出所有存下来的值；库还是空的时候返回 `None`。
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
                    "translucent" => settings.translucent = value == "1",
                    "panel_opacity" => {
                        if let Ok(opacity) = value.parse() {
                            settings.panel_opacity = opacity;
                        }
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

    /// 在一个事务里把整份记录换上去。
    fn write(&self, connection: &Connection) -> rusqlite::Result<()> {
        let transaction = connection.unchecked_transaction()?;
        transaction.execute("DELETE FROM settings", [])?;
        {
            let mut insert =
                transaction.prepare("INSERT INTO settings (key, value) VALUES (?1, ?2)")?;
            insert.execute(params!["theme", self.theme.key()])?;
            insert.execute(params![
                "translucent",
                u8::from(self.translucent).to_string()
            ])?;
            insert.execute(params!["panel_opacity", self.panel_opacity.to_string()])?;
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

    /// 用户对这条命令的决定，缺的项补默认值。
    pub fn command(&self, section: &str, id: &str) -> CommandSettings {
        self.commands
            .get(&(section.to_string(), id.to_string()))
            .cloned()
            .unwrap_or_default()
    }

    /// 这条命令该不该出现在主页上。
    pub fn command_enabled(&self, section: &str, id: &str) -> bool {
        self.lookup(section, id).is_none_or(|state| state.enabled)
    }

    /// 这条命令跑过多少次。
    pub fn command_uses(&self, section: &str, id: &str) -> u64 {
        self.lookup(section, id).map_or(0, |state| state.uses)
    }

    /// 这条命令绑的全局快捷键。
    pub fn command_shortcut(&self, section: &str, id: &str) -> Option<&str> {
        self.lookup(section, id)
            .and_then(|state| state.shortcut.as_deref())
    }

    /// 把命令放上主页，或从主页拿掉。
    pub fn set_command_enabled(&mut self, section: &str, id: &str, enabled: bool) {
        self.state_mut(section, id).enabled = enabled;
    }

    /// 给这条命令记一次运行。
    pub fn count_use(&mut self, section: &str, id: &str) {
        let state = self.state_mut(section, id);
        state.uses = state.uses.saturating_add(1);
    }

    /// 给命令绑一个全局快捷键，或者解绑。
    pub fn set_command_shortcut(&mut self, section: &str, id: &str, shortcut: Option<String>) {
        self.state_mut(section, id).shortcut = shortcut;
    }

    /// 这条命令存下来的记录——用户动过才有。
    fn lookup(&self, section: &str, id: &str) -> Option<&CommandSettings> {
        self.commands.get(&(section.to_string(), id.to_string()))
    }

    /// 这条命令的记录，第一次改的时候顺手建出来。
    fn state_mut(&mut self, section: &str, id: &str) -> &mut CommandSettings {
        self.commands
            .entry((section.to_string(), id.to_string()))
            .or_default()
    }

    /// 把刚读出来的记录拉回界面允许的范围，手改过的库也不会摆出一个没法用的界面。
    fn tidy(&mut self) {
        self.search_height = self
            .search_height
            .clamp(Self::SEARCH_HEIGHT_MIN, Self::SEARCH_HEIGHT_MAX);
        self.panel_opacity = self
            .panel_opacity
            .clamp(Self::PANEL_OPACITY_MIN, Self::PANEL_OPACITY_MAX);
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            translucent: false,
            panel_opacity: Self::PANEL_OPACITY_DEFAULT,
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
        let connection = Connection::open_in_memory().expect("内存数据库");
        connection.execute_batch(SCHEMA).expect("建表");
        connection
    }

    fn sample() -> Settings {
        let mut settings = Settings {
            theme: Theme::Dark,
            translucent: true,
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
        settings.write(&connection).expect("写入");
        let read = Settings::read(&connection).expect("读取").expect("有数据");
        assert_eq!(read, settings);
    }

    #[test]
    fn a_record_without_a_position_survives_a_round_trip() {
        let settings = Settings::default();
        let connection = database();
        settings.write(&connection).expect("写入");
        let read = Settings::read(&connection).expect("读取").expect("有数据");
        assert_eq!(read, settings);
    }

    #[test]
    fn an_untouched_database_reads_as_empty() {
        let connection = database();
        assert!(Settings::read(&connection).expect("读取").is_none());
    }

    #[test]
    fn two_sections_may_hold_a_command_of_the_same_name() {
        let mut settings = Settings::default();
        settings.count_use("system", "quit");
        settings.count_use("system", "quit");
        settings.count_use("about", "quit");
        let connection = database();
        settings.write(&connection).expect("写入");
        let read = Settings::read(&connection).expect("读取").expect("有数据");
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
        settings.write(&connection).expect("写入");
        let read = Settings::read(&connection).expect("读取").expect("有数据");
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
    fn unknown_command_records_are_pruned_by_section_and_id() {
        let mut settings = Settings::default();
        settings.set_command_enabled("system", "quit", false);
        settings.set_command_shortcut("about", "removed", Some("ctrl+KeyR".into()));
        settings.set_command_shortcut("removed-section", "quit", Some("ctrl+KeyQ".into()));

        assert!(settings.retain_known_commands([("system", "quit"), ("about", "repository")]));
        assert_eq!(settings.commands.len(), 1);
        assert!(
            settings
                .commands
                .contains_key(&("system".into(), "quit".into()))
        );
        assert!(!settings.retain_known_commands([("system", "quit")]));
    }

    #[test]
    fn a_stored_field_height_is_pulled_back_into_range() {
        let connection = database();
        connection
            .execute(
                "INSERT INTO settings (key, value) VALUES ('search_height', '400')",
                [],
            )
            .expect("插入");
        let mut settings = Settings::read(&connection).expect("读取").expect("有数据");
        settings.tidy();
        assert_eq!(settings.search_height, Settings::SEARCH_HEIGHT_MAX);
    }
}
