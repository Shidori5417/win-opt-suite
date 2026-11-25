//! `WindowsProbeSource`: read-only [`ProbeSource`] implementation using native
//! Win32, DXGI, ToolHelp, NetAPI, and CPUID queries.
//!
//! NOTE: this module is gated by `#[cfg(windows)]` and verified by Windows CI.

use std::ffi::c_void;
use std::mem::{offset_of, size_of};
use std::ptr;

use shared_types::{
    AppError, AppResult, CpuInfo, GpuInfo, MemoryStatus, OsVersionInfo, PowerStatus, ProbeSource,
};
use windows::Win32::Foundation::{CloseHandle, ERROR_NO_MORE_FILES, HANDLE, MAX_PATH};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, DXGI_ERROR_NOT_FOUND, IDXGIFactory1,
};
use windows::Win32::NetworkManagement::NetManagement::{
    NERR_Success, NETSETUP_JOIN_STATUS, NetApiBufferFree, NetGetJoinInformation, NetSetupDomainName,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::ProcessStatus::{EnumDeviceDrivers, GetDeviceDriverBaseNameW};
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, REG_ROUTINE_FLAGS, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
    RRF_SUBKEY_WOW6464KEY, RegGetValueW,
};
use windows::Win32::System::Services::{
    CloseServiceHandle, ENUM_SERVICE_STATE, ENUM_SERVICE_STATUS_PROCESSW, ENUM_SERVICE_TYPE,
    EnumServicesStatusExW, OpenSCManagerW, SC_ENUM_PROCESS_INFO, SC_HANDLE,
    SC_MANAGER_ENUMERATE_SERVICE, SERVICE_ACTIVE, SERVICE_DRIVER, SERVICE_STATE_ALL, SERVICE_WIN32,
};
use windows::Win32::System::SystemInformation::{
    GROUP_AFFINITY, GetLogicalProcessorInformationEx, GetSystemFirmwareTable, GlobalMemoryStatusEx,
    MEMORYSTATUSEX, PROCESSOR_RELATIONSHIP, RSMB, RelationProcessorCore,
};
use windows::core::{Error as WinError, PCWSTR, PWSTR};

use super::{Wide, map_error};
use crate::errors::{
    ERROR_FILE_NOT_FOUND, ERROR_INSUFFICIENT_BUFFER, ERROR_MORE_DATA, ERROR_PATH_NOT_FOUND,
    from_win32, internal,
};
use crate::smbios::{is_virtual_hardware, parse_smbios_chassis};

const BIOS_SUBKEY: &str = r"HARDWARE\DESCRIPTION\System\BIOS";

const CURRENT_VERSION_SUBKEY: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
const CPU_SUBKEY: &str = r"HARDWARE\DESCRIPTION\System\CentralProcessor\0";

/// Read-only Windows implementation of [`ProbeSource`].
#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsProbeSource;

impl WindowsProbeSource {
    pub fn new() -> Self {
        Self
    }
}

/// Owned Win32 `HANDLE` closed via `CloseHandle` on drop.
struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            // SAFETY: `self.0` was returned by `CreateToolhelp32Snapshot`, is
            // owned exclusively by this guard, and is closed once.
            let _ = unsafe { CloseHandle(self.0) };
        }
    }
}

/// Owned SCM handle closed via `CloseServiceHandle` on drop.
struct ScHandle(SC_HANDLE);

impl Drop for ScHandle {
    fn drop(&mut self) {
        // SAFETY: `self.0` came from a successful `OpenSCManagerW`, is owned
        // exclusively by this guard, and is closed once.
        let _ = unsafe { CloseServiceHandle(self.0) };
    }
}

/// Owned NetAPI buffer freed via `NetApiBufferFree` on drop.
struct NetApiBuffer(PWSTR);

impl Drop for NetApiBuffer {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: `self.0` was allocated by `NetGetJoinInformation` and is
            // freed once with `NetApiBufferFree`.
            let _ = unsafe { NetApiBufferFree(Some(self.0.0.cast())) };
        }
    }
}

