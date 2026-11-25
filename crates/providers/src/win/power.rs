//! `PowerPlanProvider`: active power scheme and per-setting AC/DC values via
//! `powrprof` (`PowerGetActiveScheme`, `PowerSetActiveScheme`,
//! `PowerRead{AC,DC}ValueIndex`, `PowerWrite{AC,DC}ValueIndex`).

use std::ptr;

use shared_types::{
    ActionSpec, AppError, AppResult, PowerPlanAction, ProviderKind, ResourceState, StateSnapshot,
    SystemProvider, VerifyOutcome,
};
use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::System::Power::{
    PowerGetActiveScheme, PowerReadACValueIndex, PowerReadDCValueIndex, PowerSetActiveScheme,
    PowerWriteACValueIndex, PowerWriteDCValueIndex,
};
use windows::core::GUID;

use super::win32_result;
use crate::contract::{ensure_kind, snapshot_of, verify_from_snapshot};
use crate::errors::{ERROR_FILE_NOT_FOUND, check_snapshot, from_win32, internal, mismatch};
use crate::guid::{GuidParts, format_guid, parse_guid};

const KIND: ProviderKind = ProviderKind::PowerPlan;

/// Activates power schemes and sets AC/DC setting values.
#[derive(Debug, Default, Clone, Copy)]
pub struct PowerPlanProvider;

impl PowerPlanProvider {
    pub fn new() -> Self {
        Self
    }
}

fn to_guid(p: &GuidParts) -> GUID {
    GUID::from_values(p.data1, p.data2, p.data3, p.data4)
}

fn from_guid(g: &GUID) -> GuidParts {
    GuidParts {
        data1: g.data1,
        data2: g.data2,
        data3: g.data3,
        data4: g.data4,
    }
}

fn parse(value: &str) -> AppResult<GUID> {
    Ok(to_guid(&parse_guid(value)?))
}

/// Frees the buffer `PowerGetActiveScheme` allocated for the scheme GUID.
struct LocalAllocatedGuid(*mut GUID);

impl Drop for LocalAllocatedGuid {
    fn drop(&mut self) {
        // SAFETY: the pointer was allocated with LocalAlloc by
        // PowerGetActiveScheme, is owned by this guard and freed exactly once.
        let _ = unsafe { LocalFree(Some(HLOCAL(self.0.cast()))) };
    }
}

fn active_scheme(resource: &str) -> AppResult<GUID> {
    let mut raw: *mut GUID = ptr::null_mut();
    // SAFETY: `raw` is a valid out-pointer; on success the API stores a
    // LocalAlloc'ed GUID pointer in it.
    let rc = unsafe { PowerGetActiveScheme(None, &mut raw) };
    win32_result(rc.0, resource)?;
    if raw.is_null() {
        return Err(internal("PowerGetActiveScheme returned a null scheme"));
    }
    let owned = LocalAllocatedGuid(raw);
    // SAFETY: `owned.0` is non-null and points to a valid GUID until `owned`
    // is dropped at the end of this function.
    let guid = unsafe { *owned.0 };
    Ok(guid)
}

fn set_active_scheme(scheme: &GUID, resource: &str) -> AppResult<()> {
    // SAFETY: `scheme` is a valid GUID that outlives the call.
    let rc = unsafe { PowerSetActiveScheme(None, Some(ptr::from_ref(scheme))) };
    win32_result(rc.0, resource)
}

fn read_values(
    scheme: &GUID,
    subgroup: &GUID,
    setting: &GUID,
    resource: &str,
) -> AppResult<Option<(u32, u32)>> {
    let mut ac: u32 = 0;
    // SAFETY: all GUID pointers are valid for the call; `ac` is a valid
    // out-pointer.
    let rc = unsafe {
        PowerReadACValueIndex(
            None,
            Some(ptr::from_ref(scheme)),
            Some(ptr::from_ref(subgroup)),
            Some(ptr::from_ref(setting)),
            &mut ac,
        )
    };
    match rc.0 {
        0 => {}
        ERROR_FILE_NOT_FOUND => return Ok(None),
        other => return Err(from_win32(other, resource)),
    }
    let mut dc: u32 = 0;
    // SAFETY: as above, with `dc` as the out-pointer.
    let rc = unsafe {
        PowerReadDCValueIndex(
            None,
            Some(ptr::from_ref(scheme)),
            Some(ptr::from_ref(subgroup)),
            Some(ptr::from_ref(setting)),
            &mut dc,
        )
    };
    match rc {
        0 => Ok(Some((ac, dc))),
        ERROR_FILE_NOT_FOUND => Ok(None),
        other => Err(from_win32(other, resource)),
    }
}

