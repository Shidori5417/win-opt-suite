//! Shared fixtures for the core-engine integration tests: an in-memory
//! registry provider with fault injection and a call log, manifest and
//! facts builders, a unique temp directory, and manager constructors.
//!
//! Helpers here are used by several test binaries, so not every item is used
//! by every one of them.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use core_engine::lock::FileLock;
use core_engine::store::{NewStep, NewTransaction};
use core_engine::{ProviderRegistry, Store, TransactionManager};
use shared_types::{
    ActionSpec, AppError, AppResult, CpuInfo, ProviderKind, RegistryAction, RegistryHive,
    RegistryValue, ResourceId, ResourceState, RiskLevel, StateSnapshot, StepStatus, SystemFacts,
    SystemProvider, TransactionId, TransactionStatus, TweakCategory, TweakId, TweakManifest,
    VerifyOutcome,
};

/// Registry key used by every test action.
pub const TEST_KEY: &str = r"SOFTWARE\WinOptTest";

/// Kind of provider call, recorded in the shared call log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Read,
    Apply,
    Revert,
    Verify,
}

#[derive(Default)]
struct Faults {
    /// `apply` fails with `AccessDenied` without writing.
    apply_fail: HashSet<ResourceId>,
    /// `apply` writes the desired state, then fails with `AccessDenied`.
    apply_partial: HashSet<ResourceId>,
    /// `revert` fails with `AccessDenied` without restoring.
    revert_fail: HashSet<ResourceId>,
}

/// In-memory `SystemProvider` for `ProviderKind::Registry`.
///
/// Cloning yields another handle to the same "system": box one clone into a
/// [`ProviderRegistry`] and keep the other to seed, steer and inspect state.
#[derive(Clone, Default)]
pub struct InMemoryMockProvider {
    state: Arc<Mutex<HashMap<ResourceId, ResourceState>>>,
    faults: Arc<Mutex<Faults>>,
    log: Arc<Mutex<Vec<(Op, ResourceId)>>>,
}

impl InMemoryMockProvider {
    pub fn new() -> Self {
        Self::default()
    }

    /// A registry containing this provider (sharing its state).
    pub fn registry(&self) -> ProviderRegistry {
        let mut registry = ProviderRegistry::new();
        registry
            .register(Box::new(self.clone()))
            .expect("register mock provider");
        registry
    }

    /// Sets the live state of the resource addressed by `action`.
    pub fn seed(&self, action: &ActionSpec, state: ResourceState) {
        self.state
            .lock()
            .unwrap()
            .insert(action.resource_id(), state);
    }

    /// Current live state of the resource addressed by `action`.
    pub fn state_of(&self, action: &ActionSpec) -> Option<ResourceState> {
        self.state
            .lock()
            .unwrap()
            .get(&action.resource_id())
            .cloned()
    }

    /// Copy of the whole live state, for exact before/after comparisons.
    pub fn snapshot_state(&self) -> HashMap<ResourceId, ResourceState> {
        self.state.lock().unwrap().clone()
    }

    pub fn fail_apply_on(&self, action: &ActionSpec) {
        self.faults
            .lock()
            .unwrap()
            .apply_fail
            .insert(action.resource_id());
    }

    /// `apply` writes the desired state and then fails (partial write).
    pub fn fail_apply_after_partial_write_on(&self, action: &ActionSpec) {
        self.faults
            .lock()
            .unwrap()
            .apply_partial
            .insert(action.resource_id());
    }

    pub fn fail_revert_on(&self, action: &ActionSpec) {
        self.faults
            .lock()
            .unwrap()
            .revert_fail
            .insert(action.resource_id());
    }

    pub fn clear_faults(&self) {
        *self.faults.lock().unwrap() = Faults::default();
    }

    /// Ordered call log of every `read`/`apply`/`revert`/`verify` call.
    pub fn log(&self) -> Vec<(Op, ResourceId)> {
        self.log.lock().unwrap().clone()
    }

    pub fn clear_log(&self) {
        self.log.lock().unwrap().clear();
    }

    fn record(&self, op: Op, id: &ResourceId) {
        self.log.lock().unwrap().push((op, id.clone()));
    }
}

fn check_kind(action: &ActionSpec) -> AppResult<()> {
    let found = action.provider_kind();
    if found == ProviderKind::Registry {
        Ok(())
    } else {
        Err(AppError::ActionProviderMismatch {
            expected: ProviderKind::Registry,
            found,
        })
    }
}