fn is_win32_hresult(err: &WinError, win32_code: u32) -> bool {
    (err.code().0 as u32) == (0x8007_0000 | win32_code)
}

fn reg_get_sz(root: HKEY, subkey: &str, value_name: &str) -> AppResult<Option<String>> {
    let resource = format!("registry:{subkey}\\{value_name}");
    let subkey_w = Wide::new(subkey)?;
    let name_w = Wide::new(value_name)?;
    let flags: REG_ROUTINE_FLAGS = RRF_RT_REG_SZ | RRF_SUBKEY_WOW6464KEY;

    let mut bytes: u32 = 0;
    // SAFETY: size query only (`pvdata` is `None`); `subkey_w` and `name_w`
    // are NUL-terminated and outlive the call.
    let rc = unsafe {
        RegGetValueW(
            root,
            subkey_w.pcwstr(),
            name_w.pcwstr(),
            flags,
            None,
            None,
            Some(&mut bytes),
        )
    };
    match rc.0 {
        0 => {}
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => return Ok(None),
        other => return Err(from_win32(other, &resource)),
    }

    for _ in 0..8 {
        let word_len = (bytes as usize).div_ceil(2).max(1);
        let mut words = vec![0u16; word_len];
        let mut capacity = (words.len() * 2) as u32;
        // SAFETY: `words` provides `capacity` writable bytes and outlives the
        // call; `subkey_w` and `name_w` are NUL-terminated.
        let rc = unsafe {
            RegGetValueW(
                root,
                subkey_w.pcwstr(),
                name_w.pcwstr(),
                flags,
                None,
                Some(words.as_mut_ptr().cast()),
                Some(&mut capacity),
            )
        };
        match rc.0 {
            0 => {
                let unit_len = (capacity as usize) / 2;
                let valid = words.get(..unit_len).unwrap_or(&[]);
                let end = valid.iter().position(|&u| u == 0).unwrap_or(valid.len());
                let text = String::from_utf16(valid.get(..end).unwrap_or(&[]))
                    .map_err(|_| internal(format!("invalid UTF-16 registry value `{resource}`")))?;
                return Ok(Some(text));
            }
            ERROR_MORE_DATA => bytes = capacity.max(bytes.saturating_add(16)),
            ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => return Ok(None),
            other => return Err(from_win32(other, &resource)),
        }
    }

    Err(internal(format!(
        "registry value `{resource}` kept changing size"
    )))
}

fn reg_get_dword(root: HKEY, subkey: &str, value_name: &str) -> AppResult<Option<u32>> {
    let resource = format!("registry:{subkey}\\{value_name}");
    let subkey_w = Wide::new(subkey)?;
    let name_w = Wide::new(value_name)?;
    let flags: REG_ROUTINE_FLAGS = RRF_RT_REG_DWORD | RRF_SUBKEY_WOW6464KEY;

    let mut value: u32 = 0;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: `value` is a valid 4-byte writable buffer; `subkey_w` and
    // `name_w` are NUL-terminated and outlive the call.
    let rc = unsafe {
        RegGetValueW(
            root,
            subkey_w.pcwstr(),
            name_w.pcwstr(),
            flags,
            None,
            Some(ptr::from_mut(&mut value).cast()),
            Some(&mut size),
        )
    };
    match rc.0 {
        0 => Ok(Some(value)),
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => Ok(None),
        other => Err(from_win32(other, &resource)),
    }
}