/// Writes both values and re-activates the scheme when it is the active one
/// (required for the change to take effect immediately).
fn write_values(
    scheme: &GUID,
    subgroup: &GUID,
    setting: &GUID,
    ac: u32,
    dc: u32,
    resource: &str,
) -> AppResult<()> {
    // SAFETY: all GUID pointers are valid for the call.
    let rc = unsafe {
        PowerWriteACValueIndex(
            None,
            ptr::from_ref(scheme),
            Some(ptr::from_ref(subgroup)),
            Some(ptr::from_ref(setting)),
            ac,
        )
    };
    win32_result(rc.0, resource)?;
    // SAFETY: all GUID pointers are valid for the call.
    let rc = unsafe {
        PowerWriteDCValueIndex(
            None,
            ptr::from_ref(scheme),
            Some(ptr::from_ref(subgroup)),
            Some(ptr::from_ref(setting)),
            dc,
        )
    };
    win32_result(rc, resource)?;
    if *scheme == active_scheme(resource)? {
        set_active_scheme(scheme, resource)?;
    }
    Ok(())
}

fn scheme_or_active(scheme: Option<&str>, resource: &str) -> AppResult<GUID> {
    match scheme {
        Some(s) => parse(s),
        None => active_scheme(resource),
    }
}

fn read_state(action: &ActionSpec, resource: &str) -> AppResult<Option<ResourceState>> {
    match plan_action(action)? {
        PowerPlanAction::ActivateScheme { .. } => {
            let guid = active_scheme(resource)?;
            Ok(Some(ResourceState::ActivePowerScheme {
                scheme_guid: format_guid(&from_guid(&guid)),
            }))
        }
        PowerPlanAction::SetValue {
            scheme_guid,
            subgroup_guid,
            setting_guid,
            ..
        } => {
            let scheme = scheme_or_active(scheme_guid.as_deref(), resource)?;
            let values = read_values(
                &scheme,
                &parse(subgroup_guid)?,
                &parse(setting_guid)?,
                resource,
            )?;
            Ok(values
                .map(|(ac_value, dc_value)| ResourceState::PowerSetting { ac_value, dc_value }))
        }
        _ => Err(internal("unsupported power plan action")),
    }
}

fn plan_action(action: &ActionSpec) -> AppResult<&PowerPlanAction> {
    ensure_kind(KIND, action)?;
    match action {
        ActionSpec::PowerPlan(a) => Ok(a),
        other => Err(mismatch(KIND, other)),
    }
}

impl SystemProvider for PowerPlanProvider {
    fn kind(&self) -> ProviderKind {
        KIND
    }

    fn read(&self, action: &ActionSpec) -> AppResult<StateSnapshot> {
        let state = read_state(action, action.resource_id().as_str())?;
        Ok(snapshot_of(action, KIND, state))
    }

    fn apply(&self, action: &ActionSpec) -> AppResult<()> {
        let resource = action.resource_id();
        let resource = resource.as_str();
        match plan_action(action)? {
            PowerPlanAction::ActivateScheme { scheme_guid } => {
                set_active_scheme(&parse(scheme_guid)?, resource)
            }
            PowerPlanAction::SetValue {
                scheme_guid,
                subgroup_guid,
                setting_guid,
                ac_value,
                dc_value,
            } => write_values(
                &scheme_or_active(scheme_guid.as_deref(), resource)?,
                &parse(subgroup_guid)?,
                &parse(setting_guid)?,
                *ac_value,
                *dc_value,
                resource,
            ),
            _ => Err(internal("unsupported power plan action")),
        }
    }

    fn revert(&self, action: &ActionSpec, snapshot: &StateSnapshot) -> AppResult<()> {
        check_snapshot(KIND, snapshot)?;
        let resource = action.resource_id();
        let resource = resource.as_str();
        match (plan_action(action)?, &snapshot.state) {
            (
                PowerPlanAction::ActivateScheme { .. },
                Some(ResourceState::ActivePowerScheme { scheme_guid }),
            ) => set_active_scheme(&parse(scheme_guid)?, resource),
            (
                PowerPlanAction::SetValue {
                    scheme_guid,
                    subgroup_guid,
                    setting_guid,
                    ..
                },
                Some(ResourceState::PowerSetting { ac_value, dc_value }),
            ) => write_values(
                &scheme_or_active(scheme_guid.as_deref(), resource)?,
                &parse(subgroup_guid)?,
                &parse(setting_guid)?,
                *ac_value,
                *dc_value,
                resource,
            ),
            // The setting had no explicit value in the scheme. Windows offers
            // no API to remove a single stored value index, so there is
            // nothing further to undo.
            (PowerPlanAction::SetValue { .. }, None) => Ok(()),
            _ => Err(AppError::Internal {
                message: "snapshot state does not match the power plan action".into(),
            }),
        }
    }

    fn verify(&self, action: &ActionSpec) -> AppResult<VerifyOutcome> {
        let live = self.read(action)?;
        Ok(verify_from_snapshot(action, &live))
    }
}
