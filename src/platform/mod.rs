//! Windows 平台细节：窗口外壳、单例、开机启动、管理员模式、图标、HTTP、
//! 系统对话框，以及交给 shell 打开。

pub mod admin;
pub mod autostart;
pub mod dialog;
pub mod http;
pub mod instance;
pub mod logo;
pub mod shell;
pub mod vsync;
pub mod window;
