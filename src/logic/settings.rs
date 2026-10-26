//! 跨次运行记下来的设置。
//!
//! 存在 SQLite 数据库 `%APPDATA%\tools\tools.db` 里：一张 `settings` 表放
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

/// 用户自己加的工具都有哪几种。
///
/// 静态命令写在 `command/catalogue.rs` 里，编译期就定了；这几种是用户在设置页里
/// 随手加的，存在数据库的 `tools` 表里。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ToolKind {
    /// 文件：打开它所在的文件夹，或者按设置用默认应用打开。
    File,
    /// 文件夹：打开它自己。
    Folder,
    /// 快捷终端：在系统默认终端里跑一条命令。
    Terminal,
}

impl ToolKind {
    /// 全部种类，顺序就是设置页里的顺序。
    pub const ALL: [Self; 3] = [Self::File, Self::Folder, Self::Terminal];

    /// 存进数据库时写的值。
    pub fn key(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Folder => "folder",
            Self::Terminal => "terminal",
        }
    }

    /// 从数据库读回来；认不出的值返回 `None`（那一条就当不存在）。
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.key() == key)
    }

    /// 记调用次数、绑快捷键时用的「子模块」键。
    ///
    /// 和静态命令的 `Section::key()` 不会撞：那边全是英文单词，这边带 `tool-` 前缀。
    pub fn section_key(self) -> &'static str {
        match self {
            Self::File => "tool-file",
            Self::Folder => "tool-folder",
            Self::Terminal => "tool-terminal",
        }
    }

    /// 这一种是不是「指着某个文件/文件夹」（要检查路径还在不在）。
    pub fn is_path(self) -> bool {
        matches!(self, Self::File | Self::Folder)
    }
}

/// 用户自己加的一条工具。
///
/// 名字、目标、开关、次数、快捷键全在这一个结构里：它和静态命令一样参与主页的
/// 排序与搜索，但存在自己那张表里，不占 `commands` 表的键空间。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserTool {
    /// 主键，只在工具表里唯一。发布后不再复用。
    pub id: i64,
    pub kind: ToolKind,
    /// 主页和设置页上显示的名字，同时是搜索词。
    pub name: String,
    /// 文件路径 / 文件夹路径 / 命令行，同样参与搜索。
    pub target: String,
    /// 要不要出现在主页上。
    pub enabled: bool,
    /// 跑过多少次。
    pub uses: u64,
    /// 唤起它的全局快捷键，形如 `ctrl+alt+KeyP`。
    pub shortcut: Option<String>,
    /// [`ToolKind::File`] 专用：执行时用**默认应用**打开文件（`true`），
    /// 还是打开它所在的文件夹（`false`）。别的种类不用这个字段。
    pub open_default: bool,
    /// [`ToolKind::Terminal`] 专用：终端的工作目录；`None` 表示用户文件夹。
    pub start_dir: Option<String>,
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
    /// 面板宽度，逻辑像素。用户拖边框改，收起面板时回写。
    pub window_width: u32,
    /// 面板高度，逻辑像素。同样的来路。
    pub window_height: u32,
    /// 搜索框高度，逻辑像素。
    pub search_height: u32,
    /// 登录启动项在不在。**注册表才是事实**，这里只是镜像一份，好在画设置页时
    /// 不用去读注册表。
    pub launch_at_login: bool,
    /// 是否以管理员身份启动。**计划任务才是事实**（`%SystemRoot%\System32\schtasks`
    /// 里那条 `tools-launcher`），这里同样只是镜像。
    pub run_as_admin: bool,
    /// 收起面板后还能记住原页面多少秒。0 表示每次都回命令列表。
    pub resume_seconds: u32,
    /// 唤出面板的全局快捷键，形如 `ctrl+Space`；`None` 表示不绑，只能从托盘打开。
    pub summon_shortcut: Option<String>,
    /// 计算器结果最多保留的小数位数；`0` 表示只显示整数（非整数会四舍五入）。
    pub calculator_precision: u32,
    /// 指令匹配：算式直接显示结果。
    pub matcher_calculator: bool,
    /// 指令匹配：金额转大写。
    pub matcher_amount: bool,
    /// 指令匹配：网址可打开。
    pub matcher_url: bool,
    /// 指令匹配：文件/文件夹路径可打开。
    pub matcher_path: bool,
    /// 火狐系浏览器的 profile 目录；`None` 表示按产品目录自动探测。
    pub firefox_profile: Option<String>,
    /// GitHub 用户名：个人项目和星标都用它查。
    pub github_user: String,
    /// GitHub 令牌，可以为空；填了接口额度高一档。
    pub github_token: String,
    /// 每条命令的状态，键是（子模块键, 命令 id）。
    pub commands: BTreeMap<(String, String), CommandSettings>,
    /// 用户自己加的工具（文件快捷方式、快捷终端），按 id 升序。
    pub tools: Vec<UserTool>,
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
CREATE TABLE IF NOT EXISTS tools (
    id           INTEGER PRIMARY KEY NOT NULL,
    kind         TEXT NOT NULL,
    name         TEXT NOT NULL,
    target       TEXT NOT NULL,
    enabled      INTEGER NOT NULL,
    uses         INTEGER NOT NULL,
    shortcut     TEXT,
    open_default INTEGER NOT NULL,
    start_dir    TEXT
);
";

