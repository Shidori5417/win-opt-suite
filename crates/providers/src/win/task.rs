//! `ScheduledTaskProvider`: enable/disable tasks through the Task Scheduler
//! COM API (`ITaskService` -> `ITaskFolder` -> `IRegisteredTask`).

use shared_types::{
    ActionSpec, AppError, AppResult, ProviderKind, ResourceState, ScheduledTaskAction,
    StateSnapshot, SystemProvider, VerifyOutcome,
};
use windows::Win32::Foundation::{VARIANT_BOOL, VARIANT_FALSE, VARIANT_TRUE};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::System::TaskScheduler::{IRegisteredTask, ITaskService, TaskScheduler};
use windows::Win32::System::Variant::VARIANT;
use windows::core::BSTR;

use super::com::ComGuard;
use super::map_error;
use crate::contract::{ensure_kind, snapshot_of, verify_from_snapshot};
use crate::errors::{check_snapshot, is_not_found, mismatch};
use crate::paths::split_task_path;

const KIND: ProviderKind = ProviderKind::ScheduledTask;

/// Reads and toggles the `Enabled` flag of scheduled tasks.
#[derive(Debug, Default, Clone, Copy)]
pub struct ScheduledTaskProvider;

impl ScheduledTaskProvider {
    pub fn new() -> Self {
        Self
    }
}

fn action_of(action: &ActionSpec) -> AppResult<&ScheduledTaskAction> {
    ensure_kind(KIND, action)?;
    match action {
        ActionSpec::ScheduledTask(a) => Ok(a),
        other => Err(mismatch(KIND, other)),
    }
}

/// Resolves the task (or `None` when the folder/task does not exist) and
/// runs `f` while COM is initialised and the interfaces are alive.
fn with_task<T>(
    path: &str,
    resource: &str,
    f: impl FnOnce(Option<&IRegisteredTask>) -> AppResult<T>,
) -> AppResult<T> {
    let (folder_path, task_name) = split_task_path(path)?;
    // Declared first => dropped last: interfaces are released before
    // CoUninitialize.
    let _com = ComGuard::init()?;

    // SAFETY: COM is initialised on this thread (guard above); the CLSID is a
    // valid Task Scheduler class and the requested interface matches it.
    let created: windows::core::Result<ITaskService> =
        unsafe { CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER) };
    let service = created.map_err(|e| map_error(&e, resource))?;

    let empty = VARIANT::default();
    // SAFETY: `service` is a live interface; empty VARIANTs mean the local
    // machine and the current user.
    let connected = unsafe { service.Connect(&empty, &empty, &empty, &empty) };
    connected.map_err(|e| map_error(&e, resource))?;

    // SAFETY: `service` is connected; the BSTR outlives the call.
    let folder = unsafe { service.GetFolder(&BSTR::from(folder_path.as_str())) };
    let folder = match folder {
        Ok(folder) => folder,
        Err(e) if is_not_found(&map_error(&e, resource)) => return f(None),
        Err(e) => return Err(map_error(&e, resource)),
    };

    // SAFETY: `folder` is a live interface; the BSTR outlives the call.
    let task = unsafe { folder.GetTask(&BSTR::from(task_name.as_str())) };
    match task {
        Ok(task) => f(Some(&task)),
        Err(e) if is_not_found(&map_error(&e, resource)) => f(None),
        Err(e) => Err(map_error(&e, resource)),
    }
}

fn read_enabled(path: &str, resource: &str) -> AppResult<Option<bool>> {
    with_task(path, resource, |task| {
        let Some(task) = task else { return Ok(None) };
        // SAFETY: `task` is a live IRegisteredTask.
        let enabled = unsafe { task.Enabled() };
        enabled
            .map(|v| Some(v.0 != 0))
            .map_err(|e| map_error(&e, resource))
    })
}

fn set_enabled(path: &str, enabled: bool, resource: &str) -> AppResult<()> {
    with_task(path, resource, |task| {
        let task = task.ok_or_else(|| AppError::NotFound {
            resource: resource.to_owned(),
        })?;
        let flag: VARIANT_BOOL = if enabled { VARIANT_TRUE } else { VARIANT_FALSE };
        // SAFETY: `task` is a live IRegisteredTask.
        let res = unsafe { task.SetEnabled(flag) };
        res.map_err(|e| map_error(&e, resource))
    })
}

impl SystemProvider for ScheduledTaskProvider {
    fn kind(&self) -> ProviderKind {
        KIND
    }

    fn read(&self, action: &ActionSpec) -> AppResult<StateSnapshot> {
        let a = action_of(action)?;
        let state = read_enabled(&a.path, action.resource_id().as_str())?
            .map(|enabled| ResourceState::ScheduledTask { enabled });
        Ok(snapshot_of(action, KIND, state))
    }

    fn apply(&self, action: &ActionSpec) -> AppResult<()> {
        let a = action_of(action)?;
        set_enabled(&a.path, a.enabled, action.resource_id().as_str())
    }

    fn revert(&self, action: &ActionSpec, snapshot: &StateSnapshot) -> AppResult<()> {
        let a = action_of(action)?;
        check_snapshot(KIND, snapshot)?;
        match &snapshot.state {
            // Tasks are never created by this provider; nothing to undo.
            None => Ok(()),
            Some(ResourceState::ScheduledTask { enabled }) => {
                set_enabled(&a.path, *enabled, action.resource_id().as_str())
            }
            Some(_) => Err(AppError::Internal {
                message: "snapshot state is not a scheduled-task state".into(),
            }),
        }
    }

    fn verify(&self, action: &ActionSpec) -> AppResult<VerifyOutcome> {
        let live = self.read(action)?;
        Ok(verify_from_snapshot(action, &live))
    }
}
