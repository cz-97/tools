//! Windows 平台细节：窗口外壳、单例、开机启动、管理员模式、图标、HTTP、
//! 系统对话框，以及交给 shell 打开。
//!
//! 最底下的一层：这里不知道「设置」「命令」「页面」是什么，也不依赖 `tools-core`。
//! 非 Windows 目标上整个模块退化成空操作，调用方不必分平台写代码。

pub mod admin;
pub mod autostart;
pub mod dialog;
pub mod http;
pub mod instance;
pub mod logo;
pub mod shell;
pub mod vsync;
pub mod window;