fn cpuid_brand_string() -> Option<String> {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        #[cfg(target_arch = "x86")]
        use core::arch::x86::__cpuid;
        #[cfg(target_arch = "x86_64")]
        use core::arch::x86_64::__cpuid;

        if __cpuid(0x8000_0000).eax < 0x8000_0004 {
            return None;
        }
        let mut raw = [0u8; 48];
        for (idx, leaf) in [0x8000_0002_u32, 0x8000_0003, 0x8000_0004]
            .into_iter()
            .enumerate()
        {
            let res = __cpuid(leaf);
            let base_off = idx * 16;
            for (word_idx, word) in [res.eax, res.ebx, res.ecx, res.edx].into_iter().enumerate() {
                let byte_off = base_off + word_idx * 4;
                if let Some(slot) = raw.get_mut(byte_off..byte_off + 4) {
                    slot.copy_from_slice(&word.to_le_bytes());
                }
            }
        }
        let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
        let brand = String::from_utf8_lossy(raw.get(..end).unwrap_or(&[]))
            .trim()
            .to_owned();
        if brand.is_empty() { None } else { Some(brand) }
    }
    #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
    {
        None
    }
}

fn cpu_core_counts() -> AppResult<(u32, u32)> {
    const RESOURCE: &str = "cpu:logical_processor_info";
    let mut len: u32 = 0;
    // SAFETY: size query only (`buffer` is `None`); `len` is a valid
    // out-pointer.
    let probe = unsafe { GetLogicalProcessorInformationEx(RelationProcessorCore, None, &mut len) };
    match probe {
        Err(e) if is_win32_hresult(&e, ERROR_INSUFFICIENT_BUFFER) => {}
        Err(e) => return Err(map_error(&e, RESOURCE)),
        Ok(()) => return Ok((0, 0)),
    }

    let mut buf = vec![0u64; (len as usize).div_ceil(8).max(1)];
    for _ in 0..8 {
        let mut capacity = (buf.len() * 8) as u32;
        // SAFETY: `buf` is 8-byte aligned with `capacity` writable bytes and
        // outlives the call.
        let res = unsafe {
            GetLogicalProcessorInformationEx(
                RelationProcessorCore,
                Some(buf.as_mut_ptr().cast()),
                &mut capacity,
            )
        };
        match res {
            Ok(()) => {
                return parse_processor_core_records(&buf, capacity as usize);
            }
            Err(e) if is_win32_hresult(&e, ERROR_INSUFFICIENT_BUFFER) => {
                let needed_words = (capacity as usize).div_ceil(8).max(buf.len() * 2);
                buf.resize(needed_words, 0);
            }
            Err(e) => return Err(map_error(&e, RESOURCE)),
        }
    }

    Err(internal("GetLogicalProcessorInformationEx kept growing"))
}

fn parse_processor_core_records(buf: &[u64], total_bytes: usize) -> AppResult<(u32, u32)> {
    const HEADER_SIZE: usize = 8;
    let total = total_bytes.min(buf.len() * 8);
    let base = buf.as_ptr().cast::<u8>();
    let mut offset: usize = 0;
    let mut physical_cores: u32 = 0;
    let mut logical_cores: u32 = 0;

    while let Some(header_end) = offset.checked_add(HEADER_SIZE) {
        if header_end > total {
            break;
        }
        let rel_ptr = base.wrapping_add(offset).cast::<i32>();
        // SAFETY: `offset + HEADER_SIZE <= total` within live `buf`.
        let relationship = unsafe { ptr::read_unaligned(rel_ptr) };

        let size_ptr = base.wrapping_add(offset + 4).cast::<u32>();
        // SAFETY: `offset + HEADER_SIZE <= total` within live `buf`.
        let record_size = unsafe { ptr::read_unaligned(size_ptr) } as usize;
        if record_size < HEADER_SIZE {
            return Err(internal(
                "invalid SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX size",
            ));
        }
        let Some(record_end) = offset.checked_add(record_size) else {
            break;
        };
        if record_end > total {
            break;
        }

        if relationship == RelationProcessorCore.0
            && let Some(proc_end) = header_end.checked_add(size_of::<PROCESSOR_RELATIONSHIP>())
            && proc_end <= record_end
        {
            let proc_ptr = base
                .wrapping_add(header_end)
                .cast::<PROCESSOR_RELATIONSHIP>();
            // SAFETY: `header_end + size_of::<PROCESSOR_RELATIONSHIP>() <= record_end <= total`
            // within live `buf`.
            let proc_rel = unsafe { ptr::read_unaligned(proc_ptr) };
            physical_cores = physical_cores.saturating_add(1);

            let masks_start =
                header_end.saturating_add(offset_of!(PROCESSOR_RELATIONSHIP, GroupMask));
            for g in 0..usize::from(proc_rel.GroupCount) {
                let Some(group_offset) = g
                    .checked_mul(size_of::<GROUP_AFFINITY>())
                    .and_then(|d| masks_start.checked_add(d))
                else {
                    break;
                };
                let Some(group_end) = group_offset.checked_add(size_of::<GROUP_AFFINITY>()) else {
                    break;
                };
                if group_end > record_end {
                    break;
                }
                let group_ptr = base.wrapping_add(group_offset).cast::<GROUP_AFFINITY>();
                // SAFETY: `group_end <= record_end <= total` within live `buf`.
                let group = unsafe { ptr::read_unaligned(group_ptr) };
                logical_cores = logical_cores.saturating_add(group.Mask.count_ones());
            }
        }

        offset = record_end;
    }

    Ok((physical_cores, logical_cores))
}

