//! 按用户自己的设置去打开文件夹和链接。
//!
//! 点名某个程序——`explorer.exe`、某个浏览器——等于把用户随时能改的选择钉死。
//! 这里改用 `ShellExecuteW` 让 shell 去解析关联：文件夹用系统登记的文件管理器打开，
//! 链接用默认浏览器打开。
//!
//! 这一步放在单独的线程里。`ShellExecuteW` 会跑 shell 自己的消息泵，而这个泵在 GPUI
//! 还持着 App 借用的时候把事件送回面板，就会 `RefCell already borrowed`，进而
//! `panic in a function that cannot unwind`，进程直接 abort。这个调用还会一直阻塞到
//! 处理器接手。开一个初始化过 COM 的短命线程，两件事都离开了 UI 线程。

use std::ffi::OsStr;
use std::path::Path;

/// 让 shell 用默认处理器打开 `target`。
///
/// 立刻返回；失败由那个尝试打开的线程写到标准错误。
pub fn open(target: impl AsRef<OsStr>) {
    let target = target.as_ref().to_owned();
    std::thread::spawn(move || {
        sys::prepare();
        if !sys::open(&target) {
            eprintln!(
                "tools: could not open {}",
                std::path::Path::new(&target).display()
            );
        }
    });
}

/// 打开 `path` 的「位置」：是文件夹就打开它，是文件就打开它所在的文件夹。
///
/// 同样交给 shell 解析关联，用户换过文件管理器也照样是他选的那个。
pub fn open_location(path: impl AsRef<Path>) {
    let path = path.as_ref().to_path_buf();
    std::thread::spawn(move || {
        sys::prepare();
        let Some(folder) = sys::location(&path) else {
            eprintln!("tools: {} 打不开，路径已经不在", path.display());
            return;
        };
        if !sys::open(folder.as_os_str()) {
            eprintln!("tools: could not open {}", folder.display());
        }
    });
}

/// 在终端里跑一条命令，返回「这次请求收下了没有」。
///
/// 用 Windows Terminal（`wt.exe`）：给它 `-d <目录>` 和一句 `pwsh -NoExit -Command`，
/// 跑完窗口留着。
///
/// 提权下的坑（这就是为什么这里分两条路）：
///
/// **非管理员**：直接 `CreateProcess` 起 wt 就行。
///
/// **管理员**：同样直接起，wt 的参数会在「应用执行别名」那一跳里被吃掉（Windows 自己的
/// bug），终端起来了但进去是一条空 shell。这时得走 shell 的 `runas` 动词
/// （`ShellExecuteW`），它走的是另一条激活路径，参数能原样到 WT。而且调用者**已经提权**，
/// runas 不会再问一次 UAC。
///
/// 用 `powershell -Command "Start-Process -Verb RunAs wt ..."` 也能达到同样的效果，
/// 但 powershell 是控制台程序，从提权进程里 spawn 会**先闪一个黑框**——这里省掉它，
/// 直接在进程内调 shell。
///
/// 判断用「当前进程提权没有」（`admin::is_elevated`），不是「设置里开没开管理员模式」：
/// 开关开着但眼前这个进程还是普通权限时，硬走 runas 会弹 UAC。
///
/// 没装 Windows Terminal（`wt.exe` 不在 PATH 上）就退回 `cmd /K` + 新控制台，由系统的
/// 默认终端接手——那条路不用挑程序，但提权下会退化成 conhost。
pub fn run_in_terminal(command: &str, dir: Option<&Path>) -> bool {
    let command = command.trim();
    if command.is_empty() {
        return false;
    }
    let command = command.to_string();
    let cwd = dir
        .map(Path::to_path_buf)
        .filter(|path| path.is_dir())
        .or_else(|| std::env::var_os("USERPROFILE").map(std::path::PathBuf::from));

    // 和 `open` 一样丢到短命线程：ShellExecute 会跑自己的消息泵，不能在 UI 线程上跑。
    std::thread::spawn(move || {
        if open_terminal(&command, cwd.as_deref()) {
            return;
        }
        // 兜底：老路，新控制台交给系统的默认终端。
        if let Err(error) = sys::spawn_console("cmd", &command, cwd.as_deref()) {
            eprintln!("tools: 终端没能打开（{error}）");
        }
    });
    true
}

