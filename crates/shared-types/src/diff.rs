//! Dry-run plans shown to the user before anything is written.

use serde::{Deserialize, Serialize};

use crate::action::{ProviderKind, ResourceState};
use crate::conflict::{ConflictWarning, Severity};
use crate::ids::{ResourceId, TweakId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Create,
    Modify,
    Delete,
    NoChange,
}

impl ChangeKind {
    pub fn classify(before: Option<&ResourceState>, after: Option<&ResourceState>) -> Self {
        match (before, after) {
            (None, None) => ChangeKind::NoChange,
            (None, Some(_)) => ChangeKind::Create,
            (Some(_), None) => ChangeKind::Delete,
            (Some(b), Some(a)) if a == b => ChangeKind::NoChange,
            (Some(_), Some(_)) => ChangeKind::Modify,
        }
    }
}

/// One `old -> new` row of the dry-run diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct DiffEntry {
    pub tweak_id: TweakId,
    pub provider: ProviderKind,
    pub resource: ResourceId,
    pub before: Option<ResourceState>,
    pub after: Option<ResourceState>,
    pub change: ChangeKind,
}

impl DiffEntry {
    pub fn new(
        tweak_id: TweakId,
        provider: ProviderKind,
        resource: ResourceId,
        before: Option<ResourceState>,
        after: Option<ResourceState>,
    ) -> Self {
        let change = ChangeKind::classify(before.as_ref(), after.as_ref());
        Self {
            tweak_id,
            provider,
            resource,
            before,
            after,
            change,
        }
    }
}

/// Complete plan for applying a set of tweaks. Producing it never modifies the system.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct DryRunDiff {
    pub entries: Vec<DiffEntry>,
    pub warnings: Vec<ConflictWarning>,
    pub requires_reboot: bool,
}

impl DryRunDiff {
    pub fn has_blocking_conflicts(&self) -> bool {
        self.warnings
            .iter()
            .any(|w| w.severity == Severity::Blocking)
    }

    /// Entries that would actually change the system.
    pub fn effective_changes(&self) -> impl Iterator<Item = &DiffEntry> {
        self.entries
            .iter()
            .filter(|e| e.change != ChangeKind::NoChange)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify() {
        let a = ResourceState::ScheduledTask { enabled: true };
        let b = ResourceState::ScheduledTask { enabled: false };
        assert_eq!(ChangeKind::classify(None, Some(&a)), ChangeKind::Create);
        assert_eq!(ChangeKind::classify(Some(&a), None), ChangeKind::Delete);
        assert_eq!(
            ChangeKind::classify(Some(&a), Some(&a)),
            ChangeKind::NoChange
        );
        assert_eq!(ChangeKind::classify(Some(&a), Some(&b)), ChangeKind::Modify);
        assert_eq!(ChangeKind::classify(None, None), ChangeKind::NoChange);
    }
}
