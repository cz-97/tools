//! 面板的各个界面。

pub mod colors;
pub mod commands_list;
pub mod launcher;
pub mod pages;

// 整页视图收进了 `pages/`，旧的 `crate::ui::settings_page` 这类路径继续有效。
pub use pages::{
    calculator as calculator_page, commands as commands_page, settings as settings_page,
};
