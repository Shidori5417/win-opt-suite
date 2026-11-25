//! Saga apply, reverse-order rollback and user rollback through the public
//! `TransactionManager` API, driven by the in-memory mock provider.

mod common;

use common::{
    InMemoryMockProvider, Op, TempDir, dword, facts, manager_in_memory, only_tx, reg_action,
    registry_manifest, step_statuses, string_value, tx_status,
};
use shared_types::{ActionSpec, AppError, StepStatus, TransactionId, TransactionStatus};

/// One tweak with four registry actions; returns the manifest and actions.
fn four_actions() -> (shared_types::TweakManifest, Vec<ActionSpec>) {
    let m = registry_manifest("test.saga-1", 4);
    let actions = m.actions.clone();
    (m, actions)
}

#[test]
fn failed_apply_reverts_pending_then_applied_steps_in_strict_reverse_order() {
    let dir = TempDir::new("saga-order");
    let provider = InMemoryMockProvider::new();
    let (m, a) = four_actions();

    // Pre-existing state: a0 has an old value; an unrelated resource must
    // survive untouched.
    provider.seed(&a[0], dword(0));
    provider.seed(&reg_action("unrelated", 0), dword(9));
    provider.fail_apply_on(&a[2]);
    let before = provider.snapshot_state();

    let mgr = manager_in_memory(&provider, &dir);
    let err = mgr.apply(&[m], None, &facts()).unwrap_err();
    assert!(matches!(err, AppError::AccessDenied { .. }), "{err:?}");

    let id = only_tx(&mgr);
    let r = |i: usize| a[i].resource_id();
    assert_eq!(
        provider.log(),
        vec![
            (Op::Read, r(0)),
            (Op::Apply, r(0)),
            (Op::Verify, r(0)),
            (Op::Read, r(1)),
            (Op::Apply, r(1)),
            (Op::Verify, r(1)),
            (Op::Read, r(2)),
            // The failing PENDING step is reverted first, then the APPLIED
            // steps strictly in reverse. a3 is never read or applied.
            (Op::Apply, r(2)),
            (Op::Revert, r(2)),
            (Op::Revert, r(1)),
            (Op::Revert, r(0)),
        ]
    );
    assert_eq!(provider.snapshot_state(), before, "exact initial state");
    assert_eq!(tx_status(&mgr, &id), TransactionStatus::RolledBack);
    assert_eq!(step_statuses(&mgr, &id), vec![StepStatus::Reverted; 3]);
    let history = mgr.history().unwrap();
    assert_eq!(history[0].status, TransactionStatus::RolledBack);
}

#[test]
fn created_resources_are_deleted_and_preexisting_restored_exactly() {
    let dir = TempDir::new("saga-restore");
    let provider = InMemoryMockProvider::new();
    let (m, a) = four_actions();

    // a0 pre-exists with a string value (not the DWORD the tweak writes).
    provider.seed(&a[0], string_value("old"));
    // a2 fails after writing its new value (partial write): it did not exist
    // before, so it must be deleted, not reset.
    provider.fail_apply_after_partial_write_on(&a[2]);

    let mgr = manager_in_memory(&provider, &dir);
    assert!(mgr.apply(&[m], None, &facts()).is_err());

    assert_eq!(provider.state_of(&a[0]), Some(string_value("old")));
    assert_eq!(provider.state_of(&a[1]), None, "created then deleted");
    assert_eq!(
        provider.state_of(&a[2]),
        None,
        "partially written then deleted"
    );
    assert_eq!(provider.state_of(&a[3]), None, "never touched");
}

