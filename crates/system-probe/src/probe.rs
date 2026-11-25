//! High-level `SystemProbe` orchestrating read-only [`ProbeSource`] queries.

use std::fmt;

use shared_types::{
    AppError, AppResult, CpuInfo, MemoryStatus, MetricPhase, MetricSample, PowerStatus,
    ProbeSource, SystemFacts,
};

use crate::detect::{build_metrics, detect_anti_cheat, detect_laptop, detect_on_battery};

/// Individual non-fatal query that degraded to a safe default while
/// collecting [`SystemFacts`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ProbeQuery {
    Cpu,
    Gpus,
    Memory,
    Power,
    ChassisTypes,
    VirtualMachine,
    DomainJoined,
    LoadedDrivers,
    InstalledServices,
}

impl fmt::Display for ProbeQuery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            ProbeQuery::Cpu => "cpu",
            ProbeQuery::Gpus => "gpus",
            ProbeQuery::Memory => "memory",
            ProbeQuery::Power => "power",
            ProbeQuery::ChassisTypes => "chassis_types",
            ProbeQuery::VirtualMachine => "is_virtual_machine",
            ProbeQuery::DomainJoined => "is_domain_joined",
            ProbeQuery::LoadedDrivers => "loaded_driver_names",
            ProbeQuery::InstalledServices => "installed_service_names",
        };
        f.write_str(label)
    }
}

/// Recorded when an optional [`ProbeSource`] query fails and degrades to a
/// safe default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeWarning {
    pub query: ProbeQuery,
    pub error: AppError,
}

impl fmt::Display for ProbeWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "probe query `{}` degraded: {}", self.query, self.error)
    }
}

/// Read-only system probe backed by an injected [`ProbeSource`].
pub struct SystemProbe {
    source: Box<dyn ProbeSource>,
}

impl SystemProbe {
    /// Creates a new probe wrapping `source`.
    pub fn new(source: Box<dyn ProbeSource>) -> Self {
        Self { source }
    }

    /// Collects [`SystemFacts`], degrading non-OS queries to safe defaults.
    ///
    /// Fails only if `os_version()` fails, because build/edition detection is
    /// required by the conflict guard.
    pub fn facts(&self) -> AppResult<SystemFacts> {
        self.facts_with_warnings().map(|(facts, _)| facts)
    }

    /// Collects [`SystemFacts`] along with a [`ProbeWarning`] for each optional
    /// query that failed and fell back to a safe default.
    ///
    /// Fails if `os_version()` returns an error.
    pub fn facts_with_warnings(&self) -> AppResult<(SystemFacts, Vec<ProbeWarning>)> {
        let os = self.source.os_version()?;
        let mut warnings = Vec::new();

        let cpu = query_or_else(
            ProbeQuery::Cpu,
            self.source.cpu(),
            || CpuInfo {
                brand: String::new(),
                physical_cores: 0,
                logical_cores: 0,
            },
            &mut warnings,
        );
        let gpus = query_or_else(
            ProbeQuery::Gpus,
            self.source.gpus(),
            Vec::new,
            &mut warnings,
        );
        let memory = query_or_else(
            ProbeQuery::Memory,
            self.source.memory(),
            || MemoryStatus {
                total_bytes: 0,
                available_bytes: 0,
            },
            &mut warnings,
        );
        let power = query_or_else(
            ProbeQuery::Power,
            self.source.power(),
            || PowerStatus {
                has_battery: false,
                on_ac_power: false,
            },
            &mut warnings,
        );
        let chassis = query_or_else(
            ProbeQuery::ChassisTypes,
            self.source.chassis_types(),
            Vec::new,
            &mut warnings,
        );
        let is_virtual_machine = query_or_else(
            ProbeQuery::VirtualMachine,
            self.source.is_virtual_machine(),
            || false,
            &mut warnings,
        );
        let is_domain_joined = query_or_else(
            ProbeQuery::DomainJoined,
            self.source.is_domain_joined(),
            || false,
            &mut warnings,
        );
        let loaded_drivers = query_or_else(
            ProbeQuery::LoadedDrivers,
            self.source.loaded_driver_names(),
            Vec::new,
            &mut warnings,
        );
        let installed_services = query_or_else(
            ProbeQuery::InstalledServices,
            self.source.installed_service_names(),
            Vec::new,
            &mut warnings,
        );

        let is_laptop = detect_laptop(power, &chassis);
        let on_battery = detect_on_battery(power);
        let anti_cheat = detect_anti_cheat(&loaded_drivers, &installed_services);

        let facts = SystemFacts {
            windows_build: os.build,
            windows_ubr: os.ubr,
            windows_display_version: os.display_version,
            edition: os.edition,
            cpu,
            gpus,
            total_memory_bytes: memory.total_bytes,
            is_laptop,
            on_battery,
            is_virtual_machine,
            is_domain_joined,
            anti_cheat,
        };

        Ok((facts, warnings))
    }

