//! `TransactionManager`: saga-style apply with durable per-step snapshots,
//! reverse-order rollback and startup crash recovery (ADR-0001 §3).

use shared_types::{
    ActionSpec, AppError, AppResult, Profile, StepStatus, SystemFacts, TransactionId,
    TransactionStatus, TransactionSummary, TweakId, TweakManifest, VerifyOutcome,
};

use crate::clock::{Clock, IdGenerator, SystemClock, UuidV7Generator};
use crate::lock::TransactionLock;
use crate::planner::{DryRunPlanner, flatten_actions, has_blocking, validate_selection};
use crate::registry::ProviderRegistry;
use crate::store::{NewStep, NewTransaction, StepRecord, Store};

/// Orchestrates apply / rollback / recovery. All system access goes through
/// the [`ProviderRegistry`]; all state goes through the [`Store`].
pub struct TransactionManager {
    store: Store,
    registry: ProviderRegistry,
    lock: Box<dyn TransactionLock>,
    clock: Box<dyn Clock>,
    ids: Box<dyn IdGenerator>,
    app_version: String,
}

impl std::fmt::Debug for TransactionManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransactionManager")
            .field("registry", &self.registry)
            .field("app_version", &self.app_version)
            .finish_non_exhaustive()
    }
}

impl TransactionManager {
    /// Creates a manager with the system clock and UUIDv7 transaction ids.
    pub fn new(store: Store, registry: ProviderRegistry, lock: Box<dyn TransactionLock>) -> Self {
        Self {
            store,
            registry,
            lock,
            clock: Box::new(SystemClock),
            ids: Box::new(UuidV7Generator),
            app_version: env!("CARGO_PKG_VERSION").to_owned(),
        }
    }

    /// Replaces the time source.
    #[must_use]
    pub fn with_clock(mut self, clock: Box<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// Replaces the transaction id generator.
    #[must_use]
    pub fn with_id_generator(mut self, ids: Box<dyn IdGenerator>) -> Self {
        self.ids = ids;
        self
    }

    /// Overrides the application version recorded in new transactions.
    #[must_use]
    pub fn with_app_version(mut self, version: impl Into<String>) -> Self {
        self.app_version = version.into();
        self
    }

    /// The underlying state store.
    pub fn store(&self) -> &Store {
        &self.store
    }

    /// The provider registry.
    pub fn registry(&self) -> &ProviderRegistry {
        &self.registry
    }

    /// A read-only planner over this manager's providers.
    pub fn planner(&self) -> DryRunPlanner<'_> {
        DryRunPlanner::new(&self.registry)
    }

    /// Applies `manifests` as one atomic transaction.
    ///
    /// On any failure every touched step is reverted in reverse order and the
    /// original error is returned (or [`AppError::RollbackIncomplete`] when a
    /// revert itself failed).
    ///
    /// # Errors
    /// `LockUnavailable`, `RecoveryRequired`, `InvalidManifest`,
    /// `ConflictBlocked`, `ProviderNotRegistered`, plus any provider,
    /// `VerificationFailed` or storage error raised while applying.
    pub fn apply(
        &self,
        manifests: &[TweakManifest],
        profile: Option<Profile>,
        facts: &SystemFacts,
    ) -> AppResult<TransactionId> {
        let _guard = self.lock.acquire()?;
        self.ensure_no_incomplete()?;

        let warnings = validate_selection(manifests, facts)?;
        if has_blocking(&warnings) {
            return Err(AppError::ConflictBlocked { warnings });
        }
        let plan = flatten_actions(manifests);
        // Fail before touching anything when an adapter is missing.
        for (_, action) in &plan {
            self.registry.get(action.provider_kind())?;
        }

        let id = self.ids.next_id();
        self.store.insert_transaction(&NewTransaction {
            id: id.clone(),
            created_at_ms: self.clock.now_ms(),
            status: TransactionStatus::Pending,
            profile,
            tweak_ids: manifests.iter().map(|m| m.id.clone()).collect(),
            windows_build: facts.windows_build,
            windows_ubr: facts.windows_ubr,
            app_version: self.app_version.clone(),
        })?;
        self.set_status(&id, TransactionStatus::Applying, None)?;

        match self.run_steps(&id, &plan) {
            Ok(()) => {
                self.set_status(&id, TransactionStatus::Committed, None)?;
                Ok(id)
            }
            Err(err) => {
                self.set_status(&id, TransactionStatus::RollingBack, Some(&err))?;
                self.revert_transaction(&id, Some(err)).map(|()| id)
            }
        }
    }