#[test]
fn revert_failure_reports_rollback_incomplete_and_still_reverts_others() {
    let dir = TempDir::new("saga-revert-fail");
    let provider = InMemoryMockProvider::new();
    let (m, a) = four_actions();
    provider.seed(&a[0], dword(0));
    provider.fail_apply_on(&a[2]);
    provider.fail_revert_on(&a[1]);

    let mgr = manager_in_memory(&provider, &dir);
    let err = mgr.apply(&[m], None, &facts()).unwrap_err();
    let (transaction_id, failed_resources) = match err {
        AppError::RollbackIncomplete {
            transaction_id,
            failed_resources,
        } => (transaction_id, failed_resources),
        other => panic!("expected RollbackIncomplete, got {other:?}"),
    };
    let id = only_tx(&mgr);
    assert_eq!(transaction_id, id.to_string());
    assert_eq!(failed_resources, vec![a[1].resource_id().to_string()]);

    assert_eq!(tx_status(&mgr, &id), TransactionStatus::Failed);
    assert_eq!(
        step_statuses(&mgr, &id),
        [
            StepStatus::Reverted,
            StepStatus::RevertFailed,
            StepStatus::Reverted
        ]
    );
    // Reverts continued past the failure: a0 was restored, a2 reverted.
    assert_eq!(provider.state_of(&a[0]), Some(dword(0)));
    assert_eq!(provider.state_of(&a[2]), None);
    // The resource whose revert failed still holds the value it was given.
    assert_eq!(provider.state_of(&a[1]), Some(dword(1)));
    let reverts: Vec<_> = provider
        .log()
        .into_iter()
        .filter(|(op, _)| *op == Op::Revert)
        .collect();
    assert_eq!(
        reverts,
        vec![
            (Op::Revert, a[2].resource_id()),
            (Op::Revert, a[1].resource_id()),
            (Op::Revert, a[0].resource_id()),
        ]
    );
}

#[test]
fn user_rollback_of_committed_restores_everything_in_reverse_order() {
    let dir = TempDir::new("saga-user-rollback");
    let provider = InMemoryMockProvider::new();
    let first = registry_manifest("test.saga-1", 2);
    let second = registry_manifest("test.saga-2", 2);
    let a: Vec<ActionSpec> = first
        .actions
        .iter()
        .chain(&second.actions)
        .cloned()
        .collect();
    provider.seed(&a[0], dword(0));
    let before = provider.snapshot_state();

    let mgr = manager_in_memory(&provider, &dir);
    let id = mgr.apply(&[first, second], None, &facts()).unwrap();
    assert_eq!(tx_status(&mgr, &id), TransactionStatus::Committed);
    assert_ne!(
        provider.snapshot_state(),
        before,
        "apply changed the system"
    );

    provider.clear_log();
    mgr.rollback(&id).unwrap();

    assert_eq!(
        provider.log(),
        (0..4)
            .rev()
            .map(|i| (Op::Revert, a[i].resource_id()))
            .collect::<Vec<_>>()
    );
    assert_eq!(provider.snapshot_state(), before);
    assert_eq!(tx_status(&mgr, &id), TransactionStatus::RolledBack);
    assert_eq!(step_statuses(&mgr, &id), vec![StepStatus::Reverted; 4]);
}

#[test]
fn user_rollback_of_non_committed_transaction_is_refused() {
    let dir = TempDir::new("saga-refuse");
    let provider = InMemoryMockProvider::new();
    let mgr = manager_in_memory(&provider, &dir);

    // A transaction that ended ROLLED_BACK after a failed apply.
    let (m, a) = four_actions();
    provider.fail_apply_on(&a[1]);
    assert!(mgr.apply(&[m], None, &facts()).is_err());
    let rolled_back = only_tx(&mgr);
    assert_eq!(tx_status(&mgr, &rolled_back), TransactionStatus::RolledBack);

    // A transaction that ended FAILED (a revert could not complete).
    let m2 = registry_manifest("test.saga-2", 2);
    let b = m2.actions.clone();
    provider.fail_apply_on(&b[1]);
    provider.fail_revert_on(&b[0]);
    let err = mgr.apply(&[m2], None, &facts()).unwrap_err();
    assert!(
        matches!(err, AppError::RollbackIncomplete { .. }),
        "{err:?}"
    );
    let failed: TransactionId = mgr
        .history()
        .unwrap()
        .into_iter()
        .map(|t| t.id)
        .find(|id| *id != rolled_back)
        .expect("failed transaction recorded");
    assert_eq!(tx_status(&mgr, &failed), TransactionStatus::Failed);

    provider.clear_faults();
    provider.clear_log();
    for id in [&rolled_back, &failed] {
        assert!(matches!(mgr.rollback(id), Err(AppError::Internal { .. })));
    }
    assert!(
        provider.log().is_empty(),
        "refused rollback must not touch the system"
    );
    assert_eq!(tx_status(&mgr, &rolled_back), TransactionStatus::RolledBack);
    assert_eq!(tx_status(&mgr, &failed), TransactionStatus::Failed);
    assert!(matches!(
        mgr.rollback(&TransactionId::new("missing")),
        Err(AppError::NotFound { .. })
    ));
}
