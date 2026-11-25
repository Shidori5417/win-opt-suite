//! `DriftDetector`: compares live system state against the newest `COMMITTED`
//! baseline recorded in the [`Store`].

use std::collections::BTreeMap;

use shared_types::{
    AppError, AppResult, DriftCheckError, DriftEntry, DriftReport, ResourceId, StepStatus,
    TransactionStatus, VerifyOutcome,
};

use crate::registry::ProviderRegistry;
use crate::store::{StepRecord, Store};

/// Compares the live system state with the latest committed baseline in the
/// state store.
#[derive(Debug, Clone, Copy)]
pub struct DriftDetector<'a> {
    store: &'a Store,
    registry: &'a ProviderRegistry,
}

impl<'a> DriftDetector<'a> {
    /// Creates a detector reading committed steps from `store` and verifying
    /// live state through `registry`.
    pub fn new(store: &'a Store, registry: &'a ProviderRegistry) -> Self {
        Self { store, registry }
    }

    /// Checks every resource touched by `COMMITTED` transactions (`APPLIED`
    /// steps only), using the newest committed step per `ResourceId`.
    ///
    /// Per-resource lookup or verification failures are recorded in
    /// [`DriftReport::errors`] without aborting the scan.
    pub fn detect(&self) -> AppResult<DriftReport> {
        let committed = self
            .store
            .transactions_with_status(&[TransactionStatus::Committed])?;

        // `transactions_with_status` yields oldest-first (`created_at_ms ASC, rowid ASC`)
        // and `steps` yields ascending `seq`, so inserting into a `BTreeMap` leaves the
        // newest committed step per `ResourceId` and guarantees deterministic ordering.
        let mut latest_by_resource: BTreeMap<ResourceId, StepRecord> = BTreeMap::new();
        for tx in committed {
            for step in self.store.steps(&tx.id)? {
                if step.status == StepStatus::Applied {
                    latest_by_resource.insert(step.resource.clone(), step);
                }
            }
        }

        let mut report = DriftReport::default();
        for (resource, step) in latest_by_resource {
            report.checked = report
                .checked
                .checked_add(1)
                .ok_or_else(|| AppError::Internal {
                    message: "drift check resource count overflowed u32".to_owned(),
                })?;

            let provider = match self.registry.get(step.provider) {
                Ok(p) => p,
                Err(error) => {
                    report.errors.push(DriftCheckError { resource, error });
                    continue;
                }
            };

            match provider.verify(&step.action) {
                Ok(VerifyOutcome::Matches) => {}
                Ok(VerifyOutcome::Drifted { expected, actual }) => {
                    report.drifted.push(DriftEntry {
                        transaction_id: step.transaction_id,
                        tweak_id: step.tweak_id,
                        provider: step.provider,
                        resource,
                        expected,
                        actual,
                    });
                }
                Err(error) => {
                    report.errors.push(DriftCheckError { resource, error });
                }
            }
        }

        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicI64, AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};

    use shared_types::{
        ActionSpec, ProviderKind, ResourceState, ServiceAction, ServiceStartType, StateSnapshot,
        SystemProvider, TransactionId, TweakId,
    };

    use super::*;
    use crate::clock::{Clock, IdGenerator};
    use crate::lock::FileLock;
    use crate::manager::TransactionManager;
    use crate::store::{NewStep, NewTransaction};
    use crate::testing::{FakeProvider, dword, facts, manifest, reg};