fn open_scm_enumerate(resource: &str) -> AppResult<ScHandle> {
    // SAFETY: null machine/database names select the local active SCM database.
    let handle =
        unsafe { OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_ENUMERATE_SERVICE) }
            .map_err(|e| map_error(&e, resource))?;
    Ok(ScHandle(handle))
}

fn enumerate_services<F>(
    service_type: ENUM_SERVICE_TYPE,
    service_state: ENUM_SERVICE_STATE,
    resource: &str,
    mut on_entry: F,
) -> AppResult<()>
where
    F: FnMut(&ENUM_SERVICE_STATUS_PROCESSW) -> AppResult<()>,
{
    let scm = open_scm_enumerate(resource)?;
    // 64 KiB initial aligned buffer avoids an extra failure round-trip on most hosts.
    let mut buf = vec![0u64; 8192];
    let mut resume_handle: u32 = 0;

    for _ in 0..1024 {
        let byte_len = buf.len() * size_of::<u64>();
        let mut bytes_needed: u32 = 0;
        let mut returned: u32 = 0;
        // SAFETY: `buf` is a live `Vec<u64>` allocation of `byte_len` bytes.
        let bytes =
            unsafe { std::slice::from_raw_parts_mut(buf.as_mut_ptr().cast::<u8>(), byte_len) };
        // SAFETY: `scm` is valid, `bytes` is writable for `byte_len` bytes, and
        // all out-pointers outlive the call.
        let res = unsafe {
            EnumServicesStatusExW(
                scm.0,
                SC_ENUM_PROCESS_INFO,
                service_type,
                service_state,
                Some(bytes),
                &mut bytes_needed,
                &mut returned,
                Some(&mut resume_handle),
                PCWSTR::null(),
            )
        };

        let entry_bytes =
            (returned as usize).saturating_mul(size_of::<ENUM_SERVICE_STATUS_PROCESSW>());
        if entry_bytes > byte_len {
            return Err(internal(
                "EnumServicesStatusExW returned count exceeding buffer",
            ));
        }
        if returned > 0 {
            // SAFETY: `EnumServicesStatusExW` initialized `returned` structs at
            // the start of the 8-byte aligned `buf`.
            let entries = unsafe {
                std::slice::from_raw_parts(
                    buf.as_ptr().cast::<ENUM_SERVICE_STATUS_PROCESSW>(),
                    returned as usize,
                )
            };
            for entry in entries {
                on_entry(entry)?;
            }
        }

        match res {
            Ok(()) => return Ok(()),
            Err(e) if is_win32_hresult(&e, ERROR_MORE_DATA) => {
                if returned == 0 && (bytes_needed as usize) > byte_len {
                    buf.resize((bytes_needed as usize).div_ceil(size_of::<u64>()), 0);
                }
            }
            Err(e) => return Err(map_error(&e, resource)),
        }
    }

    Err(internal("EnumServicesStatusExW exceeded pagination limit"))
}