/// 开一个 Windows Terminal 窗口；没装 wt 或者起不来就返回 `false`。
fn open_terminal(command: &str, dir: Option<&Path>) -> bool {
    sys::prepare();
    if !sys::on_path("wt.exe") {
        return false;
    }
    // 有 pwsh 用 pwsh（PowerShell 7），没有就 Windows 自带的 powershell。
    let shell = if sys::on_path("pwsh.exe") {
        "pwsh.exe"
    } else {
        "powershell.exe"
    };
    let args = terminal_arguments(shell, command, dir);

    if super::admin::is_elevated() {
        // 提权：交给 shell 的 runas 动词，参数不会被吞，也不再问 UAC。
        let parameters = args
            .iter()
            .map(|arg| quote_argument(arg))
            .collect::<Vec<_>>()
            .join(" ");
        super::admin::log(&format!("提权开终端：wt.exe {parameters}"));
        return sys::execute("runas", "wt.exe", &parameters);
    }

    std::process::Command::new("wt.exe")
        .args(&args)
        .spawn()
        .map(|_| true)
        .unwrap_or_else(|error| {
            eprintln!("tools: wt 起不来（{error}）");
            false
        })
}

/// 交给 wt 的那串参数：`[-d <目录>] <shell> -NoLogo -NoExit -Command <命令>`。
///
/// 单独拎出来是为了能测：提权那条路要把它们拼成**一个**命令行字符串，引号错一个，
/// 里面那条命令就废了。
fn terminal_arguments(shell: &str, command: &str, dir: Option<&Path>) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(dir) = dir {
        args.push("-d".to_string());
        args.push(dir.display().to_string());
    }
    args.extend([
        shell.to_string(),
        "-NoLogo".to_string(),
        "-NoExit".to_string(),
        "-Command".to_string(),
        command.to_string(),
    ]);
    args
}

/// 拼 `ShellExecute` 的参数串：带空白或引号的字段套一对双引号，内部引号转义。
fn quote_argument(value: &str) -> String {
    if !value
        .chars()
        .any(|character| character.is_whitespace() || character == '"')
    {
        return value.to_string();
    }
    let escaped = value.replace('"', "\\\"");
    // 以反斜杠结尾时必须再补一个：`"dir D:\"` 里的 `\"` 会被当成转义引号，
    // 收尾的那个引号就没了。这是 Windows 命令行引号规则里最经典的坑。
    let tail = if escaped.ends_with('\\') { "\\" } else { "" };
    format!("\"{escaped}{tail}\"")
}

/// 命令的第一个词如果是磁盘上的一个程序，返回它的完整路径。
///
/// 取图标用：`ipconfig` → `C://Windows//System32//ipconfig.exe`。不是程序（`cd`、`dir`
/// 这类内建）或者找不到，返回 `None`。
pub fn program_of(command: &str) -> Option<std::path::PathBuf> {
    let (program, _) = split_first_token(command);
    sys::resolve_program(program)
}

