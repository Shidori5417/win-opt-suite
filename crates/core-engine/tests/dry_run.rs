//! Dry-run planning: the planner only reads, classifies changes correctly,
//! and blocking conflicts are reported and refuse `apply` before any write.

mod common;

use common::{
    InMemoryMockProvider, Op, TempDir, dword, facts, manager_in_memory, manifest, reg_action,
};
use core_engine::DryRunPlanner;
use shared_types::{AppError, ChangeKind, ConflictKind, Severity, TweakId};

#[test]
fn planner_classifies_create_modify_and_nochange_without_writing() {
    let dir = TempDir::new("dry-classify");
    let provider = InMemoryMockProvider::new();
    let modify = reg_action("modify", 1);
    let create = reg_action("create", 1);
    let no_change = reg_action("no-change", 1);
    provider.seed(&modify, dword(0));
    provider.seed(&no_change, dword(1));
    let before = provider.snapshot_state();

    let mut m = manifest(
        "test.dry-1",
        vec![modify.clone(), create.clone(), no_change.clone()],
    );
    m.requires_reboot = true;

    let mgr = manager_in_memory(&provider, &dir);
    let diff = mgr.planner().plan(&[m], &facts()).unwrap();

    let changes: Vec<ChangeKind> = diff.entries.iter().map(|e| e.change).collect();
    assert_eq!(
        changes,
        [ChangeKind::Modify, ChangeKind::Create, ChangeKind::NoChange]
    );
    assert_eq!(diff.entries[0].before, Some(dword(0)));
    assert_eq!(diff.entries[0].after, Some(dword(1)));
    assert_eq!(diff.entries[1].before, None);
    assert!(diff.requires_reboot);
    assert!(diff.warnings.is_empty());
    assert_eq!(diff.effective_changes().count(), 2);

    // Planning never calls apply, revert or verify.
    assert!(
        provider.log().iter().all(|(op, _)| *op == Op::Read),
        "planner must only read: {:?}",
        provider.log()
    );
    assert_eq!(provider.snapshot_state(), before, "system unchanged");
    assert!(mgr.history().unwrap().is_empty());
}

#[test]
fn planner_free_function_also_only_reads() {
    let dir = TempDir::new("dry-free-fn");
    let provider = InMemoryMockProvider::new();
    let mgr = manager_in_memory(&provider, &dir);
    let m = manifest("test.dry-2", vec![reg_action("v", 3)]);
    let diff = DryRunPlanner::new(mgr.registry())
        .plan(&[m], &facts())
        .unwrap();
    assert_eq!(diff.entries[0].change, ChangeKind::Create);
    assert!(provider.log().iter().all(|(op, _)| *op == Op::Read));
}

#[test]
fn declared_conflict_is_reported_and_apply_is_refused_without_apply_calls() {
    let dir = TempDir::new("dry-declared");
    let provider = InMemoryMockProvider::new();
    let mut a = manifest("test.dry-a", vec![reg_action("a", 1)]);
    a.conflicts_with = vec![TweakId::new("test.dry-b")];
    let b = manifest("test.dry-b", vec![reg_action("b", 1)]);

    let mgr = manager_in_memory(&provider, &dir);
    let diff = mgr
        .planner()
        .plan(&[a.clone(), b.clone()], &facts())
        .unwrap();
    assert!(diff.has_blocking_conflicts());
    assert!(diff.warnings.iter().any(|w| {
        w.severity == Severity::Blocking && matches!(w.kind, ConflictKind::DeclaredConflict { .. })
    }));

    provider.clear_log();
    let err = mgr.apply(&[a, b], None, &facts()).unwrap_err();
    match err {
        AppError::ConflictBlocked { warnings } => assert!(
            warnings
                .iter()
                .any(|w| matches!(w.kind, ConflictKind::DeclaredConflict { .. }))
        ),
        other => panic!("expected ConflictBlocked, got {other:?}"),
    }
    assert!(
        provider.log().iter().all(|(op, _)| *op == Op::Read),
        "no Apply/Revert/Verify may happen: {:?}",
        provider.log()
    );
    assert!(mgr.history().unwrap().is_empty());
}

#[test]
fn same_resource_with_different_values_is_blocking_contention() {
    let dir = TempDir::new("dry-contention");
    let provider = InMemoryMockProvider::new();
    let a = manifest("test.dry-c1", vec![reg_action("shared", 1)]);
    let b = manifest("test.dry-c2", vec![reg_action("shared", 2)]);

    let mgr = manager_in_memory(&provider, &dir);
    let diff = mgr
        .planner()
        .plan(&[a.clone(), b.clone()], &facts())
        .unwrap();
    assert!(diff.has_blocking_conflicts());
    assert!(diff.warnings.iter().any(|w| matches!(
        &w.kind,
        ConflictKind::ResourceContention { tweaks, .. } if tweaks.len() == 2
    )));

    provider.clear_log();
    let err = mgr.apply(&[a, b], None, &facts()).unwrap_err();
    assert!(matches!(err, AppError::ConflictBlocked { .. }), "{err:?}");
    assert!(
        provider.log().iter().all(|(op, _)| *op == Op::Read),
        "{:?}",
        provider.log()
    );
    assert!(mgr.history().unwrap().is_empty());
}