    /// Rolls a `COMMITTED` transaction back (reverse step order).
    ///
    /// # Errors
    /// `LockUnavailable`, `RecoveryRequired`, `NotFound`, `Internal` when the
    /// transaction is not `COMMITTED`, or `RollbackIncomplete`.
    pub fn rollback(&self, transaction_id: &TransactionId) -> AppResult<()> {
        let _guard = self.lock.acquire()?;
        self.ensure_no_incomplete()?;
        let record = self.load(transaction_id)?;
        if record.status != TransactionStatus::Committed {
            return Err(AppError::Internal {
                message: format!(
                    "transaction {transaction_id} is {}; only COMMITTED transactions can be rolled back",
                    record.status
                ),
            });
        }
        self.set_status(transaction_id, TransactionStatus::RollingBack, None)?;
        self.revert_transaction(transaction_id, None)
    }

    /// Transactions left `PENDING` / `APPLYING` / `ROLLING_BACK` by a crash,
    /// newest first. A non-empty result must be recovered before applying.
    pub fn find_incomplete(&self) -> AppResult<Vec<TransactionSummary>> {
        let mut all = self.store.summaries()?;
        all.retain(|t| t.status.is_incomplete());
        Ok(all)
    }

    /// Crash recovery: reverts the `APPLIED` and `PENDING` steps of an
    /// incomplete transaction (also retries a `FAILED` one) in reverse order,
    /// skipping steps already `REVERTED`. Returns the final status.
    ///
    /// # Errors
    /// `LockUnavailable`, `NotFound`, `Internal` when the transaction needs no
    /// recovery, or `RollbackIncomplete` when a revert failed again.
    pub fn recover(&self, transaction_id: &TransactionId) -> AppResult<TransactionStatus> {
        let _guard = self.lock.acquire()?;
        let record = self.load(transaction_id)?;
        if !record.status.is_incomplete() && record.status != TransactionStatus::Failed {
            return Err(AppError::Internal {
                message: format!(
                    "transaction {transaction_id} is {}; nothing to recover",
                    record.status
                ),
            });
        }
        self.set_status(transaction_id, TransactionStatus::RollingBack, None)?;
        self.revert_transaction(transaction_id, None)
            .map(|()| TransactionStatus::RolledBack)
    }

    /// All transactions, newest first.
    pub fn history(&self) -> AppResult<Vec<TransactionSummary>> {
        self.store.summaries()
    }

    // ------------------------------------------------------------ internals --

    fn load(&self, id: &TransactionId) -> AppResult<crate::store::TransactionRecord> {
        self.store
            .get_transaction(id)?
            .ok_or_else(|| AppError::NotFound {
                resource: format!("transaction {id}"),
            })
    }

    fn ensure_no_incomplete(&self) -> AppResult<()> {
        match self.find_incomplete()?.first() {
            Some(t) => Err(AppError::RecoveryRequired {
                transaction_id: t.id.to_string(),
            }),
            None => Ok(()),
        }
    }

    fn set_status(
        &self,
        id: &TransactionId,
        status: TransactionStatus,
        error: Option<&AppError>,
    ) -> AppResult<()> {
        self.store
            .set_transaction_status(id, status, self.clock.now_ms(), error)
    }

    fn set_step(
        &self,
        id: &TransactionId,
        seq: u32,
        status: StepStatus,
        error: Option<&AppError>,
    ) -> AppResult<()> {
        self.store
            .set_step_status(id, seq, status, self.clock.now_ms(), error)
    }

