//! Read-only system facts produced by `system-probe` and consumed by the
//! conflict guard and the UI.

use serde::{Deserialize, Serialize};

use crate::manifest::EnvironmentTag;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum AntiCheatDriver {
    /// Riot Vanguard (`vgk.sys`).
    Vanguard,
    /// FACEIT (`FACEIT.sys`).
    Faceit,
    /// Easy Anti-Cheat (`EasyAntiCheat*.sys` / service).
    EasyAntiCheat,
    /// BattlEye (`BEDaisy.sys`).
    BattlEye,
}

impl AntiCheatDriver {
    pub const fn environment_tag(self) -> EnvironmentTag {
        match self {
            AntiCheatDriver::Vanguard => EnvironmentTag::AntiCheatVanguard,
            AntiCheatDriver::Faceit => EnvironmentTag::AntiCheatFaceit,
            AntiCheatDriver::EasyAntiCheat => EnvironmentTag::AntiCheatEasyAntiCheat,
            AntiCheatDriver::BattlEye => EnvironmentTag::AntiCheatBattlEye,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct CpuInfo {
    pub brand: String,
    pub physical_cores: u32,
    pub logical_cores: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct GpuInfo {
    pub name: String,
    /// PCI vendor id (e.g. 0x10DE NVIDIA, 0x1002 AMD, 0x8086 Intel).
    pub vendor_id: u32,
    pub dedicated_memory_bytes: u64,
}

/// Snapshot of the machine the tool is running on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SystemFacts {
    pub windows_build: u32,
    /// Update Build Revision (the `.xxxx` part of e.g. `26100.2605`).
    pub windows_ubr: u32,
    pub windows_display_version: String,
    pub edition: String,
    pub cpu: CpuInfo,
    pub gpus: Vec<GpuInfo>,
    pub total_memory_bytes: u64,
    pub is_laptop: bool,
    pub on_battery: bool,
    pub is_virtual_machine: bool,
    pub is_domain_joined: bool,
    pub anti_cheat: Vec<AntiCheatDriver>,
}

impl SystemFacts {
    /// Environment tags used to evaluate `excluded_on` of manifests.
    pub fn environment_tags(&self) -> Vec<EnvironmentTag> {
        let mut tags = Vec::new();
        tags.push(if self.is_laptop {
            EnvironmentTag::Laptop
        } else {
            EnvironmentTag::Desktop
        });
        if self.on_battery {
            tags.push(EnvironmentTag::OnBattery);
        }
        if self.is_virtual_machine {
            tags.push(EnvironmentTag::VirtualMachine);
        }
        if self.is_domain_joined {
            tags.push(EnvironmentTag::DomainJoined);
        }
        tags.extend(self.anti_cheat.iter().map(|d| d.environment_tag()));
        tags
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum MetricKind {
    /// Physical memory in use while idle, in bytes.
    IdleMemoryUsedBytes,
    RunningServices,
    RunningProcesses,
}

impl MetricKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            MetricKind::IdleMemoryUsedBytes => "idle_memory_used_bytes",
            MetricKind::RunningServices => "running_services",
            MetricKind::RunningProcesses => "running_processes",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum MetricPhase {
    Before,
    After,
}

impl MetricPhase {
    pub const fn as_str(self) -> &'static str {
        match self {
            MetricPhase::Before => "before",
            MetricPhase::After => "after",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct MetricSample {
    pub kind: MetricKind,
    pub phase: MetricPhase,
    pub value: f64,
    pub captured_at_unix_ms: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_tags_cover_laptop_and_anti_cheat() {
        let facts = SystemFacts {
            windows_build: 26100,
            windows_ubr: 1,
            windows_display_version: "24H2".into(),
            edition: "Professional".into(),
            cpu: CpuInfo {
                brand: "x".into(),
                physical_cores: 8,
                logical_cores: 16,
            },
            gpus: vec![],
            total_memory_bytes: 0,
            is_laptop: true,
            on_battery: false,
            is_virtual_machine: false,
            is_domain_joined: false,
            anti_cheat: vec![AntiCheatDriver::Vanguard],
        };
        let tags = facts.environment_tags();
        assert!(tags.contains(&EnvironmentTag::Laptop));
        assert!(!tags.contains(&EnvironmentTag::Desktop));
        assert!(tags.contains(&EnvironmentTag::AntiCheatVanguard));
    }
}
