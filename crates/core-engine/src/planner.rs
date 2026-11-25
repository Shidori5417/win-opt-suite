//! `DryRunPlanner`: builds the `old -> new` diff without writing anything.

use shared_types::{
    ActionSpec, AppResult, ConflictWarning, DiffEntry, DryRunDiff, Severity, SystemFacts, TweakId,
    TweakManifest,
};

use crate::conflict::ConflictGuard;
use crate::registry::ProviderRegistry;

/// Validates every manifest and returns the conflict analysis of the
/// selection (shared by the planner and the transaction manager).
pub(crate) fn validate_selection(
    manifests: &[TweakManifest],
    facts: &SystemFacts,
) -> AppResult<Vec<ConflictWarning>> {
    for m in manifests {
        m.validate()?;
    }
    Ok(ConflictGuard::check(manifests, facts))
}

pub(crate) fn has_blocking(warnings: &[ConflictWarning]) -> bool {
    warnings.iter().any(|w| w.severity == Severity::Blocking)
}

/// Flattens the selection into the ordered action list that is applied.
pub(crate) fn flatten_actions(manifests: &[TweakManifest]) -> Vec<(TweakId, ActionSpec)> {
    manifests
        .iter()
        .flat_map(|m| m.actions.iter().map(|a| (m.id.clone(), a.clone())))
        .collect()
}

/// Read-only planner. Only ever calls [`shared_types::SystemProvider::read`].
#[derive(Debug, Clone, Copy)]
pub struct DryRunPlanner<'a> {
    registry: &'a ProviderRegistry,
}

impl<'a> DryRunPlanner<'a> {
    /// Creates a planner reading through `registry`.
    pub fn new(registry: &'a ProviderRegistry) -> Self {
        Self { registry }
    }

    /// Plans the application of `manifests` on a machine described by `facts`.
    ///
    /// Blocking conflicts are *reported* in [`DryRunDiff::warnings`], not
    /// returned as an error, so the UI can show them next to the diff.
    ///
    /// # Errors
    /// `InvalidManifest` for a manifest failing `validate()`,
    /// `ProviderNotRegistered`, or any error of `provider.read()`.
    pub fn plan(&self, manifests: &[TweakManifest], facts: &SystemFacts) -> AppResult<DryRunDiff> {
        let warnings = validate_selection(manifests, facts)?;
        let mut entries = Vec::new();
        for (tweak_id, action) in flatten_actions(manifests) {
            let provider = self.registry.get(action.provider_kind())?;
            let snapshot = provider.read(&action)?;
            entries.push(DiffEntry::new(
                tweak_id,
                action.provider_kind(),
                action.resource_id(),
                snapshot.state,
                action.desired_state(),
            ));
        }
        Ok(DryRunDiff {
            entries,
            warnings,
            requires_reboot: manifests.iter().any(|m| m.requires_reboot),
        })
    }
}

#[cfg(test)]
mod tests {
    use shared_types::{AppError, ChangeKind, ProviderKind};

    use super::*;
    use crate::testing::{FakeProvider, dword, facts, manifest, reg};

    fn registry(p: &FakeProvider) -> ProviderRegistry {
        let mut r = ProviderRegistry::new();
        r.register(Box::new(p.clone())).unwrap();
        r
    }

    #[test]
    fn builds_diff_without_writing() {
        let p = FakeProvider::default();
        p.set(&reg("A", 0), dword(0));
        let reg_ = registry(&p);
        let mut m = manifest("tweak.a", vec![reg("A", 1), reg("B", 2), reg("C", 3)]);
        m.requires_reboot = true;
        p.set(&reg("C", 0), dword(3));
        let diff = DryRunPlanner::new(&reg_).plan(&[m], &facts()).unwrap();
        let changes: Vec<_> = diff.entries.iter().map(|e| e.change).collect();
        assert_eq!(
            changes,
            [ChangeKind::Modify, ChangeKind::Create, ChangeKind::NoChange]
        );
        assert_eq!(diff.entries[0].before, Some(dword(0)));
        assert_eq!(diff.entries[0].after, Some(dword(1)));
        assert!(diff.requires_reboot);
        assert!(diff.warnings.is_empty());
        // nothing was written
        assert_eq!(p.get(&reg("A", 0)), Some(dword(0)));
        assert_eq!(p.get(&reg("B", 0)), None);
        assert!(p.log().is_empty());
    }

    #[test]
    fn reports_blocking_conflicts_in_diff() {
        let p = FakeProvider::default();
        let reg_ = registry(&p);
        let a = manifest("tweak.a", vec![reg("A", 1)]);
        let b = manifest("tweak.b", vec![reg("A", 2)]);
        let diff = DryRunPlanner::new(&reg_).plan(&[a, b], &facts()).unwrap();
        assert!(diff.has_blocking_conflicts());
        assert_eq!(diff.entries.len(), 2);
    }

    #[test]
    fn rejects_invalid_manifest_and_missing_provider() {
        let reg_ = ProviderRegistry::new();
        let planner = DryRunPlanner::new(&reg_);
        let bad = manifest("tweak.a", vec![]);
        assert!(matches!(
            planner.plan(&[bad], &facts()),
            Err(AppError::InvalidManifest { .. })
        ));
        let ok = manifest("tweak.a", vec![reg("A", 1)]);
        assert_eq!(
            planner.plan(&[ok], &facts()).unwrap_err(),
            AppError::ProviderNotRegistered {
                provider: ProviderKind::Registry
            }
        );
    }
}
