//! 全局单例：一个登录会话里只允许跑一个实例。
//!
//! 第二个实例绝不跟先来的抢东西（热键、托盘图标、SQLite 库），它只做两件事：
//! 把「亮一下面板」的信号丢给已经跑着的那个，然后自己退出。所以重复双击 exe
//! 的效果等于「把面板叫回来」，而不是长出第二个后台进程。
//!
//! 同一套信号里还有一条「安静退出」（`tools.exe --quit`）：面板自己以管理员身份跑着时，
//! 普通权限的进程**杀不掉**它（要 UAC 授权），但它可以让它自己走——收到信号就
//! `cx.quit()`。部署脚本靠这条收旧进程，不必弹权限框。
//!
//! 重启靠**旧的先交身份**（[`Instance`] 一丢，内核对象就没了），新进程照常抢就行——
//! 不用让新的等在单例上，那种跨进程的等待既慢又难判断卡在哪。
//!
//! 名字带 `Local\` 前缀：只在当前登录会话里唯一，换个用户 / 会话各跑各的。

/// 「把面板亮出来」这个请求的名字。
const WAKE_NAME: &str = r"Local\tools.wake";
/// 「安静退出」这个请求的名字。
const QUIT_NAME: &str = r"Local\tools.quit";
/// 单例身份的名字。
const MUTEX_NAME: &str = r"Local\tools.singleton";

/// 拿到手的单例身份。活着就代表「我是唯一的那个」；丢弃（或进程退出）就放开。
pub struct Instance {
    held: sys::Held,
}

impl Instance {
    /// 抢单例身份。已经有人在跑就返回 `None`——调用方该做的只有一件事：
    /// [`Instance::wake_existing`]，然后退出。
    pub fn acquire() -> Option<Self> {
        Self::acquire_named(MUTEX_NAME, WAKE_NAME, QUIT_NAME)
    }

    /// 请求已经在跑的那个实例把面板亮出来，成功返回 `true`。
    ///
    /// 拿不到事件（没有实例在跑，或者是老版本）就返回 `false`，调用方照常退出。
    pub fn wake_existing() -> bool {
        sys::signal(WAKE_NAME)
    }

    /// 请求已经在跑的那个实例**自己退出**，成功返回 `true`。
    ///
    /// 这是唯一不用 UAC 就能收掉提权进程的路子：普通权限的进程杀不动 High 完整性的
    /// 进程，但能让它自己走。
    pub fn quit_existing() -> bool {
        sys::signal(QUIT_NAME)
    }

    /// 非阻塞地问一句：有别人在叫我们把面板亮出来吗。
    ///
    /// 事件是自动复位的，读到一次就清掉，不会连着重开两遍。
    pub fn take_wakeup(&self) -> bool {
        sys::take(&self.held)
    }

    /// 唤醒事件的句柄值，交给一条后台线程死等。
    ///
    /// 句柄在 [`Instance`] 被丢弃后就失效了，但那时进程正准备退出（重启会让旧进程
    /// 先交身份），等它的线程会拿到 `WAIT_FAILED` 自己退出，不会卡住。
    pub fn wake_handle(&self) -> usize {
        sys::wake_handle(&self.held)
    }

    /// 退出事件的句柄值，和唤醒事件一起交给那条线程死等。
    pub fn quit_handle(&self) -> usize {
        sys::quit_handle(&self.held)
    }

    /// 换一套名字抢身份。生产代码用 [`Instance::acquire`]，这里给测试一个独立命名空间：
    /// 机器上真跑着一个实例时，测试也不能被它挡掉。
    ///
    /// **三个名字必须和 [`Instance::wake_existing`] / [`Instance::quit_existing`] 打到的那两个
    /// 对上**，否则信号发出去没人接（踩过：quit 从 wake 派生，生产名字却写死，结果
    /// `--quit` 一直报「没有实例在跑」）。
    fn acquire_named(mutex: &str, wake: &str, quit: &str) -> Option<Self> {
        sys::acquire((mutex, wake, quit)).map(|held| Self { held })
    }
}

