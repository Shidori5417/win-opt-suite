//! Startup crash recovery against a file-backed store: a transaction left
//! APPLYING by a "crash" is found, blocks new work, is recovered, and the
//! database survives reopening.

mod common;

use common::{
    CRASHED_TX, InMemoryMockProvider, Op, TempDir, dword, facts, manager_on_file,
    registry_manifest, simulate_crash, tx_status,
};
use core_engine::Store;
use rusqlite::Connection;
use shared_types::{AppError, TransactionId, TransactionStatus};

#[test]
fn crash_mid_apply_is_found_blocks_apply_and_is_recovered_after_reopen() {
    let dir = TempDir::new("crash-recover");
    let db = dir.path().join("state.db");
    let lock = dir.path().join("tx.lock");
    let provider = InMemoryMockProvider::new();
    // Pre-existing value that the tweak overwrites; recovery must restore it.
    let first = registry_manifest("test.crash-1", 4).actions[0].clone();
    provider.seed(&first, dword(0));
    let before_crash = provider.snapshot_state();

    let actions = simulate_crash(&db, &provider);
    // The "system" keeps the half-applied writes across the crash.
    assert_eq!(provider.state_of(&actions[2]), Some(dword(1)));
    assert_eq!(provider.state_of(&actions[3]), None);

    // Fresh process: new store handle, new manager, same database file.
    let mgr = manager_on_file(&provider, &db, &lock);
    let incomplete = mgr.find_incomplete().unwrap();
    assert_eq!(incomplete.len(), 1);
    assert_eq!(incomplete[0].id, TransactionId::new(CRASHED_TX));
    assert_eq!(incomplete[0].status, TransactionStatus::Applying);

    provider.clear_log();
    let refused = mgr
        .apply(&[registry_manifest("test.crash-2", 1)], None, &facts())
        .unwrap_err();
    assert_eq!(
        refused,
        AppError::RecoveryRequired {
            transaction_id: CRASHED_TX.to_owned()
        }
    );
    assert!(
        provider.log().is_empty(),
        "refused apply must not touch the system"
    );

    let id = TransactionId::new(CRASHED_TX);
    assert_eq!(mgr.recover(&id).unwrap(), TransactionStatus::RolledBack);
    assert_eq!(tx_status(&mgr, &id), TransactionStatus::RolledBack);

    // Reverse order over the PENDING step and the APPLIED steps.
    assert_eq!(
        provider.log(),
        vec![
            (Op::Revert, actions[2].resource_id()),
            (Op::Revert, actions[1].resource_id()),
            (Op::Revert, actions[0].resource_id()),
        ]
    );
    assert_eq!(provider.snapshot_state(), before_crash);
    assert!(mgr.find_incomplete().unwrap().is_empty());

    // Work is unblocked again.
    mgr.apply(&[registry_manifest("test.crash-2", 1)], None, &facts())
        .expect("apply after recovery");
}

#[test]
fn reopening_the_same_database_is_idempotent() {
    let dir = TempDir::new("crash-reopen");
    let db = dir.path().join("state.db");
    let lock = dir.path().join("tx.lock");
    let provider = InMemoryMockProvider::new();
    let mgr = manager_on_file(&provider, &db, &lock);
    let id = mgr
        .apply(&[registry_manifest("test.reopen-1", 2)], None, &facts())
        .unwrap();
    drop(mgr);

    let migration_state = || {
        let conn = Connection::open(&db).unwrap();
        let (count, max_applied): (i64, i64) = conn
            .query_row(
                "SELECT count(*), max(applied_at_ms) FROM schema_migrations",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        let user_version: u32 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        (count, max_applied, user_version)
    };
    let before = migration_state();
    assert!(before.0 >= 1, "initial migration recorded");

    for _ in 0..3 {
        let store = Store::open(&db).expect("reopen must not fail");
        assert_eq!(migration_state(), before, "migrations must not re-run");
        assert_eq!(store.summaries().unwrap().len(), 1, "data survives reopen");
        assert!(store.get_transaction(&id).unwrap().is_some());
    }
    assert_eq!(migration_state(), before);
}