    /// Forward path: read -> persist PENDING -> apply -> verify -> APPLIED.
    fn run_steps(&self, id: &TransactionId, plan: &[(TweakId, ActionSpec)]) -> AppResult<()> {
        for (index, (tweak_id, action)) in plan.iter().enumerate() {
            let seq = u32::try_from(index).map_err(|_| AppError::Internal {
                message: "too many actions in one transaction".to_owned(),
            })?;
            let provider = self.registry.get(action.provider_kind())?;

            let snapshot = provider.read(action)?;
            if !snapshot.is_consistent() {
                return Err(AppError::Internal {
                    message: format!(
                        "provider returned an inconsistent snapshot for {}",
                        action.resource_id()
                    ),
                });
            }
            // Durable BEFORE the first write.
            self.store.insert_step(&NewStep {
                transaction_id: id.clone(),
                seq,
                tweak_id: tweak_id.clone(),
                action: action.clone(),
                snapshot,
                updated_at_ms: self.clock.now_ms(),
            })?;

            provider.apply(action)?;
            if let VerifyOutcome::Drifted { .. } = provider.verify(action)? {
                return Err(AppError::VerificationFailed {
                    resource: action.resource_id().to_string(),
                });
            }
            self.set_step(id, seq, StepStatus::Applied, None)?;
        }
        Ok(())
    }

    /// Reverts every `PENDING` / `APPLIED` / `REVERT_FAILED` step in strictly
    /// descending `seq` order (best effort: a failing revert does not stop
    /// the others) and finalises the transaction.
    ///
    /// Returns `Err(cause)` when given (the original apply error) after a
    /// complete rollback, or `Err(RollbackIncomplete)` when any revert failed.
    fn revert_transaction(&self, id: &TransactionId, cause: Option<AppError>) -> AppResult<()> {
        let steps = self.store.steps(id)?;
        let mut failed: Vec<String> = Vec::new();
        for step in steps.iter().rev() {
            if step.status == StepStatus::Reverted {
                continue;
            }
            if let Err(err) = self.revert_step(step) {
                self.set_step(id, step.seq, StepStatus::RevertFailed, Some(&err))?;
                failed.push(step.resource.to_string());
            } else {
                self.set_step(id, step.seq, StepStatus::Reverted, None)?;
            }
        }

        if failed.is_empty() {
            self.set_status(id, TransactionStatus::RolledBack, None)?;
            match cause {
                Some(err) => Err(err),
                None => Ok(()),
            }
        } else {
            let err = AppError::RollbackIncomplete {
                transaction_id: id.to_string(),
                failed_resources: failed,
            };
            self.set_status(id, TransactionStatus::Failed, Some(&err))?;
            Err(err)
        }
    }

    fn revert_step(&self, step: &StepRecord) -> AppResult<()> {
        self.registry
            .get(step.provider)?
            .revert(&step.action, &step.snapshot)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicI64, AtomicU32, Ordering};

    use shared_types::{ConflictKind, ProviderKind};

    use super::*;
    use crate::lock::FileLock;
    use crate::testing::{FakeProvider, dword, facts, manifest, reg};