/// 死等**任意一个**信号，返回它是第几个（按传进来的顺序）。
///
/// 参数是 [`Instance::wake_handle`] / [`Instance::quit_handle`] 交出来的句柄值；之所以走
/// 模块级自由函数，是因为等它的线程不该攥着整个 [`Instance`]（那是主线程持有的身份）。
///
/// 面板挂机时别的进程可能永远不叫我们，所以这里必须阻塞等待、不能轮询：一个 200ms
/// 的轮询循环看着不贵，却是之前这个程序挂机时唯一持续醒着的东西。
///
/// 句柄失效时返回 `None`，那条线程就该收工。
pub fn wait_signals(events: &[usize]) -> Option<usize> {
    sys::wait_any(events)
}

/// 等一个新的同名进程出现，最多等 `timeout`；等到了返回 `true`。
///
/// 重启时用：交给计划任务拉起新实例之后，**必须确认它真的起来了**再退出自己。
/// `schtasks /Run` 说「成功」只代表任务被接受，任务可能只是排进队列（电池策略、
/// 上一个实例还没退干净，都会让它一直排着）——拿那个返回值当证据就会「退出后再没起来」。
pub fn wait_for_new(timeout: std::time::Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if sys::another_running() {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(60));
    }
}

#[cfg(windows)]
mod sys {
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, SetLastError, WAIT_ABANDONED,
        WAIT_FAILED, WAIT_OBJECT_0,
    };
    use windows_sys::Win32::System::Threading::{
        CreateEventW, CreateMutexW, OpenEventW, SetEvent, WaitForMultipleObjects,
        WaitForSingleObject,
    };

    /// `WaitForMultipleObjects` 的「一直等」。
    const INFINITE: u32 = 0xffff_ffff;
    /// 「只等所有句柄」而不是「等到任意一个」，这里永远是「任意一个」。
    const WAIT_ANY: i32 = 0;

    /// 打开一个事件只需要「能 SetEvent」这一项权限。
    const EVENT_MODIFY_STATE: u32 = 0x0002;

    pub struct Held {
        /// 单例身份。只靠它「存在」来判重，不真的去占有它。
        mutex: HANDLE,
        /// 别的实例用它叫我们亮面板。
        wake: HANDLE,
        /// 别的实例用它叫我们自己退出。
        quit: HANDLE,
    }

    impl Drop for Held {
        fn drop(&mut self) {
            // 关掉句柄就等于放弃身份：没有别的句柄时内核对象跟着销毁。
            unsafe {
                CloseHandle(self.quit);
                CloseHandle(self.wake);
                CloseHandle(self.mutex);
            }
        }
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn acquire(names: (&str, &str, &str)) -> Option<Held> {
        let (mutex, wake, quit) = (wide(names.0), wide(names.1), wide(names.2));
        // `CreateMutexW` 成功时**不会**清掉上一次的错误码，先归零再判，
        // 免得把上一轮的 `ERROR_ALREADY_EXISTS` 当成这一轮的结果。
        unsafe { SetLastError(0) };
        let held = unsafe { CreateMutexW(std::ptr::null(), 0, mutex.as_ptr()) };
        if held.is_null() {
            return None;
        }
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe { CloseHandle(held) };
            return None;
        }
        let wake = unsafe { CreateEventW(std::ptr::null(), 0, 0, wake.as_ptr()) };
        if wake.is_null() {
            unsafe { CloseHandle(held) };
            return None;
        }
        let quit = unsafe { CreateEventW(std::ptr::null(), 0, 0, quit.as_ptr()) };
        if quit.is_null() {
            unsafe {
                CloseHandle(wake);
                CloseHandle(held);
            }
            return None;
        }
        Some(Held {
            mutex: held,
            wake,
            quit,
        })
    }

    /// 给同名事件打个信号（唤醒、退出共用）。
    pub fn signal(name: &str) -> bool {
        let name = wide(name);
        let event = unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, name.as_ptr()) };
        if event.is_null() {
            return false;
        }
        let signalled = unsafe { SetEvent(event) } != 0;
        unsafe { CloseHandle(event) };
        signalled
    }

    pub fn take(held: &Held) -> bool {
        // 0 毫秒 = 只看一眼，别把 UI 线程挂住。
        (unsafe { WaitForSingleObject(held.wake, 0) }) == WAIT_OBJECT_0
    }

    pub fn wake_handle(held: &Held) -> usize {
        held.wake as usize
    }

    pub fn quit_handle(held: &Held) -> usize {
        held.quit as usize
    }

    pub fn wait_any(events: &[usize]) -> Option<usize> {
        if events.is_empty() {
            return None;
        }
        let handles: Vec<HANDLE> = events.iter().map(|value| *value as HANDLE).collect();
        loop {
            let result = unsafe {
                WaitForMultipleObjects(handles.len() as u32, handles.as_ptr(), WAIT_ANY, INFINITE)
            };
            // 句柄已经随着旧进程的身份一起没了，线程该收工。
            if result == WAIT_FAILED || result == WAIT_ABANDONED {
                return None;
            }
            let index = result.wrapping_sub(WAIT_OBJECT_0) as usize;
            if index < handles.len() {
                return Some(index);
            }
            // `INFINITE` 不该给出别的结果；真遇上就接着等，别把线程放跑。
        }
    }

    /// 除了自己，还有没有同名进程在跑。
    ///
    /// 用 ToolHelp 快照：只看进程名，不需要任何权限，也不需要开放别的进程句柄。
    pub fn another_running() -> bool {
        use windows_sys::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        };

        let Ok(current) = std::env::current_exe() else {
            return false;
        };
        let Some(name) = current
            .file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
        else {
            return false;
        };
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot.is_null() {
            return false;
        }
        let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut more = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
        let mut found = false;
        while more {
            let exe = String::from_utf16_lossy(&entry.szExeFile)
                .trim_end_matches('\0')
                .to_lowercase();
            if exe == name && entry.th32ProcessID != std::process::id() {
                found = true;
                break;
            }
            more = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
        }
        unsafe { CloseHandle(snapshot) };
        found
    }
}

