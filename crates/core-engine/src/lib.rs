//! # core-engine
//!
//! Platform-agnostic engine. Talks to the OS **only** through
//! `dyn shared_types::SystemProvider`, so it is fully testable on any OS with
//! in-memory providers.
//!
//! * `store`: SQLite state store with embedded forward-only migrations.
//! * `lock`: global one-transaction-at-a-time lock.
//! * `registry`: `ProviderKind` to provider lookup.
//! * `conflict`: `ConflictGuard`.
//! * `planner`: `DryRunPlanner` (read-only diff).
//! * `manager`: `TransactionManager` (saga apply, rollback, crash recovery).
#![forbid(unsafe_code)]

pub mod clock;
pub mod lock;
pub mod registry;
pub mod store;

pub use clock::{Clock, IdGenerator, SystemClock, UuidV7Generator};
pub use lock::{FileLock, LockGuard, TransactionLock};
pub use registry::ProviderRegistry;
pub use store::Store;
