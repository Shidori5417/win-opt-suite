//! RAII COM apartment guard.

use shared_types::{AppError, AppResult};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};

use crate::errors::{ComInit, classify_com_init};

/// Initialises COM for the current thread for the lifetime of the guard.
///
/// * `S_OK` / `S_FALSE` (already initialised in the same mode): the call is
///   reference counted, so the guard balances it with `CoUninitialize`.
/// * `RPC_E_CHANGED_MODE` (thread already initialised with another apartment
///   model): COM remains usable; the guard does nothing on drop.
///
/// Declare the guard **before** any COM interface pointer in the same scope:
/// locals drop in reverse order, so the interfaces are released first.
pub(crate) struct ComGuard {
    uninitialize_on_drop: bool,
    /// COM init is per-thread: the guard must not cross threads.
    _not_send: std::marker::PhantomData<*const ()>,
}

impl ComGuard {
    pub(crate) fn init() -> AppResult<Self> {
        // SAFETY: a null reserved pointer is required by the API; the call has
        // no other preconditions and is balanced in `Drop` when it succeeded.
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        match classify_com_init(hr.0) {
            ComInit::Owned => Ok(Self {
                uninitialize_on_drop: true,
                _not_send: std::marker::PhantomData,
            }),
            ComInit::Borrowed => Ok(Self {
                uninitialize_on_drop: false,
                _not_send: std::marker::PhantomData,
            }),
            ComInit::Failed => Err(AppError::Os {
                os_code: hr.0,
                message: hr.message(),
            }),
        }
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.uninitialize_on_drop {
            // SAFETY: matches the successful `CoInitializeEx` in `init` on
            // this same thread (the guard is `!Send`, so it cannot leave it).
            unsafe { CoUninitialize() };
        }
    }
}
