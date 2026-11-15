//! 启动器各个窗口共用的业务逻辑。
//!
//! 只有 `logic` 一块：能脱离界面单独测的东西都放这儿。碰系统的活（窗口、注册表、
//! HTTP）在 `tools-platform`，这里只管调用。
//!
//! 规范以仓库根目录的 `AGENTS.md` 为准。

pub mod logic;

// 文件已按 `logic` 归位，这里把顶层路径重新导出，
// 本 crate 内继续用 `crate::settings` 这类老写法，不必跟着搬家。
pub use logic::{cache, calculator, firefox, github, matcher, pinyin, settings};
