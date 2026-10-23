//! 界面上出现过的所有文字。
//!
//! 按所属界面分区，一律用常量名（也就是 id）取用，不要在绘制代码里直接写字符串。
//! 改文案只动这个文件；要加界面，就加一个同名子模块。

/// 五个设置子模块的名字。
pub mod sections {
    /// 系统。
    pub const SYSTEM: &str = "系统";
    /// 命令。
    pub const COMMANDS: &str = "命令";
    /// 外观。
    pub const APPEARANCE: &str = "外观";
    /// 计算器。
    pub const CALCULATOR: &str = "计算器";
    /// 工具（计算器这类工具的容器标签）。
    pub const TOOLS: &str = "工具";
    /// 指令匹配。
    pub const MATCHERS: &str = "指令匹配";
    /// 关于。
    pub const ABOUT: &str = "关于";
}

/// 主面板（命令列表）。
pub mod launcher {
    /// 搜索框为空时的提示，同时是拖动区的一部分。
    pub const SEARCH_HINT: &str = "搜索命令\u{2026}";
    /// 页脚：上下键的说明。
    pub const FOOTER_NAVIGATE: &str = "选择";
    /// 页脚：回车的说明。
    pub const FOOTER_OPEN: &str = "执行";
    /// 页脚：Esc 的说明。
    pub const FOOTER_CLOSE: &str = "关闭";
    /// 页脚：设置页里上下键的说明。
    pub const FOOTER_SECTIONS: &str = "切换子模块";
    /// 页脚：设置页里 Esc 的说明。
    pub const FOOTER_BACK: &str = "返回";
    /// 齿轮按钮的悬浮提示。
    pub const TOOLTIP_SETTINGS: &str = "设置";
    /// 设置页左上角返回按钮的悬浮提示。
    pub const TOOLTIP_BACK: &str = "返回";
    /// 设置页页头的标题。
    pub const SETTINGS_TITLE: &str = "设置";
    /// 计算器页页头的标题。
    pub const CALCULATOR_TITLE: &str = "计算器";
    /// 页脚右侧的命令条数。
    pub fn count(commands: usize) -> String {
        format!("{commands} 条命令")
    }
    /// 一条命令都没启用时的空状态。
    pub const EMPTY: &str = "没有可用的命令";
    /// 搜索无结果时的空状态。
    pub fn no_match(query: &str) -> String {
        format!("没有匹配 \u{201c}{query}\u{201d} 的命令")
    }
    /// 主页每行：调用次数。
    pub fn uses(count: u64) -> String {
        format!("{count} 次")
    }
    /// 主页每行：没有绑定快捷键时显示的字。
    pub const NO_SHORTCUT: &str = "无快捷键";
}

/// 命令页（设置里的「命令」标签）。
pub mod commands_page {
    /// 右侧列表上方的一行标题。顺带把快捷键按钮的两个操作说清楚：
    /// 右键没有提示、也没有别的入口，不写在明处就只能靠猜。
    pub fn caption(section: &str) -> String {
        format!("{section}的命令\u{2002}\u{00b7}\u{2002}左键录制，右键清除")
    }
    /// 该子模块下没有任何命令时的一行字。
    pub const EMPTY: &str = "这个子模块还没有命令";
    /// 左侧子模块后面的「启用数/总数」。
    pub fn enabled_ratio(enabled: usize, total: usize) -> String {
        format!("{enabled}/{total}")
    }
    /// 每行的调用次数。
    pub fn uses(count: u64) -> String {
        format!("{count} 次")
    }
    /// 快捷键按钮在没绑定时显示的字。
    pub const SHORTCUT_EMPTY: &str = "快捷键";
    /// 录制快捷键时按钮上显示的字。
    pub const RECORDING: &str = "按下组合键\u{2026}";
    /// 录制时按到一组已经被占用的键：按钮上换成这句话。
    pub fn conflict(owner: &str) -> String {
        format!("已被\u{201c}{owner}\u{201d}占用")
    }
}

