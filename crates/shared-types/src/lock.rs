//! Global "one optimisation transaction at a time" lock port.
//!
//! Lives in `shared-types` so that an OS-native implementation (Win32
//! named mutex in `providers`) and the portable file lock (`core-engine`)
//! satisfy the same contract (ADR-0004).

use crate::error::AppResult;

/// Marker for a held lock; the lock is released when the guard is dropped.
pub trait LockGuard: Send {}

impl<T: Send> LockGuard for T {}

/// Mutual exclusion between optimisation transactions, across processes.
pub trait TransactionLock: Send + Sync {
    /// Acquires the lock without blocking.
    ///
    /// # Errors
    /// [`crate::AppError::LockUnavailable`] if another holder has it.
    fn acquire(&self) -> AppResult<Box<dyn LockGuard>>;
}