    static DIR_COUNTER: AtomicU32 = AtomicU32::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let n = DIR_COUNTER.fetch_add(1, Ordering::SeqCst);
            let dir =
                std::env::temp_dir().join(format!("core-engine-drift-{}-{n}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    struct TickClock(AtomicI64);

    impl Clock for TickClock {
        fn now_ms(&self) -> i64 {
            self.0.fetch_add(10, Ordering::SeqCst)
        }
    }

    struct SeqIds(AtomicU32);

    impl IdGenerator for SeqIds {
        fn next_id(&self) -> TransactionId {
            TransactionId::new(format!("tx-{:04}", self.0.fetch_add(1, Ordering::SeqCst)))
        }
    }

    /// Small private fake provider supporting failure injection on `verify`.
    #[derive(Clone, Default)]
    struct VerifyFakeProvider {
        inner: Arc<Mutex<VerifyFakeInner>>,
    }

    #[derive(Default)]
    struct VerifyFakeInner {
        state: BTreeMap<ResourceId, ResourceState>,
        fail_verify: BTreeSet<ResourceId>,
    }

    impl VerifyFakeProvider {
        fn fail_verify(&self, action: &ActionSpec) {
            self.inner
                .lock()
                .unwrap()
                .fail_verify
                .insert(action.resource_id());
        }
    }

    impl SystemProvider for VerifyFakeProvider {
        fn kind(&self) -> ProviderKind {
            ProviderKind::Service
        }

        fn read(&self, action: &ActionSpec) -> AppResult<StateSnapshot> {
            let id = action.resource_id();
            let guard = self.inner.lock().unwrap();
            Ok(match guard.state.get(&id).cloned() {
                Some(s) => StateSnapshot::present(id, ProviderKind::Service, s),
                None => StateSnapshot::absent(id, ProviderKind::Service),
            })
        }

        fn apply(&self, action: &ActionSpec) -> AppResult<()> {
            let id = action.resource_id();
            let mut guard = self.inner.lock().unwrap();
            match action.desired_state() {
                Some(s) => {
                    guard.state.insert(id, s);
                }
                None => {
                    guard.state.remove(&id);
                }
            }
            Ok(())
        }

        fn revert(&self, action: &ActionSpec, snapshot: &StateSnapshot) -> AppResult<()> {
            let id = action.resource_id();
            let mut guard = self.inner.lock().unwrap();
            match (&snapshot.existed, &snapshot.state) {
                (true, Some(s)) => {
                    guard.state.insert(id, s.clone());
                }
                _ => {
                    guard.state.remove(&id);
                }
            }
            Ok(())
        }

        fn verify(&self, action: &ActionSpec) -> AppResult<VerifyOutcome> {
            let id = action.resource_id();
            let guard = self.inner.lock().unwrap();
            if guard.fail_verify.contains(&id) {
                return Err(AppError::AccessDenied {
                    resource: id.to_string(),
                });
            }
            let actual = guard.state.get(&id).cloned();
            Ok(VerifyOutcome::compare(action.desired_state(), actual))
        }
    }

    fn svc(name: &str, start_type: ServiceStartType) -> ActionSpec {
        ActionSpec::Service(ServiceAction {
            name: name.to_owned(),
            start_type,
        })
    }

    struct Fixture {
        reg_provider: FakeProvider,
        svc_provider: VerifyFakeProvider,
        mgr: TransactionManager,
        _dir: TempDir,
    }

    fn fixture() -> Fixture {
        let dir = TempDir::new();
        let reg_provider = FakeProvider::default();
        let svc_provider = VerifyFakeProvider::default();
        let mut registry = ProviderRegistry::new();
        registry.register(Box::new(reg_provider.clone())).unwrap();
        registry.register(Box::new(svc_provider.clone())).unwrap();
        let mgr = TransactionManager::new(
            Store::open_in_memory().unwrap(),
            registry,
            Box::new(FileLock::new(dir.0.join("tx.lock"))),
        )
        .with_clock(Box::new(TickClock(AtomicI64::new(1_000))))
        .with_id_generator(Box::new(SeqIds(AtomicU32::new(1))));
        Fixture {
            reg_provider,
            svc_provider,
            mgr,
            _dir: dir,
        }
    }

    #[test]
    fn clean_when_empty_and_when_live_matches_committed() {
        let fx = fixture();
        let empty = DriftDetector::new(fx.mgr.store(), fx.mgr.registry())
            .detect()
            .unwrap();
        assert_eq!(empty.checked, 0);
        assert!(empty.is_clean());

        fx.mgr
            .apply(
                &[manifest("tweak.a", vec![reg("B", 1), reg("A", 2)])],
                None,
                &facts(),
            )
            .unwrap();

        let report = DriftDetector::new(fx.mgr.store(), fx.mgr.registry())
            .detect()
            .unwrap();
        assert_eq!(report.checked, 2);
        assert!(report.is_clean());
    }

    #[test]
    fn detects_drifted_resources_in_deterministic_order() {
        let fx = fixture();
        let tx = fx
            .mgr
            .apply(
                &[manifest(
                    "tweak.a",
                    vec![reg("Z", 1), reg("A", 10), reg("M", 5)],
                )],
                None,
                &facts(),
            )
            .unwrap();

        // Mutate Z and A out-of-band (A deleted, Z changed).
        fx.reg_provider.set(&reg("Z", 0), dword(99));
        fx.reg_provider
            .with(|i| i.state.remove(&reg("A", 0).resource_id()));

        let report = DriftDetector::new(fx.mgr.store(), fx.mgr.registry())
            .detect()
            .unwrap();
        assert_eq!(report.checked, 3);
        assert!(!report.is_clean());
        assert!(report.errors.is_empty());
        assert_eq!(
            report.drifted,
            vec![
                DriftEntry {
                    transaction_id: tx.clone(),
                    tweak_id: TweakId::new("tweak.a"),
                    provider: ProviderKind::Registry,
                    resource: reg("A", 0).resource_id(),
                    expected: Some(dword(10)),
                    actual: None,
                },
                DriftEntry {
                    transaction_id: tx,
                    tweak_id: TweakId::new("tweak.a"),
                    provider: ProviderKind::Registry,
                    resource: reg("Z", 0).resource_id(),
                    expected: Some(dword(1)),
                    actual: Some(dword(99)),
                },
            ]
        );
    }

    #[test]
    fn newest_committed_transaction_wins_per_resource() {
        let fx = fixture();
        fx.mgr
            .apply(&[manifest("tweak.old", vec![reg("X", 1)])], None, &facts())
            .unwrap();
        let newer = fx
            .mgr
            .apply(&[manifest("tweak.new", vec![reg("X", 2)])], None, &facts())
            .unwrap();

        // Live matches newest -> clean (checked once, not twice).
        let clean = DriftDetector::new(fx.mgr.store(), fx.mgr.registry())
            .detect()
            .unwrap();
        assert_eq!(clean.checked, 1);
        assert!(clean.is_clean());

        // Revert live state back to the older transaction's value -> drifts against newer.
        fx.reg_provider.set(&reg("X", 0), dword(1));
        let drifted = DriftDetector::new(fx.mgr.store(), fx.mgr.registry())
            .detect()
            .unwrap();
        assert_eq!(drifted.checked, 1);
        assert_eq!(drifted.drifted.len(), 1);
        assert_eq!(
            drifted.drifted.first(),
            Some(&DriftEntry {
                transaction_id: newer,
                tweak_id: TweakId::new("tweak.new"),
                provider: ProviderKind::Registry,
                resource: reg("X", 0).resource_id(),
                expected: Some(dword(2)),
                actual: Some(dword(1)),
            })
        );
    }

    #[test]
    fn ignores_non_committed_transactions_and_non_applied_steps() {
        let fx = fixture();
        // Committed baseline: A = 1.
        fx.mgr
            .apply(&[manifest("tweak.base", vec![reg("A", 1)])], None, &facts())
            .unwrap();

        // Committed then user-rolled-back transaction: A = 2 (reverts A back to 1).
        let rolled = fx
            .mgr
            .apply(
                &[manifest("tweak.rolled", vec![reg("A", 2), reg("B", 9)])],
                None,
                &facts(),
            )
            .unwrap();
        fx.mgr.rollback(&rolled).unwrap();

        // Failed mid-apply transaction (rolled back automatically).
        fx.reg_provider
            .with(|i| i.fail_apply.insert(reg("D", 0).resource_id()));
        let _ = fx.mgr.apply(
            &[manifest("tweak.failed", vec![reg("C", 3), reg("D", 4)])],
            None,
            &facts(),
        );

        // Incomplete APPLYING transaction left in store.
        let store = fx.mgr.store();
        let pending_tx = TransactionId::new("tx-crashed");
        store
            .insert_transaction(&NewTransaction {
                id: pending_tx.clone(),
                created_at_ms: 9_999,
                status: TransactionStatus::Applying,
                profile: None,
                tweak_ids: vec![TweakId::new("tweak.crash")],
                windows_build: 26100,
                windows_ubr: 1,
                app_version: "test".into(),
            })
            .unwrap();
        let pending_action = reg("E", 5);
        store
            .insert_step(&NewStep {
                transaction_id: pending_tx,
                seq: 0,
                tweak_id: TweakId::new("tweak.crash"),
                action: pending_action.clone(),
                snapshot: StateSnapshot::absent(
                    pending_action.resource_id(),
                    ProviderKind::Registry,
                ),
                updated_at_ms: 9_999,
            })
            .unwrap();

        // Committed transaction with an explicit non-APPLIED step should also skip that step.
        let skipped_tx = TransactionId::new("tx-mixed");
        store
            .insert_transaction(&NewTransaction {
                id: skipped_tx.clone(),
                created_at_ms: 10_000,
                status: TransactionStatus::Committed,
                profile: None,
                tweak_ids: vec![TweakId::new("tweak.mixed")],
                windows_build: 26100,
                windows_ubr: 1,
                app_version: "test".into(),
            })
            .unwrap();
        let reverted_action = reg("F", 7);
        store
            .insert_step(&NewStep {
                transaction_id: skipped_tx.clone(),
                seq: 0,
                tweak_id: TweakId::new("tweak.mixed"),
                action: reverted_action.clone(),
                snapshot: StateSnapshot::absent(
                    reverted_action.resource_id(),
                    ProviderKind::Registry,
                ),
                updated_at_ms: 10_000,
            })
            .unwrap();
        store
            .set_step_status(&skipped_tx, 0, StepStatus::Reverted, 10_001, None)
            .unwrap();

        let report = DriftDetector::new(store, fx.mgr.registry())
            .detect()
            .unwrap();
        assert_eq!(report.checked, 1);
        assert!(report.is_clean());
    }

    #[test]
    fn per_resource_error_does_not_abort_scan() {
        let fx = fixture();
        let tx = fx
            .mgr
            .apply(
                &[manifest(
                    "tweak.multi",
                    vec![
                        reg("A", 1),
                        svc("DiagTrack", ServiceStartType::Disabled),
                        svc("WSearch", ServiceStartType::Manual),
                    ],
                )],
                None,
                &facts(),
            )
            .unwrap();

        // Cause verify error on DiagTrack, drift on A, and clean match on WSearch.
        fx.svc_provider
            .fail_verify(&svc("DiagTrack", ServiceStartType::Disabled));
        fx.reg_provider.set(&reg("A", 0), dword(42));

        let report = DriftDetector::new(fx.mgr.store(), fx.mgr.registry())
            .detect()
            .unwrap();
        assert_eq!(report.checked, 3);
        assert!(!report.is_clean());
        assert_eq!(
            report.drifted,
            vec![DriftEntry {
                transaction_id: tx,
                tweak_id: TweakId::new("tweak.multi"),
                provider: ProviderKind::Registry,
                resource: reg("A", 0).resource_id(),
                expected: Some(dword(1)),
                actual: Some(dword(42)),
            }]
        );
        assert_eq!(
            report.errors,
            vec![DriftCheckError {
                resource: svc("DiagTrack", ServiceStartType::Disabled).resource_id(),
                error: AppError::AccessDenied {
                    resource: "service:diagtrack".into(),
                },
            }]
        );

        // Missing provider in registry is also recorded per-resource without aborting.
        let mut partial_registry = ProviderRegistry::new();
        partial_registry
            .register(Box::new(fx.reg_provider.clone()))
            .unwrap();
        let partial_report = DriftDetector::new(fx.mgr.store(), &partial_registry)
            .detect()
            .unwrap();
        assert_eq!(partial_report.checked, 3);
        assert_eq!(partial_report.drifted.len(), 1);
        assert_eq!(partial_report.errors.len(), 2);
        assert_eq!(
            partial_report.errors.first().map(|e| &e.error),
            Some(&AppError::ProviderNotRegistered {
                provider: ProviderKind::Service
            })
        );
    }
}