#[cfg(not(windows))]
mod sys {
    pub struct Held;

    pub fn acquire(_: (&str, &str, &str)) -> Option<Held> {
        Some(Held)
    }

    pub fn signal(_: &str) -> bool {
        false
    }

    pub fn take(_: &Held) -> bool {
        false
    }

    pub fn wake_handle(_: &Held) -> usize {
        0
    }

    pub fn quit_handle(_: &Held) -> usize {
        0
    }

    pub fn wait_any(_: &[usize]) -> Option<usize> {
        None
    }

    pub fn another_running() -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn unique(prefix: &str) -> String {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        format!(
            r"Local\{prefix}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        )
    }

    /// 测试用的三个名字：各自独立，都不许和生产名字撞上。
    fn names(prefix: &str) -> (String, String, String) {
        (
            unique(&format!("{prefix}-mutex")),
            unique(&format!("{prefix}-wake")),
            unique(&format!("{prefix}-quit")),
        )
    }

    #[test]
    fn a_second_instance_is_turned_away() {
        let (mutex, wake, quit) = names("test");
        let first =
            Instance::acquire_named(&mutex, &wake, &quit).expect("第一个该拿到");
        assert!(
            Instance::acquire_named(&mutex, &wake, &quit).is_none(),
            "第二个必须被拒"
        );

        // 唤醒信号只送一次：自动复位的事件读一遍就清掉。
        assert!(sys::signal(&wake));
        assert!(first.take_wakeup());
        assert!(!first.take_wakeup());

        // 交还身份之后别人就能接手（重启靠的就是这条）。
        drop(first);
        assert!(Instance::acquire_named(&mutex, &wake, &quit).is_some());
    }

    #[test]
    fn both_signals_are_delivered_on_the_same_wait() {
        // `--quit` 和「再双击一次」走的是同一条等待线程，两个事件得能分别认出来。
        // **这里只能打测试命名空间的事件**：打到生产名字上就等于把正在跑的启动器关掉。
        let (mutex, wake, quit) = names("test-signal");
        let held = Instance::acquire_named(&mutex, &wake, &quit).expect("第一个该拿到");
        let events = [held.wake_handle(), held.quit_handle()];

        assert!(sys::signal(&quit));
        assert_eq!(wait_signals(&events), Some(1), "退出信号落在第二个句柄上");

        assert!(sys::signal(&wake));
        assert_eq!(wait_signals(&events), Some(0), "唤醒信号落在第一个句柄上");
    }

    #[test]
    fn a_released_instance_can_be_reclaimed_at_once() {
        // 重启的路径：旧进程先丢身份，新进程马上就能拿到，不用等。
        let (mutex, wake, quit) = names("test-restart");
        let first = Instance::acquire_named(&mutex, &wake, &quit).expect("第一个该拿到");
        drop(first);
        let second =
            Instance::acquire_named(&mutex, &wake, &quit).expect("旧的一走新的就该拿到");
        assert!(Instance::acquire_named(&mutex, &wake, &quit).is_none());
        drop(second);
    }
}
