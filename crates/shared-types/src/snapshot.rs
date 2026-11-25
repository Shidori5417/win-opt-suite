//! Pre-change snapshots (persisted as `PENDING` before any write) and
//! verification outcomes.

use serde::{Deserialize, Serialize};

use crate::action::{ProviderKind, ResourceState};
use crate::ids::ResourceId;

/// State of a resource captured *before* it is modified. This is the only
/// information rollback may rely on.
///
/// Invariant: `existed == state.is_some()`. Use the constructors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct StateSnapshot {
    pub resource: ResourceId,
    pub provider: ProviderKind,
    /// Whether the resource existed before the change. If `false`, revert must
    /// **delete** the resource instead of resetting it to a default.
    pub existed: bool,
    pub state: Option<ResourceState>,
    /// Security descriptor of the resource (SDDL) when the provider had to
    /// change its ACL (e.g. TrustedInstaller-owned keys); restored on revert.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub security_descriptor_sddl: Option<String>,
}

impl StateSnapshot {
    pub fn present(resource: ResourceId, provider: ProviderKind, state: ResourceState) -> Self {
        Self {
            resource,
            provider,
            existed: true,
            state: Some(state),
            security_descriptor_sddl: None,
        }
    }

    pub fn absent(resource: ResourceId, provider: ProviderKind) -> Self {
        Self {
            resource,
            provider,
            existed: false,
            state: None,
            security_descriptor_sddl: None,
        }
    }

    #[must_use]
    pub fn with_sddl(mut self, sddl: impl Into<String>) -> Self {
        self.security_descriptor_sddl = Some(sddl.into());
        self
    }

    /// `true` when the invariant `existed == state.is_some()` holds.
    pub fn is_consistent(&self) -> bool {
        self.existed == self.state.is_some()
    }
}

/// Result of comparing the live system with an action's desired state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum VerifyOutcome {
    Matches,
    Drifted {
        expected: Option<ResourceState>,
        actual: Option<ResourceState>,
    },
}

impl VerifyOutcome {
    /// Builds the outcome from the desired and the observed state.
    pub fn compare(expected: Option<ResourceState>, actual: Option<ResourceState>) -> Self {
        if expected == actual {
            VerifyOutcome::Matches
        } else {
            VerifyOutcome::Drifted { expected, actual }
        }
    }

    pub fn is_match(&self) -> bool {
        matches!(self, VerifyOutcome::Matches)
    }
}