fn is_no_more_files(err: &WinError) -> bool {
    is_win32_hresult(err, ERROR_NO_MORE_FILES.0)
}

impl ProbeSource for WindowsProbeSource {
    fn os_version(&self) -> AppResult<OsVersionInfo> {
        let build_str = reg_get_sz(
            HKEY_LOCAL_MACHINE,
            CURRENT_VERSION_SUBKEY,
            "CurrentBuildNumber",
        )?
        .ok_or_else(|| AppError::NotFound {
            resource: format!("registry:{CURRENT_VERSION_SUBKEY}\\CurrentBuildNumber"),
        })?;
        let build = build_str
            .trim()
            .parse::<u32>()
            .map_err(|e| internal(format!("invalid CurrentBuildNumber `{build_str}`: {e}")))?;

        let ubr = reg_get_dword(HKEY_LOCAL_MACHINE, CURRENT_VERSION_SUBKEY, "UBR")?.unwrap_or(0);
        let display_version =
            match reg_get_sz(HKEY_LOCAL_MACHINE, CURRENT_VERSION_SUBKEY, "DisplayVersion")? {
                Some(v) if !v.is_empty() => v,
                _ => reg_get_sz(HKEY_LOCAL_MACHINE, CURRENT_VERSION_SUBKEY, "ReleaseId")?
                    .unwrap_or_default(),
            };
        let edition = reg_get_sz(HKEY_LOCAL_MACHINE, CURRENT_VERSION_SUBKEY, "EditionID")?
            .unwrap_or_default();

        Ok(OsVersionInfo {
            build,
            ubr,
            display_version,
            edition,
        })
    }

    fn cpu(&self) -> AppResult<CpuInfo> {
        let brand = match cpuid_brand_string() {
            Some(b) => b,
            None => reg_get_sz(HKEY_LOCAL_MACHINE, CPU_SUBKEY, "ProcessorNameString")?
                .map(|s| s.trim().to_owned())
                .unwrap_or_default(),
        };
        let (physical_cores, logical_cores) = cpu_core_counts()?;
        Ok(CpuInfo {
            brand,
            physical_cores,
            logical_cores,
        })
    }

    fn gpus(&self) -> AppResult<Vec<GpuInfo>> {
        const RESOURCE: &str = "dxgi:adapters";
        // SAFETY: `CreateDXGIFactory1` creates a COM-managed `IDXGIFactory1`
        // instance on success.
        let factory: IDXGIFactory1 =
            unsafe { CreateDXGIFactory1() }.map_err(|e| map_error(&e, RESOURCE))?;

        let mut gpus = Vec::new();
        for index in 0u32.. {
            // SAFETY: `factory` is a valid `IDXGIFactory1` interface.
            let adapter = match unsafe { factory.EnumAdapters1(index) } {
                Ok(a) => a,
                Err(e) if e.code() == DXGI_ERROR_NOT_FOUND => break,
                Err(e) => return Err(map_error(&e, RESOURCE)),
            };
            // SAFETY: `adapter` is a valid `IDXGIAdapter1` interface.
            let desc = unsafe { adapter.GetDesc1() }.map_err(|e| map_error(&e, RESOURCE))?;
            if (desc.Flags & (DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32)) != 0 {
                continue;
            }
            let end = desc
                .Description
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(desc.Description.len());
            let name = String::from_utf16_lossy(desc.Description.get(..end).unwrap_or(&[]))
                .trim()
                .to_owned();
            gpus.push(GpuInfo {
                name,
                vendor_id: desc.VendorId,
                dedicated_memory_bytes: desc.DedicatedVideoMemory as u64,
            });
        }
        Ok(gpus)
    }

