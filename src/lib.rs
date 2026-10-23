//! 启动器各个窗口共用的宿主侧服务。
//!
//! 分成两块：`logic` 是能单独测的纯逻辑，`platform` 是 Windows 平台细节。
//!
//! 规范以仓库根目录的 `AGENTS.md` 为准。

pub mod logic;
pub mod platform;

// 文件已按 `logic` / `platform` 归位，这里把顶层路径重新导出，
// 界面代码继续用 `gpui_tools::settings` 这类老写法，不必跟着搬家。
pub use logic::{cache, calculator, firefox, github, matcher, settings};
pub use platform::{autostart, http, instance, logo, shell, window};
