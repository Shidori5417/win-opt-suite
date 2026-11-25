//! Injectable time and identifier sources.

use std::time::{SystemTime, UNIX_EPOCH};

use shared_types::TransactionId;

/// Milliseconds since the Unix epoch from the system clock (0 if the clock is
/// before the epoch; saturates on overflow).
pub(crate) fn system_now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// Source of the current time, injectable for deterministic tests.
pub trait Clock: Send + Sync {
    /// Current time in milliseconds since the Unix epoch.
    fn now_ms(&self) -> i64;
}

/// [`Clock`] backed by the operating system clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        system_now_ms()
    }
}

/// Generator of unique, preferably time-sortable transaction identifiers.
pub trait IdGenerator: Send + Sync {
    /// Returns a fresh identifier.
    fn next_id(&self) -> TransactionId;
}

/// Default [`IdGenerator`]: UUIDv7 (time-ordered).
#[derive(Debug, Clone, Copy, Default)]
pub struct UuidV7Generator;

impl IdGenerator for UuidV7Generator {
    fn next_id(&self) -> TransactionId {
        TransactionId::new(uuid::Uuid::now_v7().to_string())
    }
}
