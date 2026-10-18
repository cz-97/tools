//! 命令目录：整个程序里有哪些命令、各自干什么，都在这一份数据里。
//!
//! 一条命令 = 稳定的 id + 所属子模块 + 展示文字 + 图标 + 动作。加命令就是往这里加一项；
//! 搜索、主页列表、命令页分组、启用开关、调用次数、全局快捷键全部由这一项和它的 `id`
//! 派生出来，不需要动别处。
//!
//! 子模块就是设置里的那四项（系统 / 命令 / 外观 / 关于），所以命令页读起来像设置的
//! 一张索引：命令挂在它作用的那一页下面。

use gpui_kit::App;
use gpui_kit::component::IconName;

use crate::actions;
use crate::strings;

/// 一条命令作用在应用的哪一部分。
///
/// 与设置子模块一一对应；设置的标签和图标都从这里取，不再各写一份。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    System,
    Commands,
    Appearance,
    About,
}

impl Section {
    /// 全部子模块，顺序就是界面上的顺序。
    pub const ALL: [Self; 4] = [Self::System, Self::Commands, Self::Appearance, Self::About];

    /// 界面上显示的名字。
    pub fn label(self) -> &'static str {
        match self {
            Self::System => strings::sections::SYSTEM,
            Self::Commands => strings::sections::COMMANDS,
            Self::Appearance => strings::sections::APPEARANCE,
            Self::About => strings::sections::ABOUT,
        }
    }

    /// 界面上显示的图标。
    pub fn icon(self) -> IconName {
        match self {
            Self::System => IconName::Cpu,
            Self::Commands => IconName::SquareTerminal,
            Self::Appearance => IconName::Palette,
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
    /// 它属于哪个设置子模块。
    pub section: Section,
    /// 列表和命令页显示的名称。
    pub name: &'static str,
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
        name: "设置",
        description: "打开启动器设置",
        keyword: "settings preferences 设置 偏好",
        icon: IconName::Settings,
        action: crate::app::open_settings,
    },
    Command {
        id: "program-folder",
        section: Section::System,
        name: "程序所在文件夹",
        description: "打开启动器所在的文件夹",
        keyword: "program folder directory 程序 目录 文件夹 打开",
        icon: IconName::FolderOpen,
        action: actions::open_program_folder,
    },
    Command {
        id: "repository",
        section: Section::About,
        name: "GitHub仓库",
        description: "在浏览器打开项目仓库",
        keyword: "github repository source 仓库 源码 项目",
        icon: IconName::Globe,
        action: actions::open_repository,
    },
    Command {
        id: "quit",
        section: Section::System,
        name: "退出",
        description: "杀死本程序进程",
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