    static DIR_COUNTER: AtomicU32 = AtomicU32::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let n = DIR_COUNTER.fetch_add(1, Ordering::SeqCst);
            let dir =
                std::env::temp_dir().join(format!("core-engine-mgr-{}-{n}", std::process::id()));
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
            self.0.fetch_add(1, Ordering::SeqCst)
        }
    }

    struct SeqIds(AtomicU32);

    impl IdGenerator for SeqIds {
        fn next_id(&self) -> TransactionId {
            TransactionId::new(format!("tx-{:04}", self.0.fetch_add(1, Ordering::SeqCst)))
        }
    }

    struct Fx {
        provider: FakeProvider,
        mgr: TransactionManager,
        dir: TempDir,
    }

    fn fx() -> Fx {
        let dir = TempDir::new();
        let provider = FakeProvider::default();
        let mut registry = ProviderRegistry::new();
        registry.register(Box::new(provider.clone())).unwrap();
        let mgr = TransactionManager::new(
            Store::open_in_memory().unwrap(),
            registry,
            Box::new(FileLock::new(dir.0.join("tx.lock"))),
        )
        .with_clock(Box::new(TickClock(AtomicI64::new(1_000))))
        .with_id_generator(Box::new(SeqIds(AtomicU32::new(1))));
        Fx { provider, mgr, dir }
    }

    fn four() -> Vec<TweakManifest> {
        vec![
            manifest("tweak.one", vec![reg("A", 1), reg("B", 1)]),
            manifest("tweak.two", vec![reg("C", 1), reg("D", 1)]),
        ]
    }

    fn step_statuses(fx: &Fx, id: &TransactionId) -> Vec<StepStatus> {
        fx.mgr
            .store()
            .steps(id)
            .unwrap()
            .iter()
            .map(|s| s.status)
            .collect()
    }

    fn tx_status(fx: &Fx, id: &TransactionId) -> TransactionStatus {
        fx.mgr.store().get_transaction(id).unwrap().unwrap().status
    }

    #[test]
    fn happy_path_commits() {
        let fx = fx();
        fx.provider.set(&reg("A", 0), dword(0));
        let id = fx
            .mgr
            .apply(&four(), Some(Profile::Gaming), &facts())
            .unwrap();
        assert_eq!(id.as_str(), "tx-0001");
        assert_eq!(tx_status(&fx, &id), TransactionStatus::Committed);
        assert_eq!(step_statuses(&fx, &id), vec![StepStatus::Applied; 4]);
        for n in ["A", "B", "C", "D"] {
            assert_eq!(fx.provider.get(&reg(n, 0)), Some(dword(1)));
        }
        let h = fx.mgr.history().unwrap();
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].profile, Some(Profile::Gaming));
        assert_eq!(h[0].step_count, 4);
        assert_eq!(h[0].tweak_ids.len(), 2);
        // snapshots were captured before the writes
        let steps = fx.mgr.store().steps(&id).unwrap();
        assert_eq!(steps[0].snapshot.state, Some(dword(0)));
        assert!(!steps[1].snapshot.existed);
    }

    #[test]
    fn failure_at_step_three_of_four_reverts_in_reverse_order() {
        let fx = fx();
        fx.provider.set(&reg("A", 0), dword(0));
        fx.provider
            .with(|i| i.fail_apply.insert(reg("C", 0).resource_id()));
        let err = fx.mgr.apply(&four(), None, &facts()).unwrap_err();
        assert!(matches!(err, AppError::AccessDenied { .. }), "{err:?}");

        let id = TransactionId::new("tx-0001");
        assert_eq!(tx_status(&fx, &id), TransactionStatus::RolledBack);
        // Step 4 never started; step 3 (failed, PENDING) is reverted first.
        assert_eq!(
            fx.provider.log(),
            [
                r"apply:registry:hklm\software\test\a",
                r"apply:registry:hklm\software\test\b",
                r"apply:registry:hklm\software\test\c",
                r"revert:registry:hklm\software\test\c",
                r"revert:registry:hklm\software\test\b",
                r"revert:registry:hklm\software\test\a",
            ]
        );
        assert_eq!(step_statuses(&fx, &id), vec![StepStatus::Reverted; 3]);
        assert_eq!(fx.provider.get(&reg("A", 0)), Some(dword(0)));
        let stored = fx.mgr.store().get_transaction(&id).unwrap().unwrap();
        assert!(matches!(stored.error, Some(AppError::AccessDenied { .. })));
    }

    #[test]
    fn nonexistent_resources_are_deleted_on_revert() {
        let fx = fx();
        fx.provider.with(|i| {
            i.fail_apply.insert(reg("D", 0).resource_id());
            i.write_before_fail = true; // D is half-written when it fails
        });
        fx.mgr.apply(&four(), None, &facts()).unwrap_err();
        for n in ["A", "B", "C", "D"] {
            assert_eq!(fx.provider.get(&reg(n, 0)), None, "{n} must be deleted");
        }
    }

    #[test]
    fn verification_mismatch_fails_and_rolls_back() {
        let fx = fx();
        fx.provider
            .with(|i| i.noop_apply.insert(reg("B", 0).resource_id()));
        let err = fx.mgr.apply(&four(), None, &facts()).unwrap_err();
        assert_eq!(
            err,
            AppError::VerificationFailed {
                resource: reg("B", 0).resource_id().to_string()
            }
        );
        let id = TransactionId::new("tx-0001");
        assert_eq!(tx_status(&fx, &id), TransactionStatus::RolledBack);
        assert_eq!(fx.provider.get(&reg("A", 0)), None);
    }

    #[test]
    fn failed_revert_marks_failed_and_reports_incomplete_rollback() {
        let fx = fx();
        fx.provider.with(|i| {
            i.fail_apply.insert(reg("C", 0).resource_id());
            i.fail_revert.insert(reg("B", 0).resource_id());
        });
        let err = fx.mgr.apply(&four(), None, &facts()).unwrap_err();
        assert_eq!(
            err,
            AppError::RollbackIncomplete {
                transaction_id: "tx-0001".into(),
                failed_resources: vec![reg("B", 0).resource_id().to_string()],
            }
        );
        let id = TransactionId::new("tx-0001");
        assert_eq!(tx_status(&fx, &id), TransactionStatus::Failed);
        assert_eq!(
            step_statuses(&fx, &id),
            [
                StepStatus::Reverted,
                StepStatus::RevertFailed,
                StepStatus::Reverted
            ]
        );
        // FAILED is not "incomplete": new work is allowed, retry is possible.
        assert!(fx.mgr.find_incomplete().unwrap().is_empty());
        fx.provider.with(|i| i.fail_revert.clear());
        assert_eq!(fx.mgr.recover(&id).unwrap(), TransactionStatus::RolledBack);
        assert_eq!(fx.provider.get(&reg("B", 0)), None);
    }

    #[test]
    fn user_rollback_restores_state() {
        let fx = fx();
        fx.provider.set(&reg("B", 0), dword(7));
        let id = fx.mgr.apply(&four(), None, &facts()).unwrap();
        fx.provider.with(|i| i.log.clear());
        fx.mgr.rollback(&id).unwrap();
        assert_eq!(tx_status(&fx, &id), TransactionStatus::RolledBack);
        assert_eq!(step_statuses(&fx, &id), vec![StepStatus::Reverted; 4]);
        assert_eq!(fx.provider.get(&reg("B", 0)), Some(dword(7)));
        assert_eq!(fx.provider.get(&reg("A", 0)), None);
        let log = fx.provider.log();
        assert!(
            log[0].ends_with(r"\d") && log[3].ends_with(r"\a"),
            "{log:?}"
        );
        // cannot roll back twice / unknown id
        assert!(matches!(
            fx.mgr.rollback(&id),
            Err(AppError::Internal { .. })
        ));
        assert!(matches!(
            fx.mgr.rollback(&TransactionId::new("nope")),
            Err(AppError::NotFound { .. })
        ));
    }

    /// Leaves the DB as a crash would: tx APPLYING, steps 0-1 APPLIED, step 2
    /// PENDING with the write already done, step 3 never started.
    fn simulate_crash(fx: &Fx) -> TransactionId {
        let id = TransactionId::new("crashed");
        let store = fx.mgr.store();
        store
            .insert_transaction(&NewTransaction {
                id: id.clone(),
                created_at_ms: 1,
                status: TransactionStatus::Applying,
                profile: None,
                tweak_ids: vec![TweakId::new("tweak.one")],
                windows_build: 26100,
                windows_ubr: 1,
                app_version: "test".into(),
            })
            .unwrap();
        for (seq, name) in ["A", "B", "C"].iter().enumerate() {
            let action = reg(name, 1);
            let provider = fx.mgr.registry().get(ProviderKind::Registry).unwrap();
            let snapshot = provider.read(&action).unwrap();
            let seq = u32::try_from(seq).unwrap();
            store
                .insert_step(&NewStep {
                    transaction_id: id.clone(),
                    seq,
                    tweak_id: TweakId::new("tweak.one"),
                    action: action.clone(),
                    snapshot,
                    updated_at_ms: 1,
                })
                .unwrap();
            provider.apply(&action).unwrap();
            if seq < 2 {
                store
                    .set_step_status(&id, seq, StepStatus::Applied, 2, None)
                    .unwrap();
            }
        }
        id
    }

    #[test]
    fn crash_recovery_restores_system_and_unblocks_apply() {
        let fx = fx();
        fx.provider.set(&reg("A", 0), dword(0));
        let id = simulate_crash(&fx);
        assert_eq!(fx.provider.get(&reg("C", 0)), Some(dword(1)));

        let incomplete = fx.mgr.find_incomplete().unwrap();
        assert_eq!(incomplete.len(), 1);
        assert_eq!(incomplete[0].id, id);
        assert_eq!(
            fx.mgr.apply(&four(), None, &facts()).unwrap_err(),
            AppError::RecoveryRequired {
                transaction_id: "crashed".into()
            }
        );

        fx.provider.with(|i| i.log.clear());
        assert_eq!(fx.mgr.recover(&id).unwrap(), TransactionStatus::RolledBack);
        let log = fx.provider.log();
        assert!(
            log[0].ends_with(r"\c") && log[1].ends_with(r"\b") && log[2].ends_with(r"\a"),
            "{log:?}"
        );
        assert_eq!(tx_status(&fx, &id), TransactionStatus::RolledBack);
        assert_eq!(fx.provider.get(&reg("A", 0)), Some(dword(0)));
        for n in ["B", "C"] {
            assert_eq!(fx.provider.get(&reg(n, 0)), None);
        }
        assert!(fx.mgr.find_incomplete().unwrap().is_empty());
        assert!(fx.mgr.apply(&four(), None, &facts()).is_ok());
        // recovering a finished transaction is refused
        assert!(fx.mgr.recover(&id).is_err());
    }

    #[test]
    fn recovery_skips_already_reverted_steps() {
        let fx = fx();
        let id = simulate_crash(&fx);
        fx.mgr
            .store()
            .set_step_status(&id, 1, StepStatus::Reverted, 3, None)
            .unwrap();
        fx.mgr
            .store()
            .set_transaction_status(&id, TransactionStatus::RollingBack, 3, None)
            .unwrap();
        fx.provider.with(|i| i.log.clear());
        fx.mgr.recover(&id).unwrap();
        let log = fx.provider.log();
        assert_eq!(log.len(), 2, "{log:?}");
        assert!(log.iter().all(|l| !l.ends_with(r"\b")));
    }

    #[test]
    fn lock_contention_blocks_apply_without_side_effects() {
        let fx = fx();
        let held = FileLock::new(fx.dir.0.join("tx.lock")).acquire().unwrap();
        assert_eq!(
            fx.mgr.apply(&four(), None, &facts()).unwrap_err(),
            AppError::LockUnavailable
        );
        assert!(fx.mgr.history().unwrap().is_empty());
        assert!(fx.provider.log().is_empty());
        drop(held);
        assert!(fx.mgr.apply(&four(), None, &facts()).is_ok());
    }

    #[test]
    fn blocking_conflicts_refuse_apply_before_any_write() {
        let fx = fx();
        let a = manifest("tweak.a", vec![reg("X", 1)]);
        let b = manifest("tweak.b", vec![reg("X", 2)]);
        match fx.mgr.apply(&[a, b], None, &facts()).unwrap_err() {
            AppError::ConflictBlocked { warnings } => {
                assert!(matches!(
                    warnings[0].kind,
                    ConflictKind::ResourceContention { .. }
                ));
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(fx.mgr.history().unwrap().is_empty());
        assert!(fx.provider.log().is_empty());
    }

    #[test]
    fn invalid_manifest_and_missing_provider_are_refused_up_front() {
        let fx = fx();
        let bad = manifest("tweak.a", vec![]);
        assert!(matches!(
            fx.mgr.apply(&[bad], None, &facts()),
            Err(AppError::InvalidManifest { .. })
        ));

        let empty = TransactionManager::new(
            Store::open_in_memory().unwrap(),
            ProviderRegistry::new(),
            Box::new(FileLock::new(fx.dir.0.join("other.lock"))),
        );
        assert_eq!(
            empty.apply(&four(), None, &facts()).unwrap_err(),
            AppError::ProviderNotRegistered {
                provider: ProviderKind::Registry
            }
        );
        assert!(empty.history().unwrap().is_empty());
    }

    #[test]
    fn history_is_newest_first() {
        let fx = fx();
        let first = fx
            .mgr
            .apply(&[manifest("tweak.a", vec![reg("A", 1)])], None, &facts())
            .unwrap();
        let second = fx
            .mgr
            .apply(&[manifest("tweak.b", vec![reg("B", 1)])], None, &facts())
            .unwrap();
        let ids: Vec<_> = fx
            .mgr
            .history()
            .unwrap()
            .into_iter()
            .map(|t| t.id)
            .collect();
        assert_eq!(ids, [second, first]);
    }
}