/// 计算器页（主程序的一级页面，与设置平级）。
pub mod calculator {
    /// 空行输入框里的提示。
    pub const PLACEHOLDER: &str = "计算公式";
    /// 底部：清空当前在编辑的那一行。
    pub const CLEAR_ROW: &str = "清空本行";
    /// 底部：把所有公式行撤掉，回到一行空行。
    pub const CLEAR_ALL: &str = "清空所有";
    /// 结果被复制走后，那一小会儿显示的字。
    pub const COPIED: &str = "已复制";
    /// 底部左侧的说明：认哪些运算、结果怎么复制。
    pub const HINT: &str = "支持 + - * / ^ 与括号，点结果即可复制";
}

/// 「指令匹配」标签：各匹配的名字与说明，以及命中行上的提示。
pub mod matcher {
    /// 计算表达式：名字。
    pub const CALCULATOR_NAME: &str = "计算表达式";
    /// 计算表达式：说明。
    pub const CALCULATOR_DESCRIPTION: &str = "输入算式时直接显示结果，点一下复制";
    /// 金额大写：名字。
    pub const AMOUNT_NAME: &str = "金额大写";
    /// 金额大写：说明。
    pub const AMOUNT_DESCRIPTION: &str = "输入金额（最多两位小数）显示中文大写，点一下复制";
    /// 网址：名字。
    pub const URL_NAME: &str = "网址";
    /// 网址：说明。
    pub const URL_DESCRIPTION: &str = "输入网址时在浏览器打开";
    /// 文件路径：名字。
    pub const PATH_NAME: &str = "文件路径";
    /// 文件路径：说明。
    pub const PATH_DESCRIPTION: &str = "输入文件或文件夹路径时打开，不存在会提示";
    /// 复制成功后短暂显示的字。
    pub const COPIED: &str = "已复制";
    /// 命中行右侧：显示结果类。
    pub const HINT_COPY: &str = "复制";
    /// 命中行右侧：执行动作类。
    pub const HINT_OPEN: &str = "打开";
}