/// 静默跑一条命令：**不开窗口**，进程跑完就没了。
///
/// 给「本来就有界面、或者不需要看输出」的命令用：那些 `*.cpl` 一类的系统工具、
/// 重启关机之类。
///
/// 分两条路，**能不走 cmd 就不走**：
///
/// 1. 命令的第一个词能解析成一个真的 `.exe`/`.com`，而且整条命令里没有 shell 语法
///    （管道、重定向、`&&`、`%变量%`）——直接 `CreateProcess` 那个程序，加
///    `CREATE_NO_WINDOW`。多数「静默」命令（`rundll32 <某个 cpl>`、`ipconfig`、
///    `shutdown /r`）都在这一类，中间**没有 cmd 这个进程**。
/// 2. 其余情况（`dir`、`echo x > f.txt`、带管道）才上 `cmd /C`：那些是 cmd 自己的
///    内建命令和语法，除了 cmd 没人认。
///
/// **这里不能用 `Win+R` 那套 `ShellExecute`**：Win+R 确实是把整串丢给 shell 解析，
/// 但它给控制台程序开会话时是**可见**的窗口（`SW_SHOWNORMAL`），静默模式要的正是
/// 「连黑框都别闪」，所以只能 `CreateProcess` + `CREATE_NO_WINDOW`。
pub fn run_quiet(command: &str, dir: Option<&Path>) -> bool {
    let command = command.trim();
    if command.is_empty() {
        return false;
    }
    let command = command.to_string();
    let cwd = dir
        .map(Path::to_path_buf)
        .filter(|path| path.is_dir())
        .or_else(|| std::env::var_os("USERPROFILE").map(std::path::PathBuf::from));

    // 起进程很快，但 `CreateProcess` 也会碰 shell 的解析；和别处一样放短命线程。
    std::thread::spawn(move || {
        if !needs_shell(&command)
            && let Some((program, arguments)) = direct_target(&command)
        {
            match sys::spawn_hidden(&program, arguments, cwd.as_deref()) {
                Ok(()) => return,
                Err(error) => {
                    eprintln!(
                        "tools: 直接起 {} 失败（{error}），改用 cmd",
                        program.display()
                    )
                }
            }
        }
        if let Err(error) = sys::spawn_hidden("cmd", &format!("/C {command}"), cwd.as_deref()) {
            eprintln!("tools: 命令没能执行（{error}）");
        }
    });
    true
}

/// 这条命令是不是非得 cmd 不可。
///
/// 管道 / 重定向 / 命令串联 / 变量展开都是 cmd（或另一个 shell）自己的语法，直接
/// `CreateProcess` 只会把它们当普通参数塞给程序，行为完全不对，所以一律退回 cmd。
fn needs_shell(command: &str) -> bool {
    command.contains(['&', '|', '<', '>', '^', '%', '\n'])
}

/// 命令能直接起的话，返回（可执行文件的路径，剩下的参数）。
fn direct_target(command: &str) -> Option<(std::path::PathBuf, &str)> {
    let (program, arguments) = split_first_token(command);
    let path = sys::resolve_program(program)?;
    Some((path, arguments))
}

/// 拆出命令的第一个词和剩下的部分。第一个词可能带引号（路径里有空格）。
fn split_first_token(command: &str) -> (&str, &str) {
    let command = command.trim_start();
    if let Some(rest) = command.strip_prefix('"')
        && let Some(end) = rest.find('"')
    {
        return (&rest[..end], rest[end + 1..].trim_start());
    }
    match command.find(char::is_whitespace) {
        Some(end) => (&command[..end], command[end..].trim_start()),
        None => (command, ""),
    }
}

#[cfg(windows)]
mod sys {
    use std::ffi::OsStr;
    use std::path::{Path, PathBuf};
    use std::ptr;
    use windows_sys::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    /// `ShellExecuteW` 用「大于 32」表示成功；更小的值是它自己的错误码。
    const SHELL_SUCCESS: isize = 32;
    /// `CreateProcess` 的「开一个新的控制台窗口」标志。
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;

    /// 把这个线程设成 shell 期待的单线程套间。
    ///
    /// 失败也不致命：照样能打开，只是少了需要 COM 的那些 shell 扩展。
    pub fn prepare() {
        unsafe {
            CoInitializeEx(ptr::null(), COINIT_APARTMENTTHREADED as u32);
        }
    }

