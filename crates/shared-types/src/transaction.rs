//! Transaction and step life-cycle states (persisted as TEXT in SQLite; the
//! string values are part of the storage contract — see migrations/0001).

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::ids::{TransactionId, TweakId};
use crate::manifest::Profile;

/// Transaction life-cycle:
///
/// ```text
/// Pending -> Applying -> Committed -> (RollingBack -> RolledBack)   user rollback
///                     \-> RollingBack -> RolledBack                  step failure
///                                    \-> Failed                      rollback step failed
/// ```
///
/// `Pending`, `Applying` and `RollingBack` found at startup mean the process
/// died mid-transaction: startup crash recovery must run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransactionStatus {
    Pending,
    Applying,
    Committed,
    RollingBack,
    RolledBack,
    /// Rollback could not restore every step; manual attention required.
    Failed,
}

impl TransactionStatus {
    pub const ALL: [TransactionStatus; 6] = [
        TransactionStatus::Pending,
        TransactionStatus::Applying,
        TransactionStatus::Committed,
        TransactionStatus::RollingBack,
        TransactionStatus::RolledBack,
        TransactionStatus::Failed,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            TransactionStatus::Pending => "PENDING",
            TransactionStatus::Applying => "APPLYING",
            TransactionStatus::Committed => "COMMITTED",
            TransactionStatus::RollingBack => "ROLLING_BACK",
            TransactionStatus::RolledBack => "ROLLED_BACK",
            TransactionStatus::Failed => "FAILED",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == value)
    }

    /// `true` if finding this status at startup requires crash recovery.
    pub const fn is_incomplete(self) -> bool {
        matches!(
            self,
            TransactionStatus::Pending
                | TransactionStatus::Applying
                | TransactionStatus::RollingBack
        )
    }
}

impl fmt::Display for TransactionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Per-action step state. A step is written `PENDING` (with its snapshot)
/// *before* the provider touches the system and `APPLIED` right after.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    Applied,
    Reverted,
    RevertFailed,
}

impl StepStatus {
    pub const ALL: [StepStatus; 4] = [
        StepStatus::Pending,
        StepStatus::Applied,
        StepStatus::Reverted,
        StepStatus::RevertFailed,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            StepStatus::Pending => "PENDING",
            StepStatus::Applied => "APPLIED",
            StepStatus::Reverted => "REVERTED",
            StepStatus::RevertFailed => "REVERT_FAILED",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == value)
    }
}

impl fmt::Display for StepStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// History row for the rollback screen / CLI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransactionSummary {
    pub id: TransactionId,
    pub created_at_unix_ms: i64,
    pub status: TransactionStatus,
    pub profile: Option<Profile>,
    pub tweak_ids: Vec<TweakId>,
    pub step_count: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_strings_roundtrip() {
        for s in TransactionStatus::ALL {
            assert_eq!(TransactionStatus::parse(s.as_str()), Some(s));
        }
        for s in StepStatus::ALL {
            assert_eq!(StepStatus::parse(s.as_str()), Some(s));
        }
        assert!(TransactionStatus::Applying.is_incomplete());
        assert!(!TransactionStatus::Committed.is_incomplete());
    }
}