fn denied(id: &ResourceId) -> AppError {
    AppError::AccessDenied {
        resource: id.to_string(),
    }
}

impl SystemProvider for InMemoryMockProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Registry
    }

    fn read(&self, action: &ActionSpec) -> AppResult<StateSnapshot> {
        check_kind(action)?;
        let id = action.resource_id();
        self.record(Op::Read, &id);
        let current = self.state.lock().unwrap().get(&id).cloned();
        Ok(match current {
            Some(state) => StateSnapshot::present(id, ProviderKind::Registry, state),
            None => StateSnapshot::absent(id, ProviderKind::Registry),
        })
    }

    fn apply(&self, action: &ActionSpec) -> AppResult<()> {
        check_kind(action)?;
        let id = action.resource_id();
        self.record(Op::Apply, &id);
        let (fail, partial) = {
            let faults = self.faults.lock().unwrap();
            let partial = faults.apply_partial.contains(&id);
            (faults.apply_fail.contains(&id) || partial, partial)
        };
        if fail && !partial {
            return Err(denied(&id));
        }
        {
            let mut state = self.state.lock().unwrap();
            match action.desired_state() {
                Some(desired) => {
                    state.insert(id.clone(), desired);
                }
                None => {
                    state.remove(&id);
                }
            }
        }
        if fail {
            return Err(denied(&id));
        }
        Ok(())
    }

    fn revert(&self, action: &ActionSpec, snapshot: &StateSnapshot) -> AppResult<()> {
        check_kind(action)?;
        let id = action.resource_id();
        self.record(Op::Revert, &id);
        if self.faults.lock().unwrap().revert_fail.contains(&id) {
            return Err(denied(&id));
        }
        let mut state = self.state.lock().unwrap();
        match (snapshot.existed, &snapshot.state) {
            (true, Some(previous)) => {
                state.insert(id, previous.clone());
            }
            _ => {
                state.remove(&id);
            }
        }
        Ok(())
    }

    fn verify(&self, action: &ActionSpec) -> AppResult<VerifyOutcome> {
        check_kind(action)?;
        let id = action.resource_id();
        self.record(Op::Verify, &id);
        let actual = self.state.lock().unwrap().get(&id).cloned();
        Ok(VerifyOutcome::compare(action.desired_state(), actual))
    }
}

// ------------------------------------------------------------- builders --

/// Registry DWORD action `HKLM\SOFTWARE\WinOptTest\<value_name>` = `value`.
pub fn reg_action(value_name: &str, value: u32) -> ActionSpec {
    ActionSpec::Registry(RegistryAction {
        hive: RegistryHive::LocalMachine,
        key: TEST_KEY.to_owned(),
        value_name: value_name.to_owned(),
        value: Some(RegistryValue::Dword(value)),
        delete: false,
    })
}

pub fn dword(value: u32) -> ResourceState {
    ResourceState::RegistryValue {
        value: RegistryValue::Dword(value),
    }
}

pub fn string_value(value: &str) -> ResourceState {
    ResourceState::RegistryValue {
        value: RegistryValue::String(value.to_owned()),
    }
}

/// A valid manifest (schema v1, Windows 11 build range, non-empty texts)
/// with the given actions.
pub fn manifest(id: &str, actions: Vec<ActionSpec>) -> TweakManifest {
    TweakManifest {
        schema_version: 1,
        id: TweakId::new(id),
        title: format!("Test tweak {id}"),
        description: "Sets test registry values for integration tests.".to_owned(),
        category: TweakCategory::Performance,
        risk: RiskLevel::Low,
        side_effects: "None; test-only values.".to_owned(),
        min_windows_build: 22000,
        max_windows_build: None,
        excluded_on: vec![],
        conflicts_with: vec![],
        profiles: vec![],
        requires_reboot: false,
        references: vec![],
        actions,
    }
}

/// A valid manifest with `count` registry actions, each on its own value
/// (`<id>-<i>`) set to `1`.
pub fn registry_manifest(id: &str, count: usize) -> TweakManifest {
    let actions = (0..count)
        .map(|i| reg_action(&format!("{id}-{i}"), 1))
        .collect();
    manifest(id, actions)
}