    fn wide(text: &OsStr) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt as _;
        text.encode_wide().chain(std::iter::once(0)).collect()
    }

    pub fn open(target: &OsStr) -> bool {
        let verb = wide(OsStr::new("open"));
        let file = wide(target);
        let result = unsafe {
            ShellExecuteW(
                ptr::null_mut(),
                verb.as_ptr(),
                file.as_ptr(),
                ptr::null(),
                ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        result as isize > SHELL_SUCCESS
    }

    /// 用 shell 的指定动词启动一个程序；`runas` 就是「提权启动」。
    ///
    /// 和 [`open`] 一个路子，只是能带参数：`wt.exe` 这种「应用执行别名」在提权进程里
    /// 直接 `CreateProcess` 会把参数吃掉，走 shell 就不会。
    pub fn execute(verb: &str, program: &str, parameters: &str) -> bool {
        execute_in(verb, program, parameters, None)
    }

    /// 同 [`execute`]，外加一个工作目录。
    pub fn execute_in(verb: &str, program: &str, parameters: &str, dir: Option<&Path>) -> bool {
        let verb = wide(OsStr::new(verb));
        let file = wide(OsStr::new(program));
        let parameters = wide(OsStr::new(parameters));
        let directory = dir.map(|dir| wide(dir.as_os_str()));
        let result = unsafe {
            ShellExecuteW(
                ptr::null_mut(),
                verb.as_ptr(),
                file.as_ptr(),
                parameters.as_ptr(),
                directory.as_ref().map_or(ptr::null(), |dir| dir.as_ptr()),
                SW_SHOWNORMAL,
            )
        };
        result as isize > SHELL_SUCCESS
    }

    /// PATH 上有没有这个程序。`wt.exe` / `pwsh.exe` 都是应用执行别名，`is_file` 照样真。
    pub fn on_path(name: &str) -> bool {
        std::env::var_os("PATH")
            .map(|paths| {
                std::env::split_paths(&paths).any(|directory| directory.join(name).is_file())
            })
            .unwrap_or(false)
    }

    /// 该打开哪个文件夹：文件夹返回它自己，文件返回它的上级目录。
    ///
    /// 路径不在了返回 `None`——这种情况由界面上那条提示去问用户删还是改。
    pub fn location(path: &Path) -> Option<PathBuf> {
        let metadata = std::fs::metadata(path).ok()?;
        if metadata.is_dir() {
            return Some(path.to_path_buf());
        }
        path.parent().map(Path::to_path_buf)
    }

    pub fn spawn_console(program: &str, command: &str, dir: Option<&Path>) -> std::io::Result<()> {
        use std::os::windows::process::CommandExt as _;
        let mut builder = std::process::Command::new(program);
        // `raw_arg` 原样拼在命令行后面：`/K` 后面跟的是用户写的那一整条命令，
        // 中间再加一层引号，带空格的路径就会被 cmd 自己吃掉一层。
        builder.arg("/K").raw_arg(command);
        if let Some(dir) = dir {
            builder.current_dir(dir);
        }
        builder.creation_flags(CREATE_NEW_CONSOLE).spawn()?;
        Ok(())
    }

    /// 静默跑：`CreateProcess` + `CREATE_NO_WINDOW`（全程没有窗口），输出直接扔掉。
    ///
    /// `program` 可以是 `cmd` 这样的名字，也可以是解析好的完整路径；`arguments` 原样拼在
    /// 命令行后面（cmd 的 `/C <命令>` 就是这么塞进去的）。
    pub fn spawn_hidden(
        program: impl AsRef<OsStr>,
        arguments: &str,
        dir: Option<&Path>,
    ) -> std::io::Result<()> {
        use std::os::windows::process::CommandExt as _;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut builder = std::process::Command::new(program.as_ref());
        builder
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        // 空参数别拼：`app.exe ""` 会给程序塞一个空参数，不是所有程序都受得了。
        if !arguments.is_empty() {
            builder.raw_arg(arguments);
        }
        if let Some(dir) = dir {
            builder.current_dir(dir);
        }
        builder.creation_flags(CREATE_NO_WINDOW).spawn()?;
        Ok(())
    }

    /// 把命令的第一个词解析成一个真实存在的可执行文件（`.exe` / `.com`）。
    ///
    /// 解析不出来就返回 `None`：`dir`、`echo` 这些是 cmd 的内建命令，磁盘上根本没有
    /// 对应文件，调用方据此退回 `cmd /C`。
    pub fn resolve_program(token: &str) -> Option<PathBuf> {
        fn executable(path: &Path) -> bool {
            path.is_file()
                && path.extension().is_some_and(|extension| {
                    let extension = extension.to_string_lossy().to_ascii_lowercase();
                    extension == "exe" || extension == "com"
                })
        }

        let token = token.trim_matches('"');
        if token.is_empty() {
            return None;
        }
        let path = Path::new(token);
        // 带分隔符（或者盘符）就是路径，不再去 PATH 里找。
        if path.components().count() > 1 || path.is_absolute() {
            return executable(path).then(|| path.to_path_buf());
        }

        let paths = std::env::var_os("PATH")?;
        for directory in std::env::split_paths(&paths) {
            // 用户可能已经写了 `ipconfig.exe`，那就别再补一次后缀。
            let candidates: Vec<String> = if path.extension().is_some() {
                vec![token.to_string()]
            } else {
                vec![format!("{token}.exe"), format!("{token}.com")]
            };
            for name in candidates {
                let candidate = directory.join(name);
                if executable(&candidate) {
                    return Some(candidate);
                }
            }
        }
        None
    }
}

#[cfg(not(windows))]
mod sys {
    use std::ffi::OsStr;
    use std::path::{Path, PathBuf};

    pub fn prepare() {}

    pub fn open(_target: &OsStr) -> bool {
        false
    }

    pub fn location(_path: &Path) -> Option<PathBuf> {
        None
    }

    pub fn spawn_console(
        _program: &str,
        _command: &str,
        _dir: Option<&Path>,
    ) -> std::io::Result<()> {
        Err(std::io::Error::other("这个平台上没有终端可开"))
    }

    pub fn spawn_hidden(
        _program: impl AsRef<OsStr>,
        _arguments: &str,
        _dir: Option<&Path>,
    ) -> std::io::Result<()> {
        Err(std::io::Error::other("这个平台上没有终端可开"))
    }

    pub fn resolve_program(_token: &str) -> Option<PathBuf> {
        None
    }

    pub fn execute(_verb: &str, _program: &str, _parameters: &str) -> bool {
        false
    }

    pub fn execute_in(_verb: &str, _program: &str, _parameters: &str, _dir: Option<&Path>) -> bool {
        false
    }

    pub fn on_path(_name: &str) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::{needs_shell, quote_argument, split_first_token, terminal_arguments};
    use std::path::Path;

    #[test]
    fn terminal_arguments_carry_the_start_directory_and_the_command() {
        let args = terminal_arguments("pwsh.exe", "cargo build", Some(Path::new(r"D:\code\tools")));
        assert_eq!(
            args,
            [
                "-d",
                r"D:\code\tools",
                "pwsh.exe",
                "-NoLogo",
                "-NoExit",
                "-Command",
                "cargo build"
            ]
        );
    }

    #[test]
    fn no_directory_means_no_dash_d() {
        let args = terminal_arguments("pwsh.exe", "dir", None);
        assert_eq!(args.first().map(String::as_str), Some("pwsh.exe"));
    }

    #[test]
    fn arguments_are_quoted_only_when_they_need_it() {
        assert_eq!(quote_argument("pwsh.exe"), "pwsh.exe");
        assert_eq!(quote_argument("cargo build"), "\"cargo build\"");
        assert_eq!(
            quote_argument(r"D:\my code\tools"),
            "\"D:\\my code\\tools\""
        );
        // 里面本来就有引号：转义，别让 shell 提前收尾。
        assert_eq!(quote_argument("echo \"hi\""), "\"echo \\\"hi\\\"\"");
        // 结尾是反斜杠：补一个，否则收尾引号被它吃掉。
        assert_eq!(quote_argument("dir D:\\"), "\"dir D:\\\\\"");
    }

    #[test]
    fn the_first_token_is_split_off_with_or_without_quotes() {
        assert_eq!(split_first_token("ipconfig /all"), ("ipconfig", "/all"));
        assert_eq!(split_first_token("  ipconfig   "), ("ipconfig", ""));
        assert_eq!(
            split_first_token(r#""C:\Program Files\app.exe" -x"#),
            (r"C:\Program Files\app.exe", "-x")
        );
        // 引号没闭合：整条当一个词，交给下面对解析失败的处理。
        assert_eq!(split_first_token("\"oops"), ("\"oops", ""));
    }

    #[test]
    fn shell_syntax_always_goes_through_cmd() {
        for command in [
            "echo hi > out.txt",
            "dir | findstr x",
            "a && b",
            "type %USERPROFILE%\\a.txt",
        ] {
            assert!(needs_shell(command), "{command} 该走 cmd");
        }
        for command in [
            "ipconfig /all",
            r"rundll32 sysdm.cpl,EditEnvironmentVariables",
            r#""C:\Program Files\app.exe" -x"#,
        ] {
            assert!(!needs_shell(command), "{command} 不该走 cmd");
        }
    }
}