/// 「唤出热键不绑」在数据库里写成的值。
///
/// 不能写空串：设置行不存在时读的是默认值，存的这一行必须在两种意思之间分得清。
const SUMMON_NONE: &str = "none";

/// `tools` 表必须有的列。少一列就说明这个库是旧结构，整个表丢掉重建（见 `prepare`）。
const TOOL_COLUMNS: [&str; 9] = [
    "id",
    "kind",
    "name",
    "target",
    "enabled",
    "uses",
    "shortcut",
    "open_default",
    "start_dir",
];

impl Settings {
    /// 全新安装时唤出面板用的快捷键。
    pub const SUMMON_DEFAULT: &str = "ctrl+Space";
    /// 外观页能给的搜索框最低高度。
    pub const SEARCH_HEIGHT_MIN: u32 = 24;
    /// 外观页能给的搜索框最高高度。
    pub const SEARCH_HEIGHT_MAX: u32 = 56;
    /// 高度控件一步走多少。
    pub const SEARCH_HEIGHT_STEP: u32 = 4;
    /// 全新安装用的高度：小号输入框再加一点余量。
    pub const SEARCH_HEIGHT_DEFAULT: u32 = 26;

    /// 面板默认宽度（逻辑像素）。
    pub const WINDOW_WIDTH_DEFAULT: u32 = 720;
    /// 面板默认高度（逻辑像素）。
    pub const WINDOW_HEIGHT_DEFAULT: u32 = 464;
    /// 面板最窄拖到这儿：再窄设置页左边那列和右边内容就叠在一起了。
    pub const WINDOW_WIDTH_MIN: u32 = 520;
    /// 面板最矮拖到这儿：页头加一行命令都得看得见。
    pub const WINDOW_HEIGHT_MIN: u32 = 240;
    /// 面板宽度上限。只是防手滑，真正拦着的是屏幕。
    pub const WINDOW_WIDTH_MAX: u32 = 3840;
    /// 面板高度上限。
    pub const WINDOW_HEIGHT_MAX: u32 = 2160;

    /// 子页面能记多久，界面上按顺序给出这几档。
    pub const RESUME_CHOICES: [u32; 5] = [0, 15, 30, 60, 300];
    /// 面板底色最淡能调到多淡。
    pub const PANEL_OPACITY_MIN: u32 = 0;
    /// 面板底色最深能调到多深（也就是全实）。
    pub const PANEL_OPACITY_MAX: u32 = 100;
    /// 透明度控件一步走多少个百分点。
    pub const PANEL_OPACITY_STEP: u32 = 2;
    /// 半透明底色默认的不透明度：够看清字，又明显看得出背后在动。
    pub const PANEL_OPACITY_DEFAULT: u32 = 78;

