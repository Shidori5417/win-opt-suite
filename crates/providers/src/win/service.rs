//! `ServiceProvider`: service start type via the Service Control Manager
//! (`OpenSCManagerW`, `OpenServiceW`, `QueryServiceConfigW`,
//! `QueryServiceConfig2W`, `ChangeServiceConfigW`, `ChangeServiceConfig2W`).

use std::ffi::c_void;

use shared_types::{
    ActionSpec, AppError, AppResult, ProviderKind, ResourceState, ServiceAction, ServiceStartType,
    StateSnapshot, SystemProvider, VerifyOutcome,
};
use windows::Win32::System::Services::{
    ChangeServiceConfig2W, ChangeServiceConfigW, CloseServiceHandle, ENUM_SERVICE_TYPE,
    OpenSCManagerW, OpenServiceW, QUERY_SERVICE_CONFIGW, QueryServiceConfig2W, QueryServiceConfigW,
    SC_HANDLE, SC_MANAGER_CONNECT, SERVICE_CHANGE_CONFIG, SERVICE_CONFIG_DELAYED_AUTO_START_INFO,
    SERVICE_DELAYED_AUTO_START_INFO, SERVICE_ERROR, SERVICE_NO_CHANGE, SERVICE_QUERY_CONFIG,
    SERVICE_START_TYPE,
};
use windows::core::{BOOL, PCWSTR};

use super::{Wide, map_error};
use crate::contract::{ensure_kind, snapshot_of, verify_from_snapshot};
use crate::errors::{ERROR_INSUFFICIENT_BUFFER, check_snapshot, internal, is_not_found, mismatch};
use crate::mapping::{is_auto_start, start_type_from_raw, start_type_to_raw};

const KIND: ProviderKind = ProviderKind::Service;

/// Reads and changes the start type (incl. automatic-delayed) of services.
#[derive(Debug, Default, Clone, Copy)]
pub struct ServiceProvider;

impl ServiceProvider {
    pub fn new() -> Self {
        Self
    }
}

/// Owned SCM / service handle, closed on drop.
struct ScHandle(SC_HANDLE);

impl Drop for ScHandle {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful OpenSCManagerW /
        // OpenServiceW, is owned by this guard and closed exactly once.
        let _ = unsafe { CloseServiceHandle(self.0) };
    }
}

fn action_of(action: &ActionSpec) -> AppResult<&ServiceAction> {
    ensure_kind(KIND, action)?;
    match action {
        ActionSpec::Service(a) => Ok(a),
        other => Err(mismatch(KIND, other)),
    }
}

fn open_scm(resource: &str) -> AppResult<ScHandle> {
    // SAFETY: null machine/database names select the local active database.
    let res = unsafe { OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_CONNECT) };
    res.map(ScHandle).map_err(|e| map_error(&e, resource))
}

fn open_service(scm: &ScHandle, name: &Wide, access: u32, resource: &str) -> AppResult<ScHandle> {
    // SAFETY: `scm` is a valid SCM handle; `name` is NUL-terminated and
    // outlives the call.
    let res = unsafe { OpenServiceW(scm.0, name.pcwstr(), access) };
    res.map(ScHandle).map_err(|e| map_error(&e, resource))
}

/// Raw `dwStartType` of an open service (needs `SERVICE_QUERY_CONFIG`).
fn query_start_type(svc: &ScHandle, resource: &str) -> AppResult<u32> {
    let mut needed: u32 = 0;
    // SAFETY: size query: no buffer is supplied; `needed` is a valid out-pointer.
    let probe = unsafe { QueryServiceConfigW(svc.0, None, 0, &mut needed) };
    match probe {
        Err(e) if e.code().0 as u32 == 0x8007_0000 | ERROR_INSUFFICIENT_BUFFER => {}
        Err(e) => return Err(map_error(&e, resource)),
        Ok(()) => return Err(internal("QueryServiceConfigW reported no data")),
    }
    // u64 elements keep the buffer 8-byte aligned for QUERY_SERVICE_CONFIGW.
    let words = (needed as usize).div_ceil(8).max(1);
    let mut buf = vec![0u64; words];
    let config = buf.as_mut_ptr().cast::<QUERY_SERVICE_CONFIGW>();
    let capacity = (words * 8) as u32;
    // SAFETY: `config` points to `capacity` writable, suitably aligned bytes
    // that outlive the call.
    let res = unsafe { QueryServiceConfigW(svc.0, Some(config), capacity, &mut needed) };
    res.map_err(|e| map_error(&e, resource))?;
    // SAFETY: the successful call above initialised a QUERY_SERVICE_CONFIGW at
    // the start of the buffer, which is still alive and aligned.
    let start = unsafe { (*config).dwStartType };
    Ok(start.0)
}

