//! Pure hardware, environment, anti-cheat, and metric derivation logic.
//!
//! Kept independent of `ProbeSource` and OS APIs so every rule can be
//! exhaustively unit-tested on any host.

use shared_types::{
    AntiCheatDriver, MemoryStatus, MetricKind, MetricPhase, MetricSample, PowerStatus,
};

/// SMBIOS System Enclosure (Type 3) chassis type codes that identify portable
/// form factors.
///
/// Includes Portable (8), Laptop (9), Notebook (10), Hand Held (11),
/// Docking Station (12), Sub Notebook (14), Expansion Chassis (18),
/// Peripheral Chassis (21), Tablet (30), Convertible (31), and Detachable (32).
pub const PORTABLE_CHASSIS_CODES: &[u16] = &[8, 9, 10, 11, 12, 14, 18, 21, 30, 31, 32];

const ALL_ANTI_CHEAT: [AntiCheatDriver; 4] = [
    AntiCheatDriver::Vanguard,
    AntiCheatDriver::Faceit,
    AntiCheatDriver::EasyAntiCheat,
    AntiCheatDriver::BattlEye,
];

/// Returns `true` when `code` is an SMBIOS Type 3 portable chassis code.
pub fn is_portable_chassis(code: u16) -> bool {
    PORTABLE_CHASSIS_CODES.contains(&code)
}

/// Returns `true` when the machine has a battery or reports any portable
/// SMBIOS chassis code.
pub fn detect_laptop(power: PowerStatus, chassis_types: &[u16]) -> bool {
    power.has_battery || chassis_types.iter().copied().any(is_portable_chassis)
}

/// Returns `true` when a battery is present and AC power is disconnected.
pub fn detect_on_battery(power: PowerStatus) -> bool {
    power.has_battery && !power.on_ac_power
}

fn driver_basename(raw: &str) -> &str {
    let trimmed = raw.trim();
    trimmed.rsplit(['\\', '/']).next().unwrap_or(trimmed)
}

fn matches_driver(name: &str, driver: AntiCheatDriver) -> bool {
    let base = driver_basename(name);
    match driver {
        AntiCheatDriver::Vanguard => base.eq_ignore_ascii_case("vgk.sys"),
        AntiCheatDriver::Faceit => base.eq_ignore_ascii_case("faceit.sys"),
        AntiCheatDriver::EasyAntiCheat => {
            let lower = base.to_ascii_lowercase();
            lower.starts_with("easyanticheat") && lower.ends_with(".sys")
        }
        AntiCheatDriver::BattlEye => base.eq_ignore_ascii_case("bedaisy.sys"),
        _ => false,
    }
}

fn matches_service(name: &str, driver: AntiCheatDriver) -> bool {
    let trimmed = name.trim();
    match driver {
        AntiCheatDriver::Vanguard => {
            trimmed.eq_ignore_ascii_case("vgk") || trimmed.eq_ignore_ascii_case("vgc")
        }
        AntiCheatDriver::Faceit => trimmed.eq_ignore_ascii_case("faceit"),
        AntiCheatDriver::EasyAntiCheat => trimmed.to_ascii_lowercase().starts_with("easyanticheat"),
        AntiCheatDriver::BattlEye => {
            trimmed.eq_ignore_ascii_case("beservice") || trimmed.eq_ignore_ascii_case("bedaisy")
        }
        _ => false,
    }
}

/// Detects kernel-level anti-cheat drivers from loaded driver filenames and
/// installed Windows service/driver names.
///
/// Matching is case-insensitive. Results are deduplicated and returned in a
/// fixed, deterministic order regardless of input ordering.
pub fn detect_anti_cheat(
    loaded_drivers: &[String],
    installed_services: &[String],
) -> Vec<AntiCheatDriver> {
    ALL_ANTI_CHEAT
        .into_iter()
        .filter(|&driver| {
            loaded_drivers.iter().any(|d| matches_driver(d, driver))
                || installed_services
                    .iter()
                    .any(|s| matches_service(s, driver))
        })
        .collect()
}

/// Computes physical memory currently in use (`total - available`), saturating
/// at `0` if a provider reports `available > total`.
pub fn idle_memory_used_bytes(memory: MemoryStatus) -> u64 {
    memory.total_bytes.saturating_sub(memory.available_bytes)
}

