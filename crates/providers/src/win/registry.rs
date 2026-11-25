//! `RegistryProvider`: typed registry values via `RegOpenKeyExW`,
//! `RegQueryValueExW`, `RegCreateKeyExW`, `RegSetValueExW` and
//! `RegDeleteKeyValueW`, always in the 64-bit registry view.

use shared_types::{
    ActionSpec, AppError, AppResult, ProviderKind, RegistryAction, RegistryHive, ResourceState,
    StateSnapshot, SystemProvider, VerifyOutcome,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, KEY_SET_VALUE, KEY_WOW64_64KEY,
    REG_OPTION_NON_VOLATILE, REG_SAM_FLAGS, REG_VALUE_TYPE, RegCloseKey, RegCreateKeyExW,
    RegDeleteKeyValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows::core::PCWSTR;

use super::{Wide, win32_result};
use crate::contract::{ensure_kind, snapshot_of, verify_from_snapshot};
use crate::errors::{
    ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_PATH_NOT_FOUND, check_snapshot, from_win32,
    internal,
};
use crate::paths::normalize_registry_key;
use crate::reg_codec::{decode, encode};

const KIND: ProviderKind = ProviderKind::Registry;
const VIEW_64: REG_SAM_FLAGS = KEY_WOW64_64KEY;

/// Reads and writes registry values (all [`shared_types::RegistryValue`]
/// variants) under `HKLM` / `HKCU`.
#[derive(Debug, Default, Clone, Copy)]
pub struct RegistryProvider;

impl RegistryProvider {
    pub fn new() -> Self {
        Self
    }
}

/// Owned registry key handle, closed on drop.
struct RegKey(HKEY);

impl Drop for RegKey {
    fn drop(&mut self) {
        // SAFETY: the handle was returned by a successful RegOpenKeyExW /
        // RegCreateKeyExW, is owned exclusively by this guard and is closed
        // exactly once.
        let _ = unsafe { RegCloseKey(self.0) };
    }
}

fn root(hive: RegistryHive) -> AppResult<HKEY> {
    match hive {
        RegistryHive::LocalMachine => Ok(HKEY_LOCAL_MACHINE),
        RegistryHive::CurrentUser => Ok(HKEY_CURRENT_USER),
        _ => Err(internal("unsupported registry hive")),
    }
}

fn action_of(action: &ActionSpec) -> AppResult<&RegistryAction> {
    ensure_kind(KIND, action)?;
    match action {
        ActionSpec::Registry(a) => Ok(a),
        other => Err(crate::errors::mismatch(KIND, other)),
    }
}

/// Opens an existing key; `Err(code)` carries the raw Win32 status.
fn open_key(root: HKEY, subkey: &Wide, access: REG_SAM_FLAGS) -> Result<RegKey, u32> {
    let mut handle = HKEY::default();
    // SAFETY: `subkey` outlives the call and is NUL-terminated; `handle` is a
    // valid out-pointer.
    let rc = unsafe { RegOpenKeyExW(root, subkey.pcwstr(), None, access | VIEW_64, &mut handle) };
    if rc.0 == 0 {
        Ok(RegKey(handle))
    } else {
        Err(rc.0)
    }
}

/// Raw `(type, bytes)` of a value, or `None` when the value does not exist.
fn query_raw(key: &RegKey, name: &Wide, resource: &str) -> AppResult<Option<(u32, Vec<u8>)>> {
    let mut size: u32 = 0;
    let mut reg_type = REG_VALUE_TYPE(0);
    // SAFETY: size query only: no data buffer is supplied; the out-pointers
    // are valid for the duration of the call.
    let rc = unsafe {
        RegQueryValueExW(
            key.0,
            name.pcwstr(),
            None,
            Some(&mut reg_type),
            None,
            Some(&mut size),
        )
    };
    match rc.0 {
        0 => {}
        ERROR_FILE_NOT_FOUND => return Ok(None),
        other => return Err(from_win32(other, resource)),
    }
    // The value may grow between the size query and the read: retry.
    for _ in 0..8 {
        let mut buf = vec![0u8; size as usize];
        let mut capacity = size;
        // SAFETY: `buf` is valid for `capacity` bytes and outlives the call;
        // the out-pointers are valid.
        let rc = unsafe {
            RegQueryValueExW(
                key.0,
                name.pcwstr(),
                None,
                Some(&mut reg_type),
                Some(buf.as_mut_ptr()),
                Some(&mut capacity),
            )
        };
        match rc.0 {
            0 => {
                buf.truncate(capacity as usize);
                return Ok(Some((reg_type.0, buf)));
            }
            ERROR_MORE_DATA => size = capacity.max(size.saturating_add(16)),
            ERROR_FILE_NOT_FOUND => return Ok(None),
            other => return Err(from_win32(other, resource)),
        }
    }
    Err(internal(format!(
        "registry value {resource} keeps changing size"
    )))
}

fn read_state(a: &RegistryAction, resource: &str) -> AppResult<Option<ResourceState>> {
    let subkey = Wide::new(normalize_registry_key(&a.key))?;
    let name = Wide::new(&a.value_name)?;
    let key = match open_key(root(a.hive)?, &subkey, KEY_QUERY_VALUE) {
        Ok(k) => k,
        Err(ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND) => return Ok(None),
        Err(code) => return Err(from_win32(code, resource)),
    };
    match query_raw(&key, &name, resource)? {
        None => Ok(None),
        Some((reg_type, data)) => Ok(Some(ResourceState::RegistryValue {
            value: decode(reg_type, &data)?,
        })),
    }
}

/// Deletes the value; a missing key or value is success (idempotent).
fn delete_value(a: &RegistryAction, resource: &str) -> AppResult<()> {
    let subkey = Wide::new(normalize_registry_key(&a.key))?;
    let name = Wide::new(&a.value_name)?;
    let key = match open_key(root(a.hive)?, &subkey, KEY_SET_VALUE) {
        Ok(k) => k,
        Err(ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND) => return Ok(()),
        Err(code) => return Err(from_win32(code, resource)),
    };
    // SAFETY: `key` is a valid open handle (64-bit view); a null sub-key
    // addresses the key itself; `name` is NUL-terminated and outlives the call.
    let rc = unsafe { RegDeleteKeyValueW(key.0, PCWSTR::null(), name.pcwstr()) };
    match rc.0 {
        ERROR_FILE_NOT_FOUND => Ok(()),
        other => win32_result(other, resource),
    }
}

fn write_value(
    a: &RegistryAction,
    value: &shared_types::RegistryValue,
    resource: &str,
) -> AppResult<()> {
    // Encode first so an unrepresentable value fails before touching the key.
    let (reg_type, bytes) = encode(value)?;
    let subkey = Wide::new(normalize_registry_key(&a.key))?;
    let name = Wide::new(&a.value_name)?;
    let hive_root = root(a.hive)?;
    let mut handle = HKEY::default();
    // SAFETY: `subkey` is NUL-terminated and outlives the call; `handle` is a
    // valid out-pointer; optional pointers are None.
    let rc = unsafe {
        RegCreateKeyExW(
            hive_root,
            subkey.pcwstr(),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE | VIEW_64,
            None,
            &mut handle,
            None,
        )
    };
    win32_result(rc.0, resource)?;
    let key = RegKey(handle);
    // SAFETY: `key` is a valid handle opened with KEY_SET_VALUE; `name` and
    // `bytes` outlive the call.
    let rc = unsafe {
        RegSetValueExW(
            key.0,
            name.pcwstr(),
            None,
            REG_VALUE_TYPE(reg_type),
            Some(bytes.as_slice()),
        )
    };
    win32_result(rc.0, resource)
}

impl SystemProvider for RegistryProvider {
    fn kind(&self) -> ProviderKind {
        KIND
    }

    fn read(&self, action: &ActionSpec) -> AppResult<StateSnapshot> {
        let a = action_of(action)?;
        let state = read_state(a, action.resource_id().as_str())?;
        Ok(snapshot_of(action, KIND, state))
    }

    fn apply(&self, action: &ActionSpec) -> AppResult<()> {
        let a = action_of(action)?;
        let resource = action.resource_id();
        if a.delete {
            return delete_value(a, resource.as_str());
        }
        match &a.value {
            Some(value) => write_value(a, value, resource.as_str()),
            None => Err(AppError::Internal {
                message: format!("registry action {resource} has neither value nor delete"),
            }),
        }
    }

    fn revert(&self, action: &ActionSpec, snapshot: &StateSnapshot) -> AppResult<()> {
        let a = action_of(action)?;
        check_snapshot(KIND, snapshot)?;
        let resource = action.resource_id();
        match &snapshot.state {
            // The value did not exist before: delete it, never reset it.
            None => delete_value(a, resource.as_str()),
            Some(ResourceState::RegistryValue { value }) => {
                write_value(a, value, resource.as_str())
            }
            Some(_) => Err(internal("snapshot state is not a registry value")),
        }
    }

    fn verify(&self, action: &ActionSpec) -> AppResult<VerifyOutcome> {
        let live = self.read(action)?;
        Ok(verify_from_snapshot(action, &live))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared_types::RegistryValue;
    use std::time::{SystemTime, UNIX_EPOCH};
    use windows::Win32::System::Registry::RegDeleteTreeW;

    /// Unique sandbox key under HKCU; removed (with all contents) on drop.
    struct Sandbox {
        key: String,
    }

    impl Sandbox {
        fn new() -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default();
            Self {
                key: format!(r"Software\WinOptSuite\Test\{}-{nanos}", std::process::id()),
            }
        }

        fn action(&self, name: &str, value: Option<RegistryValue>) -> ActionSpec {
            ActionSpec::Registry(RegistryAction {
                hive: RegistryHive::CurrentUser,
                key: self.key.clone(),
                value_name: name.into(),
                delete: value.is_none(),
                value,
            })
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            if let Ok(sub) = Wide::new(&self.key) {
                // SAFETY: HKCU is a predefined key; `sub` is NUL-terminated
                // and outlives the call.
                let _ = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, sub.pcwstr()) };
            }
        }
    }

    #[test]
    fn absent_apply_verify_revert_deletes() {
        let sb = Sandbox::new();
        let p = RegistryProvider::new();
        let act = sb.action("Val", Some(RegistryValue::Dword(7)));

        let before = p.read(&act).unwrap();
        assert!(!before.existed);
        assert!(before.is_consistent());

        p.apply(&act).unwrap();
        assert_eq!(p.verify(&act).unwrap(), VerifyOutcome::Matches);
        // idempotent
        p.apply(&act).unwrap();

        p.revert(&act, &before).unwrap();
        assert!(!p.read(&act).unwrap().existed);
        // reverting again is a no-op success
        p.revert(&act, &before).unwrap();
    }

    #[test]
    fn existing_value_is_restored_exactly() {
        let sb = Sandbox::new();
        let p = RegistryProvider::new();
        let original = RegistryValue::MultiString(vec!["one".into(), "two".into()]);
        p.apply(&sb.action("Val", Some(original.clone()))).unwrap();

        let act = sb.action("Val", Some(RegistryValue::Qword(99)));
        let before = p.read(&act).unwrap();
        assert!(before.existed);
        assert_eq!(
            before.state,
            Some(ResourceState::RegistryValue {
                value: original.clone()
            })
        );

        p.apply(&act).unwrap();
        assert_eq!(p.verify(&act).unwrap(), VerifyOutcome::Matches);

        p.revert(&act, &before).unwrap();
        assert_eq!(p.read(&act).unwrap(), before);
    }

    #[test]
    fn every_value_type_roundtrips() {
        let sb = Sandbox::new();
        let p = RegistryProvider::new();
        let values = [
            RegistryValue::Dword(0xDEAD_BEEF),
            RegistryValue::Qword(u64::MAX),
            RegistryValue::String("héllo".into()),
            RegistryValue::ExpandString("%SystemRoot%\\x".into()),
            RegistryValue::MultiString(vec!["a".into(), "b".into()]),
            RegistryValue::Binary(vec![0, 1, 2, 255]),
        ];
        for (i, v) in values.into_iter().enumerate() {
            let act = sb.action(&format!("V{i}"), Some(v));
            p.apply(&act).unwrap();
            assert_eq!(p.verify(&act).unwrap(), VerifyOutcome::Matches, "value {i}");
        }
    }

    #[test]
    fn delete_action_removes_value() {
        let sb = Sandbox::new();
        let p = RegistryProvider::new();
        p.apply(&sb.action("Val", Some(RegistryValue::Dword(1))))
            .unwrap();
        let del = sb.action("Val", None);
        let before = p.read(&del).unwrap();
        assert!(before.existed);
        p.apply(&del).unwrap();
        assert_eq!(p.verify(&del).unwrap(), VerifyOutcome::Matches);
        p.revert(&del, &before).unwrap();
        assert_eq!(p.read(&del).unwrap(), before);
    }

    #[test]
    fn wrong_action_kind_is_rejected() {
        let p = RegistryProvider::new();
        let other = ActionSpec::Service(shared_types::ServiceAction {
            name: "x".into(),
            start_type: shared_types::ServiceStartType::Manual,
        });
        assert!(matches!(
            p.read(&other),
            Err(AppError::ActionProviderMismatch { .. })
        ));
    }
}
