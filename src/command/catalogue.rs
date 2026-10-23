//! 命令目录：整个程序里有哪些命令、各自干什么，都在这一份数据里。
//!
//! 一条命令 = 稳定的 id + 所属子模块 + 展示文字 + 图标 + 动作。加命令就是往这里加一项；
//! 搜索、主页列表、命令页分组、启用开关、调用次数、全局快捷键全部由这一项和它的 `id`
//! 派生出来，不需要动别处。
//!
//! 命令页按 `Section` 分组（系统 / 命令 / 外观 / 计算器 / 关于）；名称和说明全部来自
//! `crate::strings`，和设置页里同功能的那一行用同一个词，不要在这里另写一份文案。

use gpui_kit::App;
// 图标用整套 Lucide 而不是组件的子集：计算器图标只在全集里才有。
use gpui_kit::assets::IconName;

use crate::actions;
use crate::strings;

/// 命令页里给命令分组的依据。
///
/// **它不等于设置左侧的标签列表**（那是 `ui::launcher::SettingsTab`）：命令页仍按
/// 系统 / 命令 / 外观 / 计算器 / 关于 分组，和设置页的 六 个标签不必一一对应。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    System,
    Commands,
    Appearance,
    /// 火狐系浏览器的书签与历史。
    Browser,
    /// GitHub 的仓库查询。
    Github,
    Calculator,
    About,
}

impl Section {
    /// 全部子模块，顺序就是界面上的顺序。
    pub const ALL: [Self; 7] = [
        Self::System,
        Self::Commands,
        Self::Appearance,
        Self::Browser,
        Self::Github,
        Self::Calculator,
        Self::About,
    ];

    /// 界面上显示的名字。
    pub fn label(self) -> &'static str {
        match self {
            Self::System => strings::sections::SYSTEM,
            Self::Commands => strings::sections::COMMANDS,
            Self::Appearance => strings::sections::APPEARANCE,
            Self::Browser => strings::sections::BROWSER,
            Self::Github => strings::sections::GITHUB,
            Self::Calculator => strings::sections::CALCULATOR,
            Self::About => strings::sections::ABOUT,
        }
    }

    /// 界面上显示的图标。
    pub fn icon(self) -> IconName {
        match self {
            Self::System => IconName::Cpu,
            Self::Commands => IconName::SquareTerminal,
            Self::Appearance => IconName::Palette,
            Self::Browser => IconName::Globe,
            Self::Github => IconName::Github,
            Self::Calculator => IconName::Calculator,
            Self::About => IconName::Info,
        }
    }

    /// 数据库给这个子模块的命令归档用的键。
    ///
    /// 它是每条命令那一行主键的一半，而且**不是**显示名：改中文名不能把已经记下
    /// 的次数、快捷键和开关弄丢。
    pub fn key(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Commands => "commands",
            Self::Appearance => "appearance",
            Self::Browser => "browser",
            Self::Github => "github",
            Self::Calculator => "calculator",
            Self::About => "about",
        }
    }
}

/// 一条能执行的命令。
pub struct Command {
    /// 关于这条命令的一切存在哪个键下。
    ///
    /// 改名不能动它；它和所属子模块一起构成持久化主键，所以必须唯一。
    pub id: &'static str,
    /// 它在命令页里归到哪个分组。
    pub section: Section,
    /// 列表和命令页显示的名称（来自 `strings`，和设置页同功能的一行同词）。
    pub name: &'static str,
    /// 列表和命令页显示的说明。
    pub description: &'static str,
    /// 搜索时额外匹配的词，让命令能被名字里没有的说法搜到。
    pub keyword: &'static str,
    pub icon: IconName,
    pub action: fn(&mut App),
}

/// 全部命令，顺序就是主页上的顺序。
pub static COMMANDS: &[Command] = &[
    Command {
        id: "open-settings",
        section: Section::System,
        name: strings::common::SETTINGS,
        description: strings::commands::OPEN_SETTINGS_DESCRIPTION,
        keyword: "settings preferences 设置 偏好",
        icon: IconName::Settings,
        action: crate::app::open_settings,
    },
    Command {
        id: "open-calculator",
        section: Section::System,
        name: strings::common::CALCULATOR,
        description: strings::commands::OPEN_CALCULATOR_DESCRIPTION,
        keyword: "calculator calc 计算器 计算 表达式 算数",
        icon: IconName::Calculator,
        action: crate::app::open_calculator,
    },
    Command {
        id: "program-folder",
        section: Section::System,
        name: strings::common::PROGRAM_FOLDER,
        description: strings::commands::PROGRAM_FOLDER_DESCRIPTION,
        keyword: "program folder directory 程序 目录 文件夹 打开",
        icon: IconName::FolderOpen,
        action: actions::open_program_folder,
    },
    Command {
        id: "repository",
        section: Section::About,
        name: strings::common::REPOSITORY,
        description: strings::commands::REPOSITORY_DESCRIPTION,
        keyword: "github repository source 仓库 源码 项目",
        icon: IconName::Globe,
        action: actions::open_repository,
    },
    Command {
        id: "bookmarks",
        section: Section::Browser,
        name: strings::common::BOOKMARKS,
        description: strings::commands::BOOKMARKS_DESCRIPTION,
        keyword: "bookmark bookmarks 书签 收藏 火狐 firefox zen librewolf",
        icon: IconName::Bookmark,
        action: actions::open_bookmarks,
    },
    Command {
        id: "history",
        section: Section::Browser,
        name: strings::common::HISTORY,
        description: strings::commands::HISTORY_DESCRIPTION,
        keyword: "history 历史 记录 浏览过 firefox zen librewolf",
        icon: IconName::Clock,
        action: actions::open_history,
    },
    Command {
        id: "github-projects",
        section: Section::Github,
        name: strings::common::PROJECTS,
        description: strings::commands::PROJECTS_DESCRIPTION,
        keyword: "github repos projects 个人项目 我的仓库 自己的",
        icon: IconName::House,
        action: actions::open_github_projects,
    },
    Command {
        id: "github-stars",
        section: Section::Github,
        name: strings::common::STARS,
        description: strings::commands::STARS_DESCRIPTION,
        keyword: "github stars starred 星标 收藏",
        icon: IconName::Star,
        action: actions::open_github_stars,
    },
    Command {
        id: "github-search",
        section: Section::Github,
        name: strings::common::REPO_SEARCH,
        description: strings::commands::REPO_SEARCH_DESCRIPTION,
        keyword: "github search repositories 搜索 查询 找仓库",
        icon: IconName::Github,
        action: actions::open_github_search,
    },
    Command {
        id: "quit",
        section: Section::System,
        name: strings::common::QUIT,
        description: strings::common::QUIT_DESCRIPTION,
        keyword: "quit exit close 退出 关闭",
        icon: IconName::Close,
        action: actions::quit_launcher,
    },
];

/// 某个子模块下的命令，按目录顺序。
pub fn commands(section: Section) -> impl Iterator<Item = &'static Command> {
    COMMANDS
        .iter()
        .filter(move |command| command.section == section)
}