/// Builds the three standard [`MetricSample`] records for a benchmark phase.
pub fn build_metrics(
    phase: MetricPhase,
    now_unix_ms: i64,
    memory: MemoryStatus,
    running_services: u32,
    running_processes: u32,
) -> Vec<MetricSample> {
    vec![
        MetricSample {
            kind: MetricKind::IdleMemoryUsedBytes,
            phase,
            value: idle_memory_used_bytes(memory) as f64,
            captured_at_unix_ms: now_unix_ms,
        },
        MetricSample {
            kind: MetricKind::RunningServices,
            phase,
            value: f64::from(running_services),
            captured_at_unix_ms: now_unix_ms,
        },
        MetricSample {
            kind: MetricKind::RunningProcesses,
            phase,
            value: f64::from(running_processes),
            captured_at_unix_ms: now_unix_ms,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    #[test]
    fn laptop_detection_covers_battery_and_portable_chassis() {
        let desktop_power = PowerStatus {
            has_battery: false,
            on_ac_power: true,
        };
        assert!(!detect_laptop(desktop_power, &[]));
        assert!(!detect_laptop(desktop_power, &[3, 4, 6, 7]));

        let battery_power = PowerStatus {
            has_battery: true,
            on_ac_power: true,
        };
        assert!(detect_laptop(battery_power, &[3]));

        for &code in PORTABLE_CHASSIS_CODES {
            assert!(is_portable_chassis(code));
            assert!(detect_laptop(desktop_power, &[3, code]), "code {code}");
        }
    }

    #[test]
    fn on_battery_requires_battery_and_no_ac() {
        assert!(!detect_on_battery(PowerStatus {
            has_battery: false,
            on_ac_power: false,
        }));
        assert!(!detect_on_battery(PowerStatus {
            has_battery: false,
            on_ac_power: true,
        }));
        assert!(!detect_on_battery(PowerStatus {
            has_battery: true,
            on_ac_power: true,
        }));
        assert!(detect_on_battery(PowerStatus {
            has_battery: true,
            on_ac_power: false,
        }));
    }

    #[test]
    fn anti_cheat_detection_from_drivers_and_services_is_case_insensitive_and_deterministic() {
        // Empty
        assert!(detect_anti_cheat(&[], &[]).is_empty());

        // Non-matching entries
        assert!(
            detect_anti_cheat(&s(&["ntoskrnl.exe", "nvlddmkm.sys"]), &s(&["Spooler"])).is_empty()
        );

        // Vanguard via driver (case-insensitive, with path) and services
        assert_eq!(
            detect_anti_cheat(&s(&[r"\SystemRoot\System32\drivers\VGK.SYS"]), &[]),
            vec![AntiCheatDriver::Vanguard]
        );
        assert_eq!(
            detect_anti_cheat(&[], &s(&["VGC"])),
            vec![AntiCheatDriver::Vanguard]
        );
        assert_eq!(
            detect_anti_cheat(&[], &s(&["vgk"])),
            vec![AntiCheatDriver::Vanguard]
        );

        // FACEIT
        assert_eq!(
            detect_anti_cheat(&s(&["FaCeIt.SyS"]), &[]),
            vec![AntiCheatDriver::Faceit]
        );
        assert_eq!(
            detect_anti_cheat(&[], &s(&["faceit"])),
            vec![AntiCheatDriver::Faceit]
        );

        // EasyAntiCheat (glob *.sys and prefix service)
        assert_eq!(
            detect_anti_cheat(&s(&["EasyAntiCheat_EOS.sys"]), &[]),
            vec![AntiCheatDriver::EasyAntiCheat]
        );
        assert_eq!(
            detect_anti_cheat(&s(&["easyanticheat.sys"]), &[]),
            vec![AntiCheatDriver::EasyAntiCheat]
        );
        assert_eq!(
            detect_anti_cheat(&[], &s(&["EasyAntiCheat_EOS"])),
            vec![AntiCheatDriver::EasyAntiCheat]
        );
        assert!(detect_anti_cheat(&s(&["EasyAntiCheat.exe"]), &[]).is_empty());

        // BattlEye
        assert_eq!(
            detect_anti_cheat(&s(&["bedaisy.SYS"]), &[]),
            vec![AntiCheatDriver::BattlEye]
        );
        assert_eq!(
            detect_anti_cheat(&[], &s(&["BEService"])),
            vec![AntiCheatDriver::BattlEye]
        );
        assert_eq!(
            detect_anti_cheat(&[], &s(&["bedaisy"])),
            vec![AntiCheatDriver::BattlEye]
        );

        // Deduplicated and deterministic regardless of input order
        let detected = detect_anti_cheat(
            &s(&["bedaisy.sys", "vgk.sys", "FACEIT.sys", "VGK.SYS"]),
            &s(&["EasyAntiCheat_EOS", "vgc", "beservice", "FACEIT"]),
        );
        assert_eq!(
            detected,
            vec![
                AntiCheatDriver::Vanguard,
                AntiCheatDriver::Faceit,
                AntiCheatDriver::EasyAntiCheat,
                AntiCheatDriver::BattlEye,
            ]
        );
    }

    #[test]
    fn build_metrics_computes_idle_memory_and_counts() {
        let mem = MemoryStatus {
            total_bytes: 32 * 1024 * 1024 * 1024,
            available_bytes: 20 * 1024 * 1024 * 1024,
        };
        let samples = build_metrics(MetricPhase::Before, 1_700_000_000_000, mem, 84, 142);
        assert_eq!(samples.len(), 3);
        assert_eq!(samples[0].kind, MetricKind::IdleMemoryUsedBytes);
        assert_eq!(samples[0].phase, MetricPhase::Before);
        assert_eq!(samples[0].value, (12u64 * 1024 * 1024 * 1024) as f64);
        assert_eq!(samples[0].captured_at_unix_ms, 1_700_000_000_000);

        assert_eq!(samples[1].kind, MetricKind::RunningServices);
        assert_eq!(samples[1].value, 84.0);

        assert_eq!(samples[2].kind, MetricKind::RunningProcesses);
        assert_eq!(samples[2].value, 142.0);
    }

    #[test]
    fn idle_memory_saturates_when_available_exceeds_total() {
        let mem = MemoryStatus {
            total_bytes: 100,
            available_bytes: 200,
        };
        assert_eq!(idle_memory_used_bytes(mem), 0);
    }
}
