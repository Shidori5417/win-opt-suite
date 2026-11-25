//! Drift reports: live system vs. the last `COMMITTED` state recorded in
//! the state database (produced by `core-engine::DriftDetector`).

use serde::{Deserialize, Serialize};

use crate::action::{ProviderKind, ResourceState};
use crate::error::AppError;
use crate::ids::{ResourceId, TransactionId, TweakId};

/// A resource whose live state no longer matches what a committed
/// transaction applied (e.g. reverted by Windows Update or another tool).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct DriftEntry {
    pub transaction_id: TransactionId,
    pub tweak_id: TweakId,
    pub provider: ProviderKind,
    pub resource: ResourceId,
    pub expected: Option<ResourceState>,
    pub actual: Option<ResourceState>,
}

/// A resource that could not be checked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct DriftCheckError {
    pub resource: ResourceId,
    pub error: AppError,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct DriftReport {
    /// Number of resources checked.
    pub checked: u32,
    pub drifted: Vec<DriftEntry>,
    pub errors: Vec<DriftCheckError>,
}

impl DriftReport {
    pub fn is_clean(&self) -> bool {
        self.drifted.is_empty() && self.errors.is_empty()
    }
}
