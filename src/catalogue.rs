//! The launcher's command catalogue.
//!
//! A module is a named group of commands; a command is one entry with a stable
//! id, a face for the list, and an action. Adding a command is adding one entry
//! here: search, the home list, the commands page, the usage counter and the
//! optional global shortcut all follow from the entry and are keyed by `id`.
//!
//! Nothing outside this file knows which commands exist, so a new module is a
//! new `Module` entry and nothing else.

use gpui_kit::App;
use gpui_kit::component::IconName;

/// One command the launcher can run.
pub struct Command {
    /// The stable key everything stored about this command is filed under.
    ///
    /// Renaming the command must not change it, and it must be unique across
    /// the catalogue: the settings file, the usage counts and the shortcuts all
    /// address the command by this string.
    pub id: &'static str,
    /// The name the list and the commands page show.
    pub name: &'static str,
    pub description: &'static str,
    /// Extra words search matches on, so a command is findable by terms that
    /// do not appear in its name.
    pub keyword: &'static str,
    pub icon: IconName,
    pub action: fn(&mut App),
}

/// A named group of commands, shown as one page of the commands section.
pub struct Module {
    /// The stable key of the group, for the page that shows it.
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub icon: IconName,
    pub commands: &'static [Command],
}

/// Every module, in the order the commands page lists them.
pub static MODULES: &[Module] = &[
    Module {
        id: "files",
        name: "文件",
        description: "本机上的位置",
        icon: IconName::FolderOpen,
        commands: &[Command {
            id: "program-folder",
            name: "Program Folder",
            description: "打开启动器所在的文件夹",
            keyword: "program folder directory 程序 目录 文件夹 打开",
            icon: IconName::FolderOpen,
            action: crate::open_program_folder,
        }],
    },
    Module {
        id: "links",
        name: "链接",
        description: "网络上的目标",
        icon: IconName::Globe,
        commands: &[Command {
            id: "repository",
            name: "Repository",
            description: "在浏览器打开项目仓库",
            keyword: "github repository source 仓库 源码 项目",
            icon: IconName::Globe,
            action: crate::open_repository,
        }],
    },
    Module {
        id: "app",
        name: "应用",
        description: "启动器本身",
        icon: IconName::Frame,
        commands: &[Command {
            id: "quit",
            name: "Quit GPUI Tools",
            description: "退出启动器",
            keyword: "quit exit close 退出 关闭",
            icon: IconName::Close,
            action: crate::quit_launcher,
        }],
    },
];

/// The command with this id, if the catalogue still has it.
///
/// A file written by an older build can name a command that has since gone; the
/// caller is expected to shrug that off.
pub fn find(id: &str) -> Option<(&'static Module, &'static Command)> {
    MODULES
        .iter()
        .flat_map(|module| module.commands.iter().map(move |command| (module, command)))
        .find(|(_, command)| command.id == id)
}