    /// Captures a before/after snapshot of memory, service, and process metrics.
    pub fn collect_metrics(
        &self,
        phase: MetricPhase,
        now_unix_ms: i64,
    ) -> AppResult<Vec<MetricSample>> {
        let memory = self.source.memory()?;
        let running_services = self.source.running_service_count()?;
        let running_processes = self.source.process_count()?;
        Ok(build_metrics(
            phase,
            now_unix_ms,
            memory,
            running_services,
            running_processes,
        ))
    }
}

fn query_or_else<T>(
    query: ProbeQuery,
    result: AppResult<T>,
    fallback: impl FnOnce() -> T,
    warnings: &mut Vec<ProbeWarning>,
) -> T {
    match result {
        Ok(value) => value,
        Err(error) => {
            warnings.push(ProbeWarning { query, error });
            fallback()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared_types::{AntiCheatDriver, EnvironmentTag, GpuInfo, MetricKind, OsVersionInfo};

    struct FakeProbeSource {
        os_version: AppResult<OsVersionInfo>,
        cpu: AppResult<CpuInfo>,
        gpus: AppResult<Vec<GpuInfo>>,
        memory: AppResult<MemoryStatus>,
        power: AppResult<PowerStatus>,
        chassis_types: AppResult<Vec<u16>>,
        is_vm: AppResult<bool>,
        is_domain: AppResult<bool>,
        drivers: AppResult<Vec<String>>,
        services: AppResult<Vec<String>>,
        running_services: AppResult<u32>,
        processes: AppResult<u32>,
    }

    impl Default for FakeProbeSource {
        fn default() -> Self {
            Self {
                os_version: Ok(OsVersionInfo {
                    build: 26100,
                    ubr: 2605,
                    display_version: "24H2".into(),
                    edition: "Professional".into(),
                }),
                cpu: Ok(CpuInfo {
                    brand: "AMD Ryzen 7 7800X3D".into(),
                    physical_cores: 8,
                    logical_cores: 16,
                }),
                gpus: Ok(vec![GpuInfo {
                    name: "NVIDIA GeForce RTX 4080".into(),
                    vendor_id: 0x10DE,
                    dedicated_memory_bytes: 16 * 1024 * 1024 * 1024,
                }]),
                memory: Ok(MemoryStatus {
                    total_bytes: 32 * 1024 * 1024 * 1024,
                    available_bytes: 22 * 1024 * 1024 * 1024,
                }),
                power: Ok(PowerStatus {
                    has_battery: false,
                    on_ac_power: true,
                }),
                chassis_types: Ok(vec![3]),
                is_vm: Ok(false),
                is_domain: Ok(false),
                drivers: Ok(vec![]),
                services: Ok(vec![]),
                running_services: Ok(95),
                processes: Ok(130),
            }
        }
    }

    impl ProbeSource for FakeProbeSource {
        fn os_version(&self) -> AppResult<OsVersionInfo> {
            self.os_version.clone()
        }

        fn cpu(&self) -> AppResult<CpuInfo> {
            self.cpu.clone()
        }

        fn gpus(&self) -> AppResult<Vec<GpuInfo>> {
            self.gpus.clone()
        }

        fn memory(&self) -> AppResult<MemoryStatus> {
            self.memory.clone()
        }

        fn power(&self) -> AppResult<PowerStatus> {
            self.power.clone()
        }

        fn chassis_types(&self) -> AppResult<Vec<u16>> {
            self.chassis_types.clone()
        }

        fn is_virtual_machine(&self) -> AppResult<bool> {
            self.is_vm.clone()
        }

        fn is_domain_joined(&self) -> AppResult<bool> {
            self.is_domain.clone()
        }

        fn loaded_driver_names(&self) -> AppResult<Vec<String>> {
            self.drivers.clone()
        }

        fn installed_service_names(&self) -> AppResult<Vec<String>> {
            self.services.clone()
        }

        fn running_service_count(&self) -> AppResult<u32> {
            self.running_services.clone()
        }

        fn process_count(&self) -> AppResult<u32> {
            self.processes.clone()
        }
    }

    fn err(msg: &str) -> AppError {
        AppError::Internal {
            message: msg.to_owned(),
        }
    }

    #[test]
    fn desktop_facts_happy_path() {
        let probe = SystemProbe::new(Box::new(FakeProbeSource::default()));
        let (facts, warnings) = probe.facts_with_warnings().unwrap();
        assert!(warnings.is_empty());
        assert_eq!(facts.windows_build, 26100);
        assert_eq!(facts.windows_ubr, 2605);
        assert_eq!(facts.windows_display_version, "24H2");
        assert_eq!(facts.edition, "Professional");
        assert_eq!(facts.cpu.physical_cores, 8);
        assert_eq!(facts.gpus.len(), 1);
        assert_eq!(facts.total_memory_bytes, 32 * 1024 * 1024 * 1024);
        assert!(!facts.is_laptop);
        assert!(!facts.on_battery);
        assert!(!facts.is_virtual_machine);
        assert!(!facts.is_domain_joined);
        assert!(facts.anti_cheat.is_empty());
        assert!(facts.environment_tags().contains(&EnvironmentTag::Desktop));
    }

    #[test]
    fn laptop_on_battery_via_chassis_and_power() {
        let src = FakeProbeSource {
            power: Ok(PowerStatus {
                has_battery: true,
                on_ac_power: false,
            }),
            chassis_types: Ok(vec![10]),
            is_vm: Ok(true),
            is_domain: Ok(true),
            drivers: Ok(vec!["vgk.sys".into(), "BEDaisy.sys".into()]),
            services: Ok(vec!["FACEIT".into(), "EasyAntiCheat_EOS".into()]),
            ..Default::default()
        };
        let probe = SystemProbe::new(Box::new(src));
        let facts = probe.facts().unwrap();
        assert!(facts.is_laptop);
        assert!(facts.on_battery);
        assert!(facts.is_virtual_machine);
        assert!(facts.is_domain_joined);
        assert_eq!(
            facts.anti_cheat,
            vec![
                AntiCheatDriver::Vanguard,
                AntiCheatDriver::Faceit,
                AntiCheatDriver::EasyAntiCheat,
                AntiCheatDriver::BattlEye,
            ]
        );
        let tags = facts.environment_tags();
        assert!(tags.contains(&EnvironmentTag::Laptop));
        assert!(tags.contains(&EnvironmentTag::OnBattery));
        assert!(tags.contains(&EnvironmentTag::VirtualMachine));
        assert!(tags.contains(&EnvironmentTag::DomainJoined));
    }

    #[test]
    fn laptop_detected_from_chassis_even_if_power_query_fails() {
        let src = FakeProbeSource {
            power: Err(err("power unavailable")),
            chassis_types: Ok(vec![31]),
            ..Default::default()
        };
        let probe = SystemProbe::new(Box::new(src));
        let (facts, warnings) = probe.facts_with_warnings().unwrap();
        assert!(facts.is_laptop);
        assert!(!facts.on_battery);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].query, ProbeQuery::Power);
    }

    #[test]
    fn os_version_failure_is_fatal() {
        let src = FakeProbeSource {
            os_version: Err(AppError::NotFound {
                resource: "CurrentBuildNumber".into(),
            }),
            ..Default::default()
        };
        let probe = SystemProbe::new(Box::new(src));
        assert!(matches!(probe.facts(), Err(AppError::NotFound { .. })));
        assert!(matches!(
            probe.facts_with_warnings(),
            Err(AppError::NotFound { .. })
        ));
    }

    #[test]
    fn non_os_failures_degrade_gracefully_with_warnings() {
        let src = FakeProbeSource {
            cpu: Err(err("cpu fail")),
            gpus: Err(err("gpu fail")),
            memory: Err(err("mem fail")),
            power: Err(err("power fail")),
            chassis_types: Err(err("chassis fail")),
            is_vm: Err(err("vm fail")),
            is_domain: Err(err("domain fail")),
            drivers: Err(err("drivers fail")),
            services: Ok(vec!["vgc".into()]),
            ..Default::default()
        };
        let probe = SystemProbe::new(Box::new(src));
        let (facts, warnings) = probe.facts_with_warnings().unwrap();
        assert_eq!(facts.windows_build, 26100);
        assert_eq!(facts.cpu.brand, "");
        assert_eq!(facts.cpu.physical_cores, 0);
        assert!(facts.gpus.is_empty());
        assert_eq!(facts.total_memory_bytes, 0);
        assert!(!facts.is_laptop);
        assert!(!facts.on_battery);
        assert!(!facts.is_virtual_machine);
        assert!(!facts.is_domain_joined);
        // Service-based detection still works even when loaded_driver_names fails.
        assert_eq!(facts.anti_cheat, vec![AntiCheatDriver::Vanguard]);

        let queries: Vec<ProbeQuery> = warnings.iter().map(|w| w.query).collect();
        assert_eq!(
            queries,
            vec![
                ProbeQuery::Cpu,
                ProbeQuery::Gpus,
                ProbeQuery::Memory,
                ProbeQuery::Power,
                ProbeQuery::ChassisTypes,
                ProbeQuery::VirtualMachine,
                ProbeQuery::DomainJoined,
                ProbeQuery::LoadedDrivers,
            ]
        );
        assert!(warnings[0].to_string().contains("cpu fail"));
    }

    #[test]
    fn collect_metrics_returns_all_three_samples_and_propagates_errors() {
        let probe = SystemProbe::new(Box::new(FakeProbeSource::default()));
        let samples = probe.collect_metrics(MetricPhase::After, 42_000).unwrap();
        assert_eq!(samples.len(), 3);
        assert_eq!(samples[0].kind, MetricKind::IdleMemoryUsedBytes);
        assert_eq!(samples[0].phase, MetricPhase::After);
        assert_eq!(samples[0].value, (10u64 * 1024 * 1024 * 1024) as f64);
        assert_eq!(samples[0].captured_at_unix_ms, 42_000);
        assert_eq!(samples[1].kind, MetricKind::RunningServices);
        assert_eq!(samples[1].value, 95.0);
        assert_eq!(samples[2].kind, MetricKind::RunningProcesses);
        assert_eq!(samples[2].value, 130.0);

        let failing = SystemProbe::new(Box::new(FakeProbeSource {
            running_services: Err(AppError::AccessDenied {
                resource: "scm".into(),
            }),
            ..Default::default()
        }));
        assert!(matches!(
            failing.collect_metrics(MetricPhase::Before, 1),
            Err(AppError::AccessDenied { .. })
        ));
    }
}
