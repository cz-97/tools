//! 面板挂机时把 GPUI 的渲染循环停下来。
//!
//! GPUI 的 Windows 平台无条件开一条 vsync 线程（`gpui-pre-windows` 的
//! `begin_vsync_thread`）：每 16.6ms 醒一次，对所有窗口 `RedrawWindow`，**跟窗口是不是
//! 显示着无关**。面板收起来挂机时，这条线程就是这个程序唯一的空闲开销——任务管理器里
//! 那个 0~0.1% 的抖动就是它。
//!
//! GPUI 没给开关，所以这里按线程名找到那条线程，**挂起**它；唤出面板之前再**恢复**。
//! 挂起期间主线程不渲染（窗口是隐藏的），所以不怕它占着设备的锁；恢复一定发生在显示
//! 之前，不会出现黑面板。
//!
//! 找不到线程（GPUI 改了名字 / 换了平台）就什么都不做，只是省不了这点 CPU。

/// 挂起 GPUI 的渲染线程，返回是否成功（或者本来就已经挂起）。
pub fn suspend() -> bool {
    imp::suspend()
}

/// 恢复 GPUI 的渲染线程。没挂起时调用是空操作。
pub fn resume() -> bool {
    imp::resume()
}

#[cfg(windows)]
mod imp {
    use std::mem::size_of;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LocalFree};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows_sys::Win32::System::Threading::{
        GetThreadDescription, OpenThread, ResumeThread, SuspendThread,
        THREAD_QUERY_LIMITED_INFORMATION, THREAD_SUSPEND_RESUME,
    };

    /// GPUI 给那条线程起的名字。
    const RENDER_THREAD_NAME: &str = "VSyncProvider";

    /// 渲染线程的句柄；0 表示还没找到。
    static RENDER_THREAD: AtomicUsize = AtomicUsize::new(0);
    /// 是不是我们挂起的。不是就不去碰它，免得把挂起计数弄乱。
    static SUSPENDED: AtomicBool = AtomicBool::new(false);

    pub fn suspend() -> bool {
        if SUSPENDED.swap(true, Ordering::SeqCst) {
            return true;
        }
        let Some(thread) = render_thread() else {
            SUSPENDED.store(false, Ordering::SeqCst);
            return false;
        };
        // 返回的是之前的挂起计数，`u32::MAX` 才是失败。
        if unsafe { SuspendThread(thread) } == u32::MAX {
            SUSPENDED.store(false, Ordering::SeqCst);
            return false;
        }
        true
    }

    pub fn resume() -> bool {
        if !SUSPENDED.swap(false, Ordering::SeqCst) {
            return true;
        }
        let Some(thread) = render_thread() else {
            return false;
        };
        let previous = unsafe { ResumeThread(thread) };
        previous != u32::MAX
    }

    fn render_thread() -> Option<HANDLE> {
        let cached = RENDER_THREAD.load(Ordering::Relaxed);
        if cached != 0 {
            return Some(cached as HANDLE);
        }
        let found = unsafe { find_render_thread() }?;
        RENDER_THREAD.store(found as usize, Ordering::Relaxed);
        Some(found)
    }

    unsafe fn find_render_thread() -> Option<HANDLE> {
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            if snapshot.is_null() {
                return None;
            }
            let mut entry = THREADENTRY32 {
                dwSize: size_of::<THREADENTRY32>() as u32,
                ..Default::default()
            };
            let mut found = None;
            if Thread32First(snapshot, &mut entry) != 0 {
                loop {
                    if entry.th32OwnerProcessID == std::process::id() {
                        let handle = OpenThread(
                            THREAD_SUSPEND_RESUME | THREAD_QUERY_LIMITED_INFORMATION,
                            0,
                            entry.th32ThreadID,
                        );
                        if !handle.is_null() {
                            if thread_name(handle) == RENDER_THREAD_NAME {
                                found = Some(handle);
                                break;
                            }
                            CloseHandle(handle);
                        }
                    }
                    if Thread32Next(snapshot, &mut entry) == 0 {
                        break;
                    }
                }
            }
            CloseHandle(snapshot);
            found
        }
    }

    /// 线程的描述名（`SetThreadDescription` 写进去的那个）。
    unsafe fn thread_name(thread: HANDLE) -> String {
        unsafe {
            let mut raw: *mut u16 = std::ptr::null_mut();
            if GetThreadDescription(thread, &mut raw) < 0 || raw.is_null() {
                return String::new();
            }
            let length = (0..).take_while(|ix| *raw.add(*ix) != 0).count();
            let name = String::from_utf16_lossy(std::slice::from_raw_parts(raw, length));
            LocalFree(raw as _);
            name
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn suspend() -> bool {
        false
    }

    pub fn resume() -> bool {
        false
    }
}
