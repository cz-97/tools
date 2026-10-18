//! GPUI Tools：一个用快捷键唤出的 Windows 启动器。
//!
//! 面板本身就是一个搜索框加一张命令列表：回车执行，Esc 收起。设置画在同一个面板里，
//! 页头同时充当拖动区，所以搜索框由面板自己持有，而不是交给列表。

// 发布版是窗口程序：双击、或从快捷方式启动，都不能闪出一个控制台。
// 调试版保留控制台，工作时的标准错误报告才看得见。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actions;
mod app;
mod catalogue;
mod shortcuts;
mod strings;
mod tray_theme;
mod ui;

fn main() {
    app::launch();
}
