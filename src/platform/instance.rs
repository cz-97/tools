//! 全局单例：一个登录会话里只允许跑一个实例。
//!
//! 第二个实例绝不跟先来的抢东西（热键、托盘图标、SQLite 库），它只做两件事：
//! 把「亮一下面板」的信号丢给已经跑着的那个，然后自己退出。所以重复双击 exe
//! 的效果等于「把面板叫回来」，而不是长出第二个后台进程。
//!
//! 重启靠**旧的先交身份**（[`Instance`] 一丢，内核对象就没了），新进程照常抢就行——
//! 不用让新的等在单例上，那种跨进程的等待既慢又难判断卡在哪。
//!
//! 名字带 `Local\` 前缀：只在当前登录会话里唯一，换个用户 / 会话各跑各的。

/// 「把面板亮出来」这个请求的名字。
const WAKE_NAME: &str = r"Local\gpui-tools.wake";
/// 单例身份的名字。
const MUTEX_NAME: &str = r"Local\gpui-tools.singleton";

/// 拿到手的单例身份。活着就代表「我是唯一的那个」；丢弃（或进程退出）就放开。
pub struct Instance {
    held: sys::Held,
}

impl Instance {
    /// 抢单例身份。已经有人在跑就返回 `None`——调用方该做的只有一件事：
    /// [`Instance::wake_existing`]，然后退出。
    pub fn acquire() -> Option<Self> {
        Self::acquire_named(MUTEX_NAME, WAKE_NAME)
    }

    /// 请求已经在跑的那个实例把面板亮出来，成功返回 `true`。
    ///
    /// 拿不到事件（没有实例在跑，或者是老版本）就返回 `false`，调用方照常退出。
    pub fn wake_existing() -> bool {
        sys::wake(WAKE_NAME)
    }

    /// 非阻塞地问一句：有别人在叫我们把面板亮出来吗。
    ///
    /// 事件是自动复位的，读到一次就清掉，不会连着重开两遍。
    pub fn take_wakeup(&self) -> bool {
        sys::take(&self.held)
    }

    /// 换一套名字抢身份。生产代码用 [`Instance::acquire`]，这里给测试一个独立命名空间：
    /// 机器上真跑着一个实例时，测试也不能被它挡掉。
    fn acquire_named(mutex: &str, wake: &str) -> Option<Self> {
        sys::acquire((mutex, wake)).map(|held| Self { held })
    }
}

#[cfg(windows)]
mod sys {
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, SetLastError, WAIT_OBJECT_0,
    };
    use windows_sys::Win32::System::Threading::{
        CreateEventW, CreateMutexW, OpenEventW, SetEvent, WaitForSingleObject,
    };

    /// 打开一个事件只需要「能 SetEvent」这一项权限。
    const EVENT_MODIFY_STATE: u32 = 0x0002;

    pub struct Held {
        /// 单例身份。只靠它「存在」来判重，不真的去占有它。
        mutex: HANDLE,
        /// 别的实例用它叫我们亮面板。
        event: HANDLE,
    }

    impl Drop for Held {
        fn drop(&mut self) {
            // 关掉句柄就等于放弃身份：没有别的句柄时内核对象跟着销毁。
            unsafe {
                CloseHandle(self.event);
                CloseHandle(self.mutex);
            }
        }
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn acquire(names: (&str, &str)) -> Option<Held> {
        let (mutex, event) = (wide(names.0), wide(names.1));
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
        let event = unsafe { CreateEventW(std::ptr::null(), 0, 0, event.as_ptr()) };
        if event.is_null() {
            unsafe { CloseHandle(held) };
            return None;
        }
        Some(Held { mutex: held, event })
    }

    pub fn wake(name: &str) -> bool {
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
        (unsafe { WaitForSingleObject(held.event, 0) }) == WAIT_OBJECT_0
    }
}

#[cfg(not(windows))]
mod sys {
    pub struct Held;

    pub fn acquire(_: (&str, &str)) -> Option<Held> {
        Some(Held)
    }

    pub fn wake(_: &str) -> bool {
        false
    }

    pub fn take(_: &Held) -> bool {
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

    #[test]
    fn a_second_instance_is_turned_away() {
        let (mutex, wake) = (unique("test-mutex"), unique("test-wake"));
        let first = Instance::acquire_named(&mutex, &wake).expect("第一个该拿到");
        assert!(
            Instance::acquire_named(&mutex, &wake).is_none(),
            "第二个必须被拒"
        );

        // 唤醒信号只送一次：自动复位的事件读一遍就清掉。
        assert!(sys::wake(&wake));
        assert!(first.take_wakeup());
        assert!(!first.take_wakeup());

        // 交还身份之后别人就能接手（重启靠的就是这条）。
        drop(first);
        assert!(Instance::acquire_named(&mutex, &wake).is_some());
    }

    #[test]
    fn a_released_instance_can_be_reclaimed_at_once() {
        // 重启的路径：旧进程先丢身份，新进程马上就能拿到，不用等。
        let (mutex, wake) = (unique("test-restart-mutex"), unique("test-restart-wake"));
        let first = Instance::acquire_named(&mutex, &wake).expect("第一个该拿到");
        drop(first);
        let second = Instance::acquire_named(&mutex, &wake).expect("旧的一走新的就该拿到");
        assert!(Instance::acquire_named(&mutex, &wake).is_none());
        drop(second);
    }
}