/// 设置页里的各项文案。
pub mod settings_page {
    /// 系统页：全局快捷键的标题。
    pub const HOTKEY_NAME: &str = "全局快捷键";
    /// 系统页：全局快捷键的说明。
    pub const HOTKEY_DESCRIPTION: &str = "在任意窗口唤出面板；左键录制，右键清除";
    /// 系统页：唤出热键没绑时按钮上的字。
    pub const HOTKEY_UNBOUND: &str = "未绑定";
    /// 系统页：开机启动的标题。
    pub const AUTOSTART_NAME: &str = "开机启动";
    /// 系统页：开机启动的说明。
    pub const AUTOSTART_DESCRIPTION: &str = "登录 Windows 后自动在后台运行";
    /// 系统页：子页面保持的标题。
    pub const RESUME_NAME: &str = "子页面保持";
    /// 系统页：子页面保持的说明。
    pub const RESUME_DESCRIPTION: &str = "隐藏后短时间内再唤出，仍停在原页面";
    /// 系统页：配置文件夹的标题。
    pub const CONFIG_NAME: &str = "配置文件夹";
    /// 系统页：配置文件夹的说明。
    pub const CONFIG_OPEN: &str = "打开";
    /// 系统页：退出启动器的标题。
    pub const QUIT_NAME: &str = "退出启动器";
    /// 系统页：退出启动器的说明。
    pub const QUIT_DESCRIPTION: &str = "关闭面板与托盘";
    /// 系统页：退出启动器的按钮字。
    pub const QUIT_ACTION: &str = "退出";
    /// 外观页：主题的标题。
    pub const THEME_NAME: &str = "主题";
    /// 外观页：主题的说明。
    pub const THEME_DESCRIPTION: &str = "面板、列表与设置页的配色";
    /// 外观页：半透明背景的标题。
    pub const TRANSLUCENT_NAME: &str = "半透明背景";
    /// 外观页：半透明背景的说明。
    pub const TRANSLUCENT_DESCRIPTION: &str = "降低面板底色不透明度；关闭后恢复实色背景";
    /// 外观页：透明度的标题。
    pub const OPACITY_NAME: &str = "透明度";
    /// 外观页：透明度的说明。
    pub const OPACITY_DESCRIPTION: &str = "半透明时底色的深浅，越淡越能看清背后的窗口";
    /// 外观页：透明度的单位。
    pub fn opacity_value(opacity: u32) -> String {
        format!("{opacity}%")
    }
    /// 外观页：搜索框高度的标题。
    pub const HEIGHT_NAME: &str = "搜索框高度";
    /// 外观页：搜索框高度的说明。
    pub const HEIGHT_DESCRIPTION: &str = "输入框的高度，越高越好点中";
    /// 外观页：搜索框高度的单位。
    pub fn height_value(height: u32) -> String {
        format!("{height} px")
    }
    /// 关于页：版本的标题。
    pub const VERSION_NAME: &str = "版本";
    /// 关于页：版本的说明。
    pub const VERSION_DESCRIPTION: &str = "当前运行的程序版本";
    /// 关于页：项目仓库的标题。
    pub const REPOSITORY_NAME: &str = "项目仓库";
    /// 关于页：项目仓库的按钮字。
    pub const REPOSITORY_OPEN: &str = "打开";
    /// 关于页：程序文件夹的标题。
    pub const PROGRAM_FOLDER_NAME: &str = "程序所在位置";
    /// 关于页：程序文件夹的按钮字。
    pub const FOLDER_OPEN: &str = "打开";
    /// 找不到配置目录时的占位字。
    pub const NO_CONFIG_DIRECTORY: &str = "未找到配置目录";
    /// 主题：跟随系统。
    pub const THEME_SYSTEM: &str = "跟随系统";
    /// 主题：浅色。
    pub const THEME_LIGHT: &str = "浅色";
    /// 主题：深色。
    pub const THEME_DARK: &str = "深色";
    /// 计算器设置：精度的标题。
    pub const CALCULATOR_NAME: &str = "计算精度";
    /// 计算器设置：精度的说明。
    pub const CALCULATOR_DESCRIPTION: &str = "结果最多保留的小数位数";
    /// 计算器设置：当前精度的单位。
    pub fn precision_value(precision: u32) -> String {
        format!("{precision} 位")
    }
    /// 工具标签下某个工具的内容标题。
    pub fn tool_caption(name: &str) -> String {
        format!("{name}的设置")
    }
    /// 子页面保持：不保持。
    pub const RESUME_NEVER: &str = "不保持";
    /// 子页面保持：15 秒。
    pub const RESUME_15: &str = "15 秒";
    /// 子页面保持：30 秒。
    pub const RESUME_30: &str = "30 秒";
    /// 子页面保持：1 分钟。
    pub const RESUME_60: &str = "1 分钟";
    /// 子页面保持：5 分钟。
    pub const RESUME_300: &str = "5 分钟";

    /// 子页面保持的每个选项及其文案。
    pub fn resume_label(seconds: u32) -> &'static str {
        match seconds {
            15 => RESUME_15,
            30 => RESUME_30,
            60 => RESUME_60,
            300 => RESUME_300,
            _ => RESUME_NEVER,
        }
    }
}

/// 托盘菜单与提示。
pub mod tray {
    /// 鼠标悬停在托盘图标上的提示。
    pub const TOOLTIP: &str = "GPUI Tools";
    /// 菜单：打开启动器。
    pub const OPEN: &str = "打开启动器";
    /// 菜单：设置。
    pub const SETTINGS: &str = "设置";
    /// 菜单：重启。
    pub const RESTART: &str = "重启";
    /// 菜单：退出。
    pub const QUIT: &str = "退出";
}
