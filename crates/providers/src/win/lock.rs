//! `NamedMutexLock`: cross-process "one transaction at a time" lock backed by
//! a Win32 named mutex (ADR-0004).

use shared_types::{AppError, AppResult, LockGuard, TransactionLock};
use windows::Win32::Foundation::{
    CloseHandle, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};

use super::{Wide, map_error};

/// Kernel object name; `Global\` makes it visible across sessions (desktop
/// app and CLI, services, elevated and non-elevated processes of one user).
pub const MUTEX_NAME: &str = "Global\\WinOptSuite.Transaction";

/// Non-blocking transaction lock over a named Win32 mutex.
///
/// Mutex ownership is thread-affine: the guard returned by `acquire` must be
/// dropped on the thread that acquired it (`ReleaseMutex` fails otherwise and
/// the mutex would stay owned until that thread exits). `TransactionManager`
/// acquires and drops the guard inside a single call, which satisfies this.
///
/// Mutex ownership is also recursive per thread: a second `acquire` on the
/// thread that already holds the lock succeeds. Contention is therefore
/// detected between threads and processes, never within one thread.
#[derive(Debug, Default, Clone, Copy)]
pub struct NamedMutexLock;

impl NamedMutexLock {
    pub fn new() -> Self {
        Self
    }
}

/// Owned, held mutex; releases and closes it on drop.
struct MutexGuard {
    // Raw handle value (HANDLE itself is not `Send`).
    handle: isize,
}

impl MutexGuard {
    fn raw(&self) -> HANDLE {
        HANDLE(self.handle as *mut core::ffi::c_void)
    }
}

impl Drop for MutexGuard {
    fn drop(&mut self) {
        // SAFETY: the handle is a valid mutex handle owned by this guard and
        // currently owned (acquired) by the calling thread; failure (wrong
        // thread) is ignored and the mutex is freed on thread exit.
        let _ = unsafe { ReleaseMutex(self.raw()) };
        // SAFETY: the handle came from a successful CreateMutexW, is owned
        // exclusively by this guard and is closed exactly once.
        let _ = unsafe { CloseHandle(self.raw()) };
    }
}

impl TransactionLock for NamedMutexLock {
    fn acquire(&self) -> AppResult<Box<dyn LockGuard>> {
        let name = Wide::new(MUTEX_NAME)?;
        // SAFETY: `name` outlives the call and is NUL-terminated; a null
        // security-attributes pointer (None) selects the default descriptor.
        let handle = unsafe { CreateMutexW(None, false, name.pcwstr()) }
            .map_err(|e| map_error(&e, MUTEX_NAME))?;
        let guard = MutexGuard {
            handle: handle.0 as isize,
        };
        // SAFETY: `handle` is a valid mutex handle just created above.
        let wait = unsafe { WaitForSingleObject(guard.raw(), 0) };
        match wait {
            // Abandoned: the previous owner died; we now own it. Any half
            // finished transaction is handled by crash recovery.
            WAIT_OBJECT_0 | WAIT_ABANDONED => Ok(Box::new(guard)),
            WAIT_TIMEOUT => {
                // Not owned: only close the handle (no ReleaseMutex).
                let handle = guard.raw();
                core::mem::forget(guard);
                // SAFETY: valid handle owned by us, closed exactly once
                // (the guard was forgotten so its Drop will not run).
                let _ = unsafe { CloseHandle(handle) };
                Err(AppError::LockUnavailable)
            }
            _ => {
                let err = windows::core::Error::from_thread();
                let mapped = map_error(&err, MUTEX_NAME);
                let handle = guard.raw();
                core::mem::forget(guard);
                // SAFETY: valid handle owned by us, closed exactly once.
                let _ = unsafe { CloseHandle(handle) };
                Err(mapped)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contention_across_threads_and_release() {
        let lock = NamedMutexLock::new();
        let guard = lock.acquire().expect("first acquire");
        let other = std::thread::spawn(|| NamedMutexLock::new().acquire().err())
            .join()
            .expect("thread");
        assert_eq!(other, Some(AppError::LockUnavailable));
        drop(guard);
        let again = std::thread::spawn(|| NamedMutexLock::new().acquire().map(|_| ()))
            .join()
            .expect("thread");
        assert!(again.is_ok());
        assert!(lock.acquire().is_ok());
    }
}