    /// 计算器结果默认保留的小数位数。
    pub const CALCULATOR_PRECISION_MIN: u32 = 0;
    /// 计算器结果最多保留这么多位小数。
    pub const CALCULATOR_PRECISION_MAX: u32 = 12;
    /// 精度步进一次走几位。
    pub const CALCULATOR_PRECISION_STEP: u32 = 1;
    /// 全新安装用的精度：和改之前的行为一致（最多十位、去尾零）。
    pub const CALCULATOR_PRECISION_DEFAULT: u32 = 10;

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
                eprintln!("tools: 设置读不出来（{error}），改用默认值");
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
                eprintln!("tools: 设置没能写进数据库（{error}）");
                false
            }
        }
    }

    /// 数据库文件的位置，在用户配置目录下。
    fn path() -> Option<PathBuf> {
        Some(Self::directory()?.join("tools.db"))
    }

    /// 设置所在的文件夹，供界面指给用户看。
    ///
    /// 这个函数不负责建目录：什么都没配过的时候这里也没东西可看，由调用方决定怎么办。
    pub fn directory() -> Option<PathBuf> {
        let profile = std::env::var_os("APPDATA")?;
        Some(PathBuf::from(profile).join("tools"))
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
                eprintln!("tools: 打不开 {}（{error}）", path.display());
                return None;
            }
        };
        match Self::prepare(&connection) {
            Ok(()) => Some(connection),
            Err(error) => {
                eprintln!("tools: 建表失败（{error}）");
                None
            }
        }
    }

    /// 建表，并且把结构过时的 `tools` 表**整个丢掉重建**。
    ///
    /// 用户工具是随手加的记录，没有值得迁移的东西，所以这里不写 ALTER：
    /// 发现列对不上就删表重建（`SCHEMA` 里是 `IF NOT EXISTS`，删完再跑一遍就建新的）。
    fn prepare(connection: &Connection) -> rusqlite::Result<()> {
        connection.execute_batch(SCHEMA)?;
        let mut columns = Vec::new();
        {
            let mut statement =
                connection.prepare("SELECT name FROM pragma_table_info('tools')")?;
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                columns.push(row.get::<_, String>(0)?);
            }
        }
        let up_to_date = TOOL_COLUMNS
            .iter()
            .all(|wanted| columns.iter().any(|name| name == wanted));
        if !up_to_date {
            eprintln!("tools: 工具表结构变了，丢掉旧的重建");
            connection.execute("DROP TABLE tools", [])?;
            connection.execute_batch(SCHEMA)?;
        }
        Ok(())
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
                    "window_width" => {
                        if let Ok(width) = value.parse() {
                            settings.window_width = width;
                        }
                    }
                    "window_height" => {
                        if let Ok(height) = value.parse() {
                            settings.window_height = height;
                        }
                    }
                    "search_height" => {
                        if let Ok(height) = value.parse() {
                            settings.search_height = height;
                        }
                    }
                    "launch_at_login" => settings.launch_at_login = value == "1",
                    "run_as_admin" => settings.run_as_admin = value == "1",
                    "resume_seconds" => {
                        if let Ok(seconds) = value.parse() {
                            settings.resume_seconds = seconds;
                        }
                    }
                    // 空值写成 "none"：这一行是「用户明确不绑」，不能读成默认值。
                    "summon_shortcut" => {
                        settings.summon_shortcut = (value != SUMMON_NONE).then_some(value);
                    }
                    "calculator_precision" => {
                        if let Ok(precision) = value.parse() {
                            settings.calculator_precision = precision;
                        }
                    }
                    "matcher_calculator" => settings.matcher_calculator = value == "1",
                    "matcher_amount" => settings.matcher_amount = value == "1",
                    "matcher_url" => settings.matcher_url = value == "1",
                    "matcher_path" => settings.matcher_path = value == "1",
                    "firefox_profile" if !value.is_empty() => {
                        settings.firefox_profile = Some(value)
                    }
                    "github_user" => settings.github_user = value,
                    "github_token" => settings.github_token = value,
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

        {
            let mut statement = connection.prepare(
                "SELECT id, kind, name, target, enabled, uses, shortcut, open_default, start_dir \
                 FROM tools ORDER BY id",
            )?;
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                let Some(kind) = ToolKind::from_key(&row.get::<_, String>(1)?) else {
                    // 认不出的种类（手改过的库、或降级运行）当它不存在。
                    continue;
                };
                found = true;
                settings.tools.push(UserTool {
                    id: row.get(0)?,
                    kind,
                    name: row.get(2)?,
                    target: row.get(3)?,
                    enabled: row.get::<_, i64>(4)? != 0,
                    uses: row.get::<_, i64>(5)?.max(0) as u64,
                    shortcut: row.get(6)?,
                    open_default: row.get::<_, i64>(7)? != 0,
                    start_dir: row.get(8)?,
                });
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
            insert.execute(params![
                "run_as_admin",
                u8::from(self.run_as_admin).to_string()
            ])?;
            insert.execute(params!["resume_seconds", self.resume_seconds.to_string()])?;
            insert.execute(params![
                "summon_shortcut",
                self.summon_shortcut.as_deref().unwrap_or(SUMMON_NONE)
            ])?;
            insert.execute(params![
                "calculator_precision",
                self.calculator_precision.to_string()
            ])?;
            insert.execute(params![
                "matcher_calculator",
                u8::from(self.matcher_calculator).to_string()
            ])?;
            insert.execute(params![
                "matcher_amount",
                u8::from(self.matcher_amount).to_string()
            ])?;
            insert.execute(params![
                "matcher_url",
                u8::from(self.matcher_url).to_string()
            ])?;
            insert.execute(params![
                "matcher_path",
                u8::from(self.matcher_path).to_string()
            ])?;
            if let Some(profile) = self.firefox_profile.as_deref() {
                insert.execute(params!["firefox_profile", profile])?;
            }
            insert.execute(params!["github_user", self.github_user.as_str()])?;
            insert.execute(params!["github_token", self.github_token.as_str()])?;
            if let Some(y) = self.window_y {
                insert.execute(params!["window_y", y.to_string()])?;
            }
            insert.execute(params!["window_width", self.window_width.to_string()])?;
            insert.execute(params!["window_height", self.window_height.to_string()])?;
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

        // 工具条目本来就少，和命令一样整表重写：不用维护「改哪行、删哪行」的账。
        transaction.execute("DELETE FROM tools", [])?;
        {
            let mut insert = transaction.prepare(
                "INSERT INTO tools (id, kind, name, target, enabled, uses, shortcut, open_default, start_dir) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )?;
            for tool in &self.tools {
                insert.execute(params![
                    tool.id,
                    tool.kind.key(),
                    tool.name,
                    tool.target,
                    i64::from(tool.enabled),
                    tool.uses as i64,
                    tool.shortcut.as_deref(),
                    i64::from(tool.open_default),
                    tool.start_dir.as_deref(),
                ])?;
            }
        }
        transaction.commit()
    }

    /// 按 id 取一条用户工具。
    pub fn tool(&self, id: i64) -> Option<&UserTool> {
        self.tools.iter().find(|tool| tool.id == id)
    }

    /// 某一种工具的全部条目，顺序和存的一样（按 id 升序）。
    pub fn tools_of(&self, kind: ToolKind) -> impl Iterator<Item = &UserTool> {
        self.tools.iter().filter(move |tool| tool.kind == kind)
    }

    /// 加一条工具，返回新 id。id 只增不回收：删掉的 id 不会被后来者占用。
    pub fn add_tool(&mut self, kind: ToolKind, name: String, target: String) -> i64 {
        let id = self.next_tool_id();
        self.tools.push(UserTool {
            id,
            kind,
            name,
            target,
            enabled: true,
            uses: 0,
            shortcut: None,
            open_default: false,
            start_dir: None,
        });
        id
    }

    /// 改名字和路径/命令，返回这条工具还在不在。
    pub fn update_tool(&mut self, id: i64, name: String, target: String) -> bool {
        let Some(tool) = self.tools.iter_mut().find(|tool| tool.id == id) else {
            return false;
        };
        tool.name = name;
        tool.target = target;
        true
    }

    /// 文件条目：用默认应用打开，还是打开所在位置。
    pub fn set_tool_open_default(&mut self, id: i64, open_default: bool) {
        if let Some(tool) = self.tools.iter_mut().find(|tool| tool.id == id) {
            tool.open_default = open_default;
        }
    }

    /// 终端条目：工作目录；空串和 `None` 都表示用户文件夹。
    pub fn set_tool_start_dir(&mut self, id: i64, dir: Option<String>) {
        if let Some(tool) = self.tools.iter_mut().find(|tool| tool.id == id) {
            tool.start_dir = dir.filter(|dir| !dir.trim().is_empty());
        }
    }

    /// 删掉一条工具，返回删掉的那条。
    pub fn remove_tool(&mut self, id: i64) -> Option<UserTool> {
        let index = self.tools.iter().position(|tool| tool.id == id)?;
        Some(self.tools.remove(index))
    }

    /// 把工具放上主页，或从主页拿掉。
    pub fn set_tool_enabled(&mut self, id: i64, enabled: bool) {
        if let Some(tool) = self.tools.iter_mut().find(|tool| tool.id == id) {
            tool.enabled = enabled;
        }
    }

    /// 给工具绑一个全局快捷键，或者解绑。
    pub fn set_tool_shortcut(&mut self, id: i64, shortcut: Option<String>) {
        if let Some(tool) = self.tools.iter_mut().find(|tool| tool.id == id) {
            tool.shortcut = shortcut;
        }
    }

    /// 给这条工具记一次运行。
    pub fn count_tool_use(&mut self, id: i64) {
        if let Some(tool) = self.tools.iter_mut().find(|tool| tool.id == id) {
            tool.uses = tool.uses.saturating_add(1);
        }
    }

    /// 下一条工具的 id：现有最大值 + 1。空表从 1 开始。
    fn next_tool_id(&self) -> i64 {
        self.tools.iter().map(|tool| tool.id).max().unwrap_or(0) + 1
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

    /// 改唤出面板的热键；`None` 表示以后不绑。
    pub fn set_summon_shortcut(&mut self, shortcut: Option<String>) {
        self.summon_shortcut = shortcut;
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
        self.calculator_precision = self.calculator_precision.clamp(
            Self::CALCULATOR_PRECISION_MIN,
            Self::CALCULATOR_PRECISION_MAX,
        );
        self.window_width = self
            .window_width
            .clamp(Self::WINDOW_WIDTH_MIN, Self::WINDOW_WIDTH_MAX);
        self.window_height = self
            .window_height
            .clamp(Self::WINDOW_HEIGHT_MIN, Self::WINDOW_HEIGHT_MAX);
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            translucent: false,
            panel_opacity: Self::PANEL_OPACITY_DEFAULT,
            window_y: None,
            window_width: Self::WINDOW_WIDTH_DEFAULT,
            window_height: Self::WINDOW_HEIGHT_DEFAULT,
            search_height: Self::SEARCH_HEIGHT_DEFAULT,
            launch_at_login: false,
            run_as_admin: false,
            resume_seconds: 0,
            summon_shortcut: Some(Settings::SUMMON_DEFAULT.to_string()),
            calculator_precision: Self::CALCULATOR_PRECISION_DEFAULT,
            matcher_calculator: true,
            matcher_amount: true,
            matcher_url: true,
            matcher_path: true,
            firefox_profile: None,
            github_user: String::new(),
            github_token: String::new(),
            commands: BTreeMap::new(),
            tools: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CommandSettings, Settings, Theme, ToolKind};
    use rusqlite::Connection;

    fn database() -> Connection {
        let connection = Connection::open_in_memory().expect("内存数据库");
        Settings::prepare(&connection).expect("建表");
        connection
    }

    #[test]
    fn an_old_tool_table_is_dropped_and_rebuilt() {
        let connection = Connection::open_in_memory().expect("内存数据库");
        // 旧结构：没有 open_default / start_dir。
        connection
            .execute_batch(
                "CREATE TABLE tools (
                    id INTEGER PRIMARY KEY NOT NULL,
                    kind TEXT NOT NULL,
                    name TEXT NOT NULL,
                    target TEXT NOT NULL,
                    enabled INTEGER NOT NULL,
                    uses INTEGER NOT NULL,
                    shortcut TEXT
                );
                INSERT INTO tools VALUES (1, 'shortcut', '旧', 'C:/a', 1, 0, NULL);",
            )
            .expect("旧库");

        Settings::prepare(&connection).expect("重建");
        let settings = Settings::read(&connection)
            .expect("读取")
            .unwrap_or_default();
        // 旧行跟着旧表一起没了：这类条目重加一次就行，不写迁移。
        assert!(settings.tools.is_empty());

        let mut settings = Settings::default();
        settings.add_tool(ToolKind::File, "新".to_string(), "C:/b".to_string());
        settings.write(&connection).expect("写入新结构");
        assert_eq!(
            Settings::read(&connection)
                .expect("读取")
                .expect("有数据")
                .tools,
            settings.tools
        );
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
        settings.set_summon_shortcut(Some("ctrl+alt+Space".to_string()));
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

    #[test]
    fn a_panel_size_survives_a_round_trip() {
        let mut settings = Settings::default();
        settings.window_width = 900;
        settings.window_height = 640;
        let connection = database();
        settings.write(&connection).expect("写入");
        let read = Settings::read(&connection).expect("读取").expect("有数据");
        assert_eq!(read.window_width, 900);
        assert_eq!(read.window_height, 640);
    }

    #[test]
    fn an_out_of_range_panel_size_is_pulled_back() {
        let connection = database();
        connection
            .execute(
                "INSERT INTO settings (key, value) VALUES ('window_width', '10'), ('window_height', '99999')",
                [],
            )
            .expect("插入");
        let mut settings = Settings::read(&connection).expect("读取").expect("有数据");
        settings.tidy();
        assert_eq!(settings.window_width, Settings::WINDOW_WIDTH_MIN);
        assert_eq!(settings.window_height, Settings::WINDOW_HEIGHT_MAX);
    }

    #[test]
    fn calculator_precision_survives_a_round_trip() {
        let mut settings = Settings::default();
        settings.calculator_precision = 4;
        let connection = database();
        settings.write(&connection).expect("写入");
        let read = Settings::read(&connection).expect("读取").expect("有数据");
        assert_eq!(read.calculator_precision, 4);
    }

    #[test]
    fn an_out_of_range_calculator_precision_is_pulled_back() {
        let connection = database();
        connection
            .execute(
                "INSERT INTO settings (key, value) VALUES ('calculator_precision', '99')",
                [],
            )
            .expect("插入");
        let mut settings = Settings::read(&connection).expect("读取").expect("有数据");
        settings.tidy();
        assert_eq!(
            settings.calculator_precision,
            Settings::CALCULATOR_PRECISION_MAX
        );
    }

    #[test]
    fn matcher_toggles_survive_a_round_trip() {
        let mut settings = Settings::default();
        settings.matcher_amount = false;
        settings.matcher_path = false;
        let connection = database();
        settings.write(&connection).expect("写入");
        let read = Settings::read(&connection).expect("读取").expect("有数据");
        assert!(!read.matcher_amount);
        assert!(!read.matcher_path);
        assert!(read.matcher_calculator);
        assert!(read.matcher_url);
    }

    #[test]
    fn the_browser_profile_and_github_identity_survive_a_round_trip() {
        let mut settings = Settings::default();
        settings.firefox_profile = Some("D:/Profiles/abc.default".to_string());
        settings.github_user = "cz-97".to_string();
        settings.github_token = "ghp_example".to_string();
        let connection = database();
        settings.write(&connection).expect("写入");
        let read = Settings::read(&connection).expect("读取").expect("有数据");
        assert_eq!(read, settings);
    }

    #[test]
    fn a_missing_browser_profile_means_automatic_detection() {
        let settings = Settings::default();
        let connection = database();
        settings.write(&connection).expect("写入");
        let read = Settings::read(&connection).expect("读取").expect("有数据");
        assert_eq!(read.firefox_profile, None);
        assert_eq!(read.github_user, "");
    }

    #[test]
    fn an_unbound_summon_shortcut_stays_unbound_across_a_round_trip() {
        let mut settings = Settings::default();
        settings.set_summon_shortcut(None);
        let connection = database();
        settings.write(&connection).expect("写入");
        let read = Settings::read(&connection).expect("读取").expect("有数据");
        assert_eq!(read.summon_shortcut, None);
    }

    #[test]
    fn a_database_without_a_summon_line_reads_the_default() {
        // 老库没有这一行：应当读成默认的 Ctrl + Space，而不是「不绑」。
        let connection = database();
        connection
            .execute(
                "INSERT INTO settings (key, value) VALUES ('theme', 'dark')",
                [],
            )
            .expect("插入");
        let settings = Settings::read(&connection).expect("读取").expect("有数据");
        assert_eq!(
            settings.summon_shortcut.as_deref(),
            Some(Settings::SUMMON_DEFAULT)
        );
    }

    #[test]
    fn user_tools_survive_a_round_trip() {
        let mut settings = Settings::default();
        let file = settings.add_tool(
            ToolKind::File,
            "账单".to_string(),
            r"D:\文档\账单.xlsx".to_string(),
        );
        let terminal = settings.add_tool(
            ToolKind::Terminal,
            "构建".to_string(),
            "cargo build".to_string(),
        );
        settings.set_tool_enabled(file, false);
        settings.set_tool_open_default(file, true);
        settings.set_tool_shortcut(terminal, Some("ctrl+alt+KeyB".to_string()));
        settings.set_tool_start_dir(terminal, Some(r"D:\code\tools".to_string()));
        settings.count_tool_use(terminal);
        settings.count_tool_use(terminal);

        let connection = database();
        settings.write(&connection).expect("写入");
        let read = Settings::read(&connection).expect("读取").expect("有数据");
        assert_eq!(read.tools, settings.tools);

        let restored = read.tool(terminal).expect("终端条目还在");
        assert_eq!(restored.kind, ToolKind::Terminal);
        assert_eq!(restored.target, "cargo build");
        assert_eq!(restored.uses, 2);
        assert_eq!(restored.shortcut.as_deref(), Some("ctrl+alt+KeyB"));
        assert_eq!(restored.start_dir.as_deref(), Some(r"D:\code\tools"));

        let restored = read.tool(file).expect("文件条目还在");
        assert!(!restored.enabled);
        assert!(restored.open_default, "「用默认应用打开」这一项得跟着存");
        assert_eq!(restored.start_dir, None);
    }

    #[test]
    fn removing_a_tool_leaves_the_rest_alone() {
        let mut settings = Settings::default();
        let first = settings.add_tool(ToolKind::File, "一".to_string(), "a.txt".to_string());
        let second = settings.add_tool(ToolKind::Terminal, "二".to_string(), "dir".to_string());
        assert!(settings.remove_tool(first).is_some());
        assert!(settings.remove_tool(first).is_none());
        assert_eq!(settings.tools.len(), 1);
        assert!(settings.tool(second).is_some());
        // id 不回收：下一条不能顶掉刚删掉的那个号。
        let third = settings.add_tool(ToolKind::Terminal, "三".to_string(), "cd".to_string());
        assert!(third > second);
    }

    #[test]
    fn a_tool_keeps_its_kind_on_the_way_back() {
        let mut settings = Settings::default();
        settings.add_tool(ToolKind::File, "文件".to_string(), "C:/a".to_string());
        settings.add_tool(ToolKind::Folder, "文件夹".to_string(), "C:/b".to_string());
        settings.add_tool(
            ToolKind::Terminal,
            "终端".to_string(),
            "ipconfig".to_string(),
        );
        let connection = database();
        settings.write(&connection).expect("写入");
        let read = Settings::read(&connection).expect("读取").expect("有数据");
        assert_eq!(read.tools_of(ToolKind::File).count(), 1);
        assert_eq!(read.tools_of(ToolKind::Folder).count(), 1);
        let terminal = read
            .tools_of(ToolKind::Terminal)
            .next()
            .expect("有一条终端");
        assert_eq!(terminal.target, "ipconfig");
    }

    #[test]
    fn an_empty_start_dir_means_the_user_folder() {
        let mut settings = Settings::default();
        let id = settings.add_tool(ToolKind::Terminal, "看".to_string(), "dir".to_string());
        settings.set_tool_start_dir(id, Some("   ".to_string()));
        assert_eq!(settings.tool(id).expect("还在").start_dir, None);
        settings.set_tool_start_dir(id, Some(r"C:\Windows".to_string()));
        assert_eq!(
            settings.tool(id).expect("还在").start_dir.as_deref(),
            Some(r"C:\Windows")
        );
    }
}
