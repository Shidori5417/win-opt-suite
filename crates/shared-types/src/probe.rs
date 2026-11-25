//! Read-only probe port (ADR-0002). `system-probe` holds the detection logic
//! and talks to the OS only through [`ProbeSource`]; the Windows
//! implementation lives in `providers` (the only crate allowed `unsafe`).
//! Implementations MUST NOT modify the system.

use serde::{Deserialize, Serialize};

use crate::error::AppResult;
use crate::system::{CpuInfo, GpuInfo};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OsVersionInfo {
    pub build: u32,
    pub ubr: u32,
    /// e.g. `24H2`.
    pub display_version: String,
    /// e.g. `Professional`.
    pub edition: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryStatus {
    pub total_bytes: u64,
    pub available_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PowerStatus {
    pub has_battery: bool,
    pub on_ac_power: bool,
}

/// Raw, read-only OS queries. Each method is independent so a failure in
/// one query degrades only the affected fact.
pub trait ProbeSource: Send + Sync {
    fn os_version(&self) -> AppResult<OsVersionInfo>;
    fn cpu(&self) -> AppResult<CpuInfo>;
    fn gpus(&self) -> AppResult<Vec<GpuInfo>>;
    fn memory(&self) -> AppResult<MemoryStatus>;
    fn power(&self) -> AppResult<PowerStatus>;
    /// SMBIOS system-enclosure chassis type codes (e.g. 9/10/14/31/32 = portable).
    fn chassis_types(&self) -> AppResult<Vec<u16>>;
    /// Hypervisor-present / known virtual hardware indicator.
    fn is_virtual_machine(&self) -> AppResult<bool>;
    fn is_domain_joined(&self) -> AppResult<bool>;
    /// File names of loaded kernel drivers, e.g. `vgk.sys`.
    fn loaded_driver_names(&self) -> AppResult<Vec<String>>;
    /// Key names of installed services and drivers, e.g. `EasyAntiCheat_EOS`.
    fn installed_service_names(&self) -> AppResult<Vec<String>>;
    fn running_service_count(&self) -> AppResult<u32>;
    fn process_count(&self) -> AppResult<u32>;
}