    fn memory(&self) -> AppResult<MemoryStatus> {
        let mut status = MEMORYSTATUSEX {
            dwLength: size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        // SAFETY: `status.dwLength` is initialized to `size_of::<MEMORYSTATUSEX>()`
        // and `status` outlives the call.
        unsafe { GlobalMemoryStatusEx(&mut status) }.map_err(|e| map_error(&e, "memory"))?;
        Ok(MemoryStatus {
            total_bytes: status.ullTotalPhys,
            available_bytes: status.ullAvailPhys,
        })
    }

    fn power(&self) -> AppResult<PowerStatus> {
        let mut status = SYSTEM_POWER_STATUS::default();
        // SAFETY: `status` is a valid writable `SYSTEM_POWER_STATUS` outliving
        // the call.
        unsafe { GetSystemPowerStatus(&mut status) }.map_err(|e| map_error(&e, "power"))?;
        // BatteryFlag: 128 = no system battery, 255 = unknown status.
        let has_battery = status.BatteryFlag != 128 && status.BatteryFlag != 255;
        // ACLineStatus: 1 = online (AC), 0 = offline (battery), 255 = unknown.
        // Unknown is treated as AC so we never claim "on battery" without proof.
        let on_ac_power = status.ACLineStatus != 0;
        Ok(PowerStatus {
            has_battery,
            on_ac_power,
        })
    }

    fn chassis_types(&self) -> AppResult<Vec<u16>> {
        const RESOURCE: &str = "smbios:rsmb";
        // SAFETY: size query only (`pfirmwaretablebuffer` is `None`); `RSMB`
        // is a valid firmware table provider signature.
        let size = unsafe { GetSystemFirmwareTable(RSMB, 0, None) };
        if size == 0 {
            return Err(map_error(&WinError::from_thread(), RESOURCE));
        }
        let mut buf = vec![0u8; size as usize];
        // SAFETY: `buf` is a writable slice of `size` bytes matching the
        // queried `RSMB` table size.
        let written = unsafe { GetSystemFirmwareTable(RSMB, 0, Some(buf.as_mut_slice())) };
        if written == 0 {
            return Err(map_error(&WinError::from_thread(), RESOURCE));
        }
        buf.truncate(written as usize);
        Ok(parse_smbios_chassis(&buf))
    }

    fn is_virtual_machine(&self) -> AppResult<bool> {
        // The CPUID hypervisor-present bit is NOT used: on Windows 11 with
        // VBS/HVCI (on by default) the host itself runs under Hyper-V and the
        // bit is set on bare metal. Firmware identity strings are reliable.
        let manufacturer =
            reg_get_sz(HKEY_LOCAL_MACHINE, BIOS_SUBKEY, "SystemManufacturer")?.unwrap_or_default();
        let product =
            reg_get_sz(HKEY_LOCAL_MACHINE, BIOS_SUBKEY, "SystemProductName")?.unwrap_or_default();
        Ok(is_virtual_hardware(&manufacturer, &product))
    }

    fn is_domain_joined(&self) -> AppResult<bool> {
        let mut name_buf = PWSTR::null();
        let mut join_status = NETSETUP_JOIN_STATUS::default();
        // SAFETY: `lpserver` is null (local machine), `name_buf` and
        // `join_status` are valid out-pointers, and any returned buffer is
        // immediately owned by `NetApiBuffer`.
        let rc = unsafe { NetGetJoinInformation(PCWSTR::null(), &mut name_buf, &mut join_status) };
        let _guard = NetApiBuffer(name_buf);
        if rc != NERR_Success {
            return Err(from_win32(rc, "domain_join"));
        }
        Ok(join_status == NetSetupDomainName)
    }

    fn loaded_driver_names(&self) -> AppResult<Vec<String>> {
        const RESOURCE: &str = "drivers:enum";
        let mut needed: u32 = 0;
        // SAFETY: size query (`cb == 0`, null base array); `needed` is a valid
        // out-pointer.
        unsafe { EnumDeviceDrivers(ptr::null_mut(), 0, &mut needed) }
            .map_err(|e| map_error(&e, RESOURCE))?;
        if needed == 0 {
            return Ok(Vec::new());
        }

        let ptr_size = size_of::<*mut c_void>();
        let mut bases =
            vec![ptr::null_mut::<c_void>(); (needed as usize).div_ceil(ptr_size).max(1)];
        let mut actual_count = None;
        for _ in 0..8 {
            let capacity = (bases.len() * ptr_size) as u32;
            // SAFETY: `bases` has `capacity` writable bytes for image base
            // pointers and outlives the call.
            unsafe { EnumDeviceDrivers(bases.as_mut_ptr(), capacity, &mut needed) }
                .map_err(|e| map_error(&e, RESOURCE))?;
            if needed <= capacity {
                actual_count = Some((needed as usize) / ptr_size);
                break;
            }
            let next_len = ((needed as usize).div_ceil(ptr_size)).max(bases.len() * 2);
            bases.resize(next_len, ptr::null_mut());
        }
        let actual_count =
            actual_count.ok_or_else(|| internal("EnumDeviceDrivers kept growing"))?;

        let mut names = Vec::with_capacity(actual_count);
        for &base in bases.get(..actual_count).unwrap_or(&[]) {
            if base.is_null() {
                continue;
            }
            let mut buf = [0u16; MAX_PATH as usize];
            // SAFETY: `base` is an image base returned by `EnumDeviceDrivers`
            // and `buf` is a writable UTF-16 buffer.
            let len = unsafe { GetDeviceDriverBaseNameW(base, &mut buf) };
            if len > 0 {
                let slice = buf.get(..len as usize).unwrap_or(&[]);
                names.push(String::from_utf16_lossy(slice));
            }
        }
        Ok(names)
    }

    fn installed_service_names(&self) -> AppResult<Vec<String>> {
        let mut names = Vec::new();
        enumerate_services(
            SERVICE_WIN32 | SERVICE_DRIVER,
            SERVICE_STATE_ALL,
            "services:installed",
            |entry| {
                if !entry.lpServiceName.is_null() {
                    // SAFETY: `EnumServicesStatusExW` wrote a NUL-terminated
                    // UTF-16 service name inside the still-live buffer.
                    let name = unsafe { entry.lpServiceName.to_string() }
                        .map_err(|_| internal("invalid UTF-16 service name"))?;
                    names.push(name);
                }
                Ok(())
            },
        )?;
        Ok(names)
    }

    fn running_service_count(&self) -> AppResult<u32> {
        let mut count: u32 = 0;
        enumerate_services(SERVICE_WIN32, SERVICE_ACTIVE, "services:running", |_| {
            count = count.saturating_add(1);
            Ok(())
        })?;
        Ok(count)
    }

    fn process_count(&self) -> AppResult<u32> {
        const RESOURCE: &str = "toolhelp:process";
        // SAFETY: `TH32CS_SNAPPROCESS` with PID 0 snapshots all processes; the
        // returned handle is wrapped in `OwnedHandle` for `CloseHandle`.
        let snap = OwnedHandle(
            unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }
                .map_err(|e| map_error(&e, RESOURCE))?,
        );
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut count: u32 = 0;
        // SAFETY: `snap.0` is a valid snapshot handle and `entry.dwSize` is
        // initialized to `size_of::<PROCESSENTRY32W>()`.
        match unsafe { Process32FirstW(snap.0, &mut entry) } {
            Ok(()) => count = count.saturating_add(1),
            Err(e) if is_no_more_files(&e) => return Ok(0),
            Err(e) => return Err(map_error(&e, RESOURCE)),
        }
        loop {
            // SAFETY: `snap.0` is a valid snapshot handle and `entry.dwSize` is
            // initialized.
            match unsafe { Process32NextW(snap.0, &mut entry) } {
                Ok(()) => count = count.saturating_add(1),
                Err(e) if is_no_more_files(&e) => break,
                Err(e) => return Err(map_error(&e, RESOURCE)),
            }
        }
        Ok(count)
    }
}