/// Desktop facts for a Windows 11 24H2 machine (build 26100), no laptop, no
/// anti-cheat.
pub fn facts() -> SystemFacts {
    SystemFacts {
        windows_build: 26100,
        windows_ubr: 2605,
        windows_display_version: "24H2".to_owned(),
        edition: "Professional".to_owned(),
        cpu: CpuInfo {
            brand: "Test CPU".to_owned(),
            physical_cores: 8,
            logical_cores: 16,
        },
        gpus: vec![],
        total_memory_bytes: 32 * 1024 * 1024 * 1024,
        is_laptop: false,
        on_battery: false,
        is_virtual_machine: false,
        is_domain_joined: false,
        anti_cheat: vec![],
    }
}

// ------------------------------------------------------------- temp dirs --

/// Unique directory under the OS temp dir, removed on drop.
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub fn new(tag: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "core-engine-it-{tag}-{}-{n}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

// ------------------------------------------------------------- managers --

/// Manager over a fresh in-memory store; the lock file lives in `dir`.
pub fn manager_in_memory(provider: &InMemoryMockProvider, dir: &TempDir) -> TransactionManager {
    TransactionManager::new(
        Store::open_in_memory().expect("open in-memory store"),
        provider.registry(),
        Box::new(FileLock::new(dir.path().join("tx.lock"))),
    )
}

/// Manager over the file-backed store at `db` (created or reopened).
pub fn manager_on_file(
    provider: &InMemoryMockProvider,
    db: &Path,
    lock: &Path,
) -> TransactionManager {
    TransactionManager::new(
        Store::open(db).expect("open file store"),
        provider.registry(),
        Box::new(FileLock::new(lock.to_path_buf())),
    )
}

// ------------------------------------------------------------ inspection --

/// Step states of a transaction, in sequence order.
pub fn step_statuses(mgr: &TransactionManager, id: &TransactionId) -> Vec<StepStatus> {
    mgr.store()
        .steps(id)
        .expect("load steps")
        .iter()
        .map(|s| s.status)
        .collect()
}

pub fn tx_status(mgr: &TransactionManager, id: &TransactionId) -> TransactionStatus {
    mgr.store()
        .get_transaction(id)
        .expect("load transaction")
        .expect("transaction exists")
        .status
}

/// The only transaction in the history; panics if there is not exactly one.
pub fn only_tx(mgr: &TransactionManager) -> TransactionId {
    let mut history = mgr.history().expect("load history");
    assert_eq!(history.len(), 1, "expected exactly one transaction");
    history.pop().expect("one transaction").id
}

// ------------------------------------------------------------ crash setup --

/// Transaction id used by [`simulate_crash`].
pub const CRASHED_TX: &str = "crashed-tx";

/// Leaves the database at `db` as a crash mid-apply would: the transaction
/// is APPLYING, steps 0 and 1 are APPLIED, step 2 is PENDING with its write
/// already done, and step 3 was never started. The mock system is mutated
/// accordingly. The store is dropped before returning. Returns the four
/// actions of the `test.crash-1` tweak.
pub fn simulate_crash(db: &Path, provider: &InMemoryMockProvider) -> Vec<ActionSpec> {
    let actions = registry_manifest("test.crash-1", 4).actions;
    let id = TransactionId::new(CRASHED_TX);
    let store = Store::open(db).expect("open store");
    store
        .insert_transaction(&NewTransaction {
            id: id.clone(),
            created_at_ms: 1,
            status: TransactionStatus::Applying,
            profile: None,
            tweak_ids: vec![TweakId::new("test.crash-1")],
            windows_build: 26100,
            windows_ubr: 2605,
            app_version: "test".to_owned(),
        })
        .expect("insert transaction");

    for (index, action) in actions.iter().take(3).enumerate() {
        let seq = u32::try_from(index).expect("small index");
        let snapshot = provider.read(action).expect("read");
        store
            .insert_step(&NewStep {
                transaction_id: id.clone(),
                seq,
                tweak_id: TweakId::new("test.crash-1"),
                action: action.clone(),
                snapshot,
                updated_at_ms: 1,
            })
            .expect("insert step");
        provider.apply(action).expect("apply");
        if seq < 2 {
            store
                .set_step_status(&id, seq, StepStatus::Applied, 2, None)
                .expect("mark applied");
        }
    }
    actions
}
