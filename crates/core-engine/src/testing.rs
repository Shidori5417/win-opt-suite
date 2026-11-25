//! Private test fixtures: an in-memory registry provider with failure
//! injection, manifest and facts builders.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use shared_types::{
    ActionSpec, AppError, AppResult, ProviderKind, RegistryAction, RegistryHive, RegistryValue,
    ResourceId, ResourceState, RiskLevel, StateSnapshot, SystemFacts, SystemProvider,
    TweakCategory, TweakId, TweakManifest, VerifyOutcome,
};

#[derive(Default)]
pub(crate) struct FakeInner {
    pub state: BTreeMap<ResourceId, ResourceState>,
    /// `apply` fails for these resources (after optionally writing).
    pub fail_apply: BTreeSet<ResourceId>,
    /// A failing `apply` leaves the new value behind (partial write).
    pub write_before_fail: bool,
    /// `apply` silently does nothing for these resources (verify mismatch).
    pub noop_apply: BTreeSet<ResourceId>,
    /// `revert` fails for these resources.
    pub fail_revert: BTreeSet<ResourceId>,
    /// Ordered log of `apply:<resource>` / `revert:<resource>` calls.
    pub log: Vec<String>,
}

/// Cloneable handle: one clone is boxed into the registry, the test keeps
/// another to inspect and steer the "system".
#[derive(Clone, Default)]
pub(crate) struct FakeProvider {
    pub inner: Arc<Mutex<FakeInner>>,
}

impl FakeProvider {
    pub fn with<T>(&self, f: impl FnOnce(&mut FakeInner) -> T) -> T {
        f(&mut self.inner.lock().unwrap())
    }

    pub fn set(&self, action: &ActionSpec, state: ResourceState) {
        self.with(|i| i.state.insert(action.resource_id(), state));
    }

    pub fn get(&self, action: &ActionSpec) -> Option<ResourceState> {
        self.with(|i| i.state.get(&action.resource_id()).cloned())
    }

    pub fn log(&self) -> Vec<String> {
        self.with(|i| i.log.clone())
    }
}

fn check(action: &ActionSpec) -> AppResult<()> {
    if action.provider_kind() == ProviderKind::Registry {
        Ok(())
    } else {
        Err(AppError::ActionProviderMismatch {
            expected: ProviderKind::Registry,
            found: action.provider_kind(),
        })
    }
}

impl SystemProvider for FakeProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Registry
    }

    fn read(&self, action: &ActionSpec) -> AppResult<StateSnapshot> {
        check(action)?;
        let id = action.resource_id();
        Ok(match self.with(|i| i.state.get(&id).cloned()) {
            Some(s) => StateSnapshot::present(id, ProviderKind::Registry, s),
            None => StateSnapshot::absent(id, ProviderKind::Registry),
        })
    }

    fn apply(&self, action: &ActionSpec) -> AppResult<()> {
        check(action)?;
        let id = action.resource_id();
        self.with(|i| {
            i.log.push(format!("apply:{id}"));
            let write = |i: &mut FakeInner| match action.desired_state() {
                Some(s) => {
                    i.state.insert(id.clone(), s);
                }
                None => {
                    i.state.remove(&id);
                }
            };
            if i.fail_apply.contains(&id) {
                if i.write_before_fail {
                    write(i);
                }
                return Err(AppError::AccessDenied {
                    resource: id.to_string(),
                });
            }
            if !i.noop_apply.contains(&id) {
                write(i);
            }
            Ok(())
        })
    }

    fn revert(&self, action: &ActionSpec, snapshot: &StateSnapshot) -> AppResult<()> {
        check(action)?;
        let id = action.resource_id();
        self.with(|i| {
            i.log.push(format!("revert:{id}"));
            if i.fail_revert.contains(&id) {
                return Err(AppError::AccessDenied {
                    resource: id.to_string(),
                });
            }
            match (&snapshot.existed, &snapshot.state) {
                (true, Some(s)) => {
                    i.state.insert(id, s.clone());
                }
                _ => {
                    i.state.remove(&id);
                }
            }
            Ok(())
        })
    }

    fn verify(&self, action: &ActionSpec) -> AppResult<VerifyOutcome> {
        check(action)?;
        let actual = self.get(action);
        Ok(VerifyOutcome::compare(action.desired_state(), actual))
    }
}

/// Registry DWORD action `HKLM\SOFTWARE\Test\<name>` = `value`.
pub(crate) fn reg(name: &str, value: u32) -> ActionSpec {
    ActionSpec::Registry(RegistryAction {
        hive: RegistryHive::LocalMachine,
        key: r"SOFTWARE\Test".into(),
        value_name: name.into(),
        value: Some(RegistryValue::Dword(value)),
        delete: false,
    })
}

pub(crate) fn dword(value: u32) -> ResourceState {
    ResourceState::RegistryValue {
        value: RegistryValue::Dword(value),
    }
}

pub(crate) fn manifest(id: &str, actions: Vec<ActionSpec>) -> TweakManifest {
    TweakManifest {
        schema_version: 1,
        id: TweakId::new(id),
        title: "Title".into(),
        description: "Description".into(),
        category: TweakCategory::Performance,
        risk: RiskLevel::Low,
        side_effects: "None".into(),
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

pub(crate) fn facts() -> SystemFacts {
    SystemFacts {
        windows_build: 26100,
        windows_ubr: 1,
        windows_display_version: "24H2".into(),
        edition: "Professional".into(),
        cpu: shared_types::CpuInfo {
            brand: "test".into(),
            physical_cores: 4,
            logical_cores: 8,
        },
        gpus: vec![],
        total_memory_bytes: 0,
        is_laptop: false,
        on_battery: false,
        is_virtual_machine: false,
        is_domain_joined: false,
        anti_cheat: vec![],
    }
}
