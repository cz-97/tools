//! The launcher's command catalogue.
//!
//! A command is one entry with a stable id, a face for the list, the section of
//! the settings it belongs to, and an action. Adding a command is adding one
//! entry here: search, the home list, the commands page, the usage counter and
//! the optional global shortcut all follow from the entry and are keyed by
//! `id`.
//!
//! The sections are the settings sub-modules (系统 / 命令 / 外观 / 关于), so the
//! commands page reads as an index of the settings: a command lives under the
//! page whose subject it acts on.

use gpui_kit::App;
use gpui_kit::component::IconName;

/// The part of the application a command acts on.
///
/// Mirrors the settings sub-modules one for one; the settings tabs take their
/// label and icon from here rather than repeating them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    System,
    Commands,
    Appearance,
    About,
}

impl Section {
    /// Every section, in the order the settings page lists them.
    pub const ALL: [Self; 4] = [Self::System, Self::Commands, Self::Appearance, Self::About];

    /// The label the settings page and the commands page both show.
    pub fn label(self) -> &'static str {
        match self {
            Self::System => "系统",
            Self::Commands => "命令",
            Self::Appearance => "外观",
            Self::About => "关于",
        }
    }

    /// The stable key the database files this section's commands under.
    ///
    /// It is half the primary key of every command row, and it is not the
    /// label: renaming a section must not orphan the counters, shortcuts and
    /// switches its commands already carry.
    pub fn key(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Commands => "commands",
            Self::Appearance => "appearance",
            Self::About => "about",
        }
    }

    /// The section's icon, shared by the settings tabs and the commands page.
    pub fn icon(self) -> IconName {
        match self {
            Self::System => IconName::Cpu,
            Self::Commands => IconName::SquareTerminal,
            Self::Appearance => IconName::Palette,
            Self::About => IconName::Info,
        }
    }
}

/// One command the launcher can run.
pub struct Command {
    /// The stable key everything stored about this command is filed under.
    ///
    /// Renaming the command must not change it, and it must be unique across
    /// the catalogue: the database, the usage counts and the shortcuts all
    /// address the command by this string.
    pub id: &'static str,
    /// The settings sub-module this command belongs to.
    pub section: Section,
    /// The name the list and the commands page show.
    pub name: &'static str,
    pub description: &'static str,
    /// Extra words search matches on, so a command is findable by terms that
    /// do not appear in its name.
    pub keyword: &'static str,
    pub icon: IconName,
    pub action: fn(&mut App),
}

/// Every command, in the order the home list shows them.
pub static COMMANDS: &[Command] = &[
    Command {
        id: "program-folder",
        section: Section::System,
        name: "Program Folder",
        description: "打开启动器所在的文件夹",
        keyword: "program folder directory 程序 目录 文件夹 打开",
        icon: IconName::FolderOpen,
        action: crate::open_program_folder,
    },
    Command {
        id: "repository",
        section: Section::System,
        name: "Repository",
        description: "在浏览器打开项目仓库",
        keyword: "github repository source 仓库 源码 项目",
        icon: IconName::Globe,
        action: crate::open_repository,
    },
    Command {
        id: "quit",
        section: Section::System,
        name: "Quit GPUI Tools",
        description: "退出启动器",
        keyword: "quit exit close 退出 关闭",
        icon: IconName::Close,
        action: crate::quit_launcher,
    },
];

/// The commands of one section, in catalogue order.
pub fn commands(section: Section) -> impl Iterator<Item = &'static Command> {
    COMMANDS
        .iter()
        .filter(move |command| command.section == section)
}
