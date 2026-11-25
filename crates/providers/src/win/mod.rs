//! Windows-only adapters. Everything here talks to native Win32/COM APIs via
//! `windows-rs`; nothing spawns external processes.
//!
//! NOTE: this module tree could not be compiled on the Linux development
//! host; it is verified by Windows CI.

mod com;
mod firewall;
mod power;
mod registry;
mod service;
mod task;

pub use firewall::FirewallProvider;
pub use power::PowerPlanProvider;
pub use registry::RegistryProvider;
pub use service::ServiceProvider;
pub use task::ScheduledTaskProvider;

use shared_types::{AppError, AppResult};
use windows::core::{Error, PCWSTR};

use crate::errors;
use crate::paths::to_wide_nul;

/// Maps a `windows::core::Error` (HRESULT) to a typed [`AppError`].
pub(crate) fn map_error(err: &Error, resource: &str) -> AppError {
    errors::from_hresult(err.code().0, resource, &err.message())
}

/// Maps a raw Win32 status code (`0` = success) to a result.
pub(crate) fn win32_result(code: u32, resource: &str) -> AppResult<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(errors::from_win32(code, resource))
    }
}

/// Owned NUL-terminated UTF-16 string handed to Win32 as a `PCWSTR`.
pub(crate) struct Wide(Vec<u16>);

impl Wide {
    pub(crate) fn new(value: &str) -> AppResult<Self> {
        Ok(Self(to_wide_nul(value)?))
    }

    /// Borrowed pointer; valid for as long as `self` lives.
    pub(crate) fn pcwstr(&self) -> PCWSTR {
        PCWSTR(self.0.as_ptr())
    }
}
