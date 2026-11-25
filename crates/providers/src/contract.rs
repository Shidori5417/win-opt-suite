//! Platform-independent helpers implementing the shared parts of the
//! `SystemProvider` contract on top of a provider's own `read`.

use shared_types::{
    ActionSpec, AppResult, ProviderKind, ResourceState, StateSnapshot, VerifyOutcome,
};

use crate::errors::mismatch;

/// Fails with `ActionProviderMismatch` unless `action` belongs to `expected`.
pub fn ensure_kind(expected: ProviderKind, action: &ActionSpec) -> AppResult<()> {
    if action.provider_kind() == expected {
        Ok(())
    } else {
        Err(mismatch(expected, action))
    }
}

/// `verify` = compare the action's desired state with the observed one.
pub fn verify_from_snapshot(action: &ActionSpec, live: &StateSnapshot) -> VerifyOutcome {
    VerifyOutcome::compare(action.desired_state(), live.state.clone())
}

/// Convenience for building a snapshot from an optional observed state.
pub fn snapshot_of(
    action: &ActionSpec,
    kind: ProviderKind,
    state: Option<ResourceState>,
) -> StateSnapshot {
    match state {
        Some(state) => StateSnapshot::present(action.resource_id(), kind, state),
        None => StateSnapshot::absent(action.resource_id(), kind),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared_types::{ServiceAction, ServiceStartType};

    fn svc(start: ServiceStartType) -> ActionSpec {
        ActionSpec::Service(ServiceAction {
            name: "DiagTrack".into(),
            start_type: start,
        })
    }

    #[test]
    fn kind_check() {
        let a = svc(ServiceStartType::Disabled);
        assert!(ensure_kind(ProviderKind::Service, &a).is_ok());
        assert!(ensure_kind(ProviderKind::Registry, &a).is_err());
    }

    #[test]
    fn snapshot_invariant_holds() {
        let a = svc(ServiceStartType::Disabled);
        let absent = snapshot_of(&a, ProviderKind::Service, None);
        assert!(!absent.existed && absent.is_consistent());
        let present = snapshot_of(
            &a,
            ProviderKind::Service,
            Some(ResourceState::Service {
                start_type: ServiceStartType::Manual,
            }),
        );
        assert!(present.existed && present.is_consistent());
    }

    #[test]
    fn verify_compares_desired_with_live() {
        let a = svc(ServiceStartType::Disabled);
        let ok = snapshot_of(
            &a,
            ProviderKind::Service,
            Some(ResourceState::Service {
                start_type: ServiceStartType::Disabled,
            }),
        );
        assert!(verify_from_snapshot(&a, &ok).is_match());
        let missing = snapshot_of(&a, ProviderKind::Service, None);
        assert!(!verify_from_snapshot(&a, &missing).is_match());
    }
}