/// Delayed-auto-start flag (only meaningful for `SERVICE_AUTO_START`).
fn query_delayed(svc: &ScHandle, resource: &str) -> AppResult<bool> {
    let mut buf = [0u8; size_of::<SERVICE_DELAYED_AUTO_START_INFO>()];
    let mut needed: u32 = 0;
    // SAFETY: `buf` is a writable buffer of exactly the info structure's size.
    let res = unsafe {
        QueryServiceConfig2W(
            svc.0,
            SERVICE_CONFIG_DELAYED_AUTO_START_INFO,
            Some(buf.as_mut_slice()),
            &mut needed,
        )
    };
    res.map_err(|e| map_error(&e, resource))?;
    Ok(i32::from_ne_bytes(buf) != 0)
}

fn read_start_type(name: &str, resource: &str) -> AppResult<Option<ServiceStartType>> {
    let wide = Wide::new(name)?;
    let scm = open_scm(resource)?;
    let svc = match open_service(&scm, &wide, SERVICE_QUERY_CONFIG, resource) {
        Ok(s) => s,
        Err(e) if is_not_found(&e) => return Ok(None),
        Err(e) => return Err(e),
    };
    let raw = query_start_type(&svc, resource)?;
    let delayed = if is_auto_start(raw) {
        query_delayed(&svc, resource)?
    } else {
        false
    };
    Ok(Some(start_type_from_raw(raw, delayed)?))
}

fn set_start_type(name: &str, start: ServiceStartType, resource: &str) -> AppResult<()> {
    let (raw, delayed) = start_type_to_raw(start)?;
    let wide = Wide::new(name)?;
    let scm = open_scm(resource)?;
    let svc = open_service(&scm, &wide, SERVICE_CHANGE_CONFIG, resource)?;
    // SAFETY: valid service handle opened with SERVICE_CHANGE_CONFIG; every
    // string is null (= unchanged) and the numeric fields use
    // SERVICE_NO_CHANGE except the start type.
    let res = unsafe {
        ChangeServiceConfigW(
            svc.0,
            ENUM_SERVICE_TYPE(SERVICE_NO_CHANGE),
            SERVICE_START_TYPE(raw),
            SERVICE_ERROR(SERVICE_NO_CHANGE),
            PCWSTR::null(),
            PCWSTR::null(),
            None,
            PCWSTR::null(),
            PCWSTR::null(),
            PCWSTR::null(),
            PCWSTR::null(),
        )
    };
    res.map_err(|e| map_error(&e, resource))?;
    if is_auto_start(raw) {
        // Always written for auto-start so Automatic <-> AutomaticDelayed
        // transitions are exact.
        let info = SERVICE_DELAYED_AUTO_START_INFO {
            fDelayedAutostart: BOOL::from(delayed),
        };
        let ptr = (&raw const info).cast::<c_void>();
        // SAFETY: `ptr` points to a live SERVICE_DELAYED_AUTO_START_INFO that
        // matches the SERVICE_CONFIG_DELAYED_AUTO_START_INFO info level.
        let res = unsafe {
            ChangeServiceConfig2W(svc.0, SERVICE_CONFIG_DELAYED_AUTO_START_INFO, Some(ptr))
        };
        res.map_err(|e| map_error(&e, resource))?;
    }
    Ok(())
}

impl SystemProvider for ServiceProvider {
    fn kind(&self) -> ProviderKind {
        KIND
    }

    fn read(&self, action: &ActionSpec) -> AppResult<StateSnapshot> {
        let a = action_of(action)?;
        let state = read_start_type(&a.name, action.resource_id().as_str())?
            .map(|start_type| ResourceState::Service { start_type });
        Ok(snapshot_of(action, KIND, state))
    }

    fn apply(&self, action: &ActionSpec) -> AppResult<()> {
        let a = action_of(action)?;
        set_start_type(&a.name, a.start_type, action.resource_id().as_str())
    }

    fn revert(&self, action: &ActionSpec, snapshot: &StateSnapshot) -> AppResult<()> {
        let a = action_of(action)?;
        check_snapshot(KIND, snapshot)?;
        match &snapshot.state {
            // A service cannot be "un-created" here: it did not exist before,
            // so there is nothing to restore (idempotent success).
            None => Ok(()),
            Some(ResourceState::Service { start_type }) => {
                set_start_type(&a.name, *start_type, action.resource_id().as_str())
            }
            Some(_) => Err(AppError::Internal {
                message: "snapshot state is not a service state".into(),
            }),
        }
    }

    fn verify(&self, action: &ActionSpec) -> AppResult<VerifyOutcome> {
        let live = self.read(action)?;
        Ok(verify_from_snapshot(action, &live))
    }
}
