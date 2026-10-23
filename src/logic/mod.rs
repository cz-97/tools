//! 纯逻辑：不碰窗口、不碰绘制，可以脱离界面单独测试。
//!
//! `firefox` / `github` 会碰磁盘和网络，但同样不碰界面——它们只回数据，
//! 画在哪一页由 `ui` 决定。

pub mod cache;
pub mod calculator;
pub mod firefox;
pub mod github;
pub mod matcher;
pub mod settings;
