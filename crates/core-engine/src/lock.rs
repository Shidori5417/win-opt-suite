//! Portable file-based implementation of the `shared_types::TransactionLock`
//! port (ADR-0004). The Win32 named mutex lives in `providers`.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

use shared_types::{AppError, AppResult, LockGuard, TransactionLock};

/// Cross-platform lock based on an exclusive advisory lock on a lock file
/// (`std::fs::File::try_lock`).
#[derive(Debug, Clone)]
pub struct FileLock {
    path: PathBuf,
}

impl FileLock {
    /// Creates a lock backed by the file at `path` (created on first acquire).
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Path of the lock file.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

struct FileLockGuard {
    file: File,
}

impl Drop for FileLockGuard {
    fn drop(&mut self) {
        // Closing the handle releases the lock anyway; ignore unlock errors.
        let _ = self.file.unlock();
    }
}

impl TransactionLock for FileLock {
    fn acquire(&self) -> AppResult<Box<dyn LockGuard>> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&self.path)?;
        match file.try_lock() {
            Ok(()) => Ok(Box::new(FileLockGuard { file })),
            Err(TryLockError::WouldBlock) => Err(AppError::LockUnavailable),
            Err(TryLockError::Error(e)) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_acquire_fails_until_guard_dropped() {
        let dir = std::env::temp_dir().join(format!("core-engine-lock-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let lock = FileLock::new(dir.join("tx.lock"));
        let guard = lock.acquire().unwrap();
        assert_eq!(lock.acquire().err(), Some(AppError::LockUnavailable));
        // A different instance on the same path contends too.
        assert!(matches!(
            FileLock::new(lock.path()).acquire(),
            Err(AppError::LockUnavailable)
        ));
        drop(guard);
        assert!(lock.acquire().is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
