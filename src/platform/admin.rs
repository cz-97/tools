//! 以管理员身份启动，而且**不要再弹 UAC**。
//!
//! 普通进程想提权只有一条路：让系统显示授权弹窗。想绕开它，只能提前把「要跑一个
//! 需要提权的程序」这件事登记到系统里——也就是**计划任务**：一条 `RUNLEVEL HIGHEST`
//! 的任务，由任务计划程序替我们启动进程，进程天生就是提权的，用户看不到任何弹窗。
//!
//! 代价是**建这条任务那一下要提权**（`schtasks /Create /RL HIGHEST` 本身就需要管理员），
//! 所以 [`install`] 会用 `runas` 动词拉一个提权的 schtasks，用户点一次授权；之后开机
//! 启动、手动启动都走 `schtasks /Run`，一律不弹窗。这就是「开关打开时问一次，以后
//! 再也不问」的全部含义。
//!
//! 任务名固定 [`TASK_NAME`]；它同时也是「管理员模式开着没有」的事实来源，设置里那个
//! 布尔值只是镜像。**但开关开着 ≠ 眼前这个进程是管理员**——只有用任务拉起来的那次才是，
//! 那件事得用 [`is_elevated`] 问进程自己。

use std::path::Path;

/// 计划任务的名字。改它等于把用户已经装好的那条丢掉，别改。
pub const TASK_NAME: &str = "tools-launcher";

/// 管理员模式开着没有：任务在，就算开着。
pub fn installed() -> bool {
    sys::query()
}

/// 当前这个进程是不是提权的。
pub fn is_elevated() -> bool {
    sys::is_elevated()
}

/// 建/更新任务：登录时触发，最高权限，跑的就是 `exe`。
///
/// 会弹**一次** UAC（因为创建任务本身要提权）。`exe` 挪了地方就再调一次。
pub fn install(exe: &Path) -> bool {
    sys::install(exe)
}

/// 删掉任务。删不掉时再试一次提权的。
pub fn remove() -> bool {
    sys::remove()
}

/// 让现有的任务立刻跑一次。**不弹 UAC**，这就是「以管理员身份重启」的实现。
pub fn launch() -> bool {
    sys::launch()
}

/// 往 `%APPDATA%\tools\admin.log` 追加一行。
///
/// 程序正常启动时没有控制台，`eprintln!` 写进的是虚空——提权这条路跨进程、跨权限，
/// 出问题时没有痕迹根本没法查。这里只记关键事实：谁启动了谁、退出码几号、提权没有。
pub fn log(message: &str) {
    use std::io::Write as _;

    let Some(base) = std::env::var_os("APPDATA") else {
        return;
    };
    let dir = Path::new(&base).join("tools");
    let _ = std::fs::create_dir_all(&dir);
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("admin.log"))
    else {
        return;
    };
    // 时间戳用纪元秒：够对齐先后，不值得为一行日志引个日期库。
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default();
    let _ = writeln!(file, "{seconds} {message}");
}

#[cfg(windows)]
mod sys {
    use std::path::Path;
    use std::process::{Command, Stdio};
    use std::ptr;
    use std::time::Duration;

    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::Security::{
        GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetExitCodeProcess, INFINITE, OpenProcessToken, WaitForSingleObject,
    };
    use windows_sys::Win32::UI::Shell::{
        SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

    use super::{TASK_NAME, log};

    /// `schtasks.exe` 的完整路径。用绝对路径是怕 PATH 里有个假的。
    const SCHTASKS: &str = r"C:\Windows\System32\schtasks.exe";

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// 不弹窗地跑一条 schtasks，返回它成功没有。
    ///
    /// 输出一律丢掉：`schtasks` 的提示是给人看的，我们只关心退出码。`CREATE_NO_WINDOW`
    /// 是为了别在用户眼前闪一个黑框。
    fn run(args: &[&str]) -> bool {
        use std::os::windows::process::CommandExt as _;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        let ok = Command::new(SCHTASKS)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .is_ok_and(|status| status.success());
        log(&format!("schtasks {args:?} -> {ok}"));
        ok
    }

    /// 提权跑一条 schtasks，等它结束，返回退出码是不是 0。
    ///
    /// `runas` 会弹 UAC；用户点了「否」时 `ShellExecuteExW` 直接失败，这里返回 `false`。
    fn run_elevated(args: &str) -> bool {
        let verb = wide("runas");
        let file = wide(SCHTASKS);
        let parameters = wide(args);

        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS,
            lpVerb: verb.as_ptr(),
            lpFile: file.as_ptr(),
            lpParameters: parameters.as_ptr(),
            nShow: SW_HIDE,
            ..Default::default()
        };
        if unsafe { ShellExecuteExW(&mut info) } == 0 {
            log(&format!("runas 启动失败（大概是点了「否」）：{args}"));
            return false;
        }
        if info.hProcess.is_null() {
            log("runas 起来了但没有进程句柄，没法等它");
            return false;
        }
        let mut code = 1u32;
        unsafe {
            WaitForSingleObject(info.hProcess, INFINITE);
            GetExitCodeProcess(info.hProcess, &mut code);
            CloseHandle(info.hProcess);
        }
        log(&format!("提权 schtasks {args} -> 退出码 {code}"));
        code == 0
    }

    pub fn is_elevated() -> bool {
        let mut token = ptr::null_mut();
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut size = 0u32;
        let ok = unsafe {
            GetTokenInformation(
                token,
                TokenElevation,
                &mut elevation as *mut TOKEN_ELEVATION as *mut core::ffi::c_void,
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut size,
            )
        };
        unsafe { CloseHandle(token) };
        ok != 0 && elevation.TokenIsElevated != 0
    }

    pub fn query() -> bool {
        run(&["/Query", "/TN", TASK_NAME])
    }

    /// 建任务。
    ///
    /// **成功与否看任务在不在，不看 schtasks 的退出码**：`/Create` 有时会因为无关紧要的
    /// 东西给出非 0，拿退出码当结论就会「明明建好了却当没建成」——第一版就栽在这。
    pub fn install(exe: &Path) -> bool {
        // /TR 的值本身要带引号（路径里有空格时 schtasks 会自己加，但我们不加就常出错），
        // 所以命令行里是 \"...\" 这种转义。
        let args = format!(
            "/Create /F /TN {TASK_NAME} /TR \"\\\"{}\\\"\" /SC ONLOGON /RL HIGHEST /DELAY 0000:05",
            exe.display()
        );
        let ok = run_elevated(&args);
        // 任务计划程序落库有一点点延迟，问几次再判死刑。
        for _ in 0..20 {
            if query() {
                log("计划任务已就绪");
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        log(&format!("计划任务没建成（schtasks 说 {ok}）"));
        false
    }

    pub fn remove() -> bool {
        if !run(&["/Delete", "/F", "/TN", TASK_NAME]) {
            // 删别人的/别处建的任务要提权，再试一把。
            run_elevated(&format!("/Delete /F /TN {TASK_NAME}"));
        }
        !query()
    }

    pub fn launch() -> bool {
        run(&["/Run", "/TN", TASK_NAME])
    }
}

#[cfg(not(windows))]
mod sys {
    use std::path::Path;

    pub fn query() -> bool {
        false
    }

    pub fn is_elevated() -> bool {
        false
    }

    pub fn install(_exe: &Path) -> bool {
        false
    }

    pub fn remove() -> bool {
        false
    }

    pub fn launch() -> bool {
        false
    }
}
