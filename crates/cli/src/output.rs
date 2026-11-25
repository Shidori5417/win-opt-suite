//! Output formatting helpers (human-readable text and `--json`).

use std::collections::BTreeMap;
use std::path::Path;

use core_engine::store::TransactionRecord;
use serde::Serialize;
use shared_types::{
    AppError, AppResult, DriftReport, DryRunDiff, MetricSample, ResourceState, Severity,
    SystemFacts, TransactionId, TransactionStatus, TransactionSummary,
};
use system_probe::ProbeWarning;

#[derive(Debug, Clone, Serialize)]
pub struct ValidateReport {
    pub manifests_dir: String,
    pub total_tweaks: usize,
    pub by_category: BTreeMap<String, usize>,
    pub by_profile: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProbeWarningJson {
    pub query: String,
    pub error: AppError,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanReport {
    pub facts: SystemFacts,
    pub warnings: Vec<ProbeWarningJson>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApplyReport {
    pub aborted: bool,
    pub transaction_id: Option<TransactionId>,
    pub dry_run: DryRunDiff,
    pub before_metrics: Vec<MetricSample>,
    pub after_metrics: Vec<MetricSample>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RollbackReport {
    pub transaction_id: TransactionId,
    pub status: TransactionStatus,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecoverReport {
    pub transaction_id: TransactionId,
    pub status: TransactionStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorCheck {
    pub name: String,
    pub status: CheckStatus,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorReport {
    pub ok: bool,
    pub checks: Vec<DoctorCheck>,
}

/// Converts a `#[non_exhaustive]` serializable enum to its serde string name.
pub fn enum_label<T: Serialize + std::fmt::Debug>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(s)) => s,
        _ => format!("{value:?}"),
    }
}

pub fn print_json<T: Serialize>(value: &T) -> AppResult<()> {
    let text = serde_json::to_string_pretty(value).map_err(|e| AppError::Serialization {
        message: e.to_string(),
    })?;
    println!("{text}");
    Ok(())
}

pub fn print_error(err: &AppError, json: bool) {
    if json {
        match serde_json::to_string_pretty(err) {
            Ok(text) => eprintln!("{text}"),
            Err(_) => eprintln!("{err}"),
        }
        return;
    }

    eprintln!("error: {err}");
    match err {
        AppError::InvalidManifest { issues, .. } => {
            for issue in issues {
                eprintln!("  - {issue}");
            }
        }
        AppError::ConflictBlocked { warnings } => {
            for w in warnings {
                eprintln!("  - [{}] {w}", severity_label(w.severity));
            }
        }
        _ => {}
    }
}

pub fn print_incomplete_warning(db_path: &Path, incomplete: &[TransactionRecord]) {
    eprintln!("==================================================================");
    eprintln!(
        "WARNING: {} incomplete transaction(s) detected in {}",
        incomplete.len(),
        db_path.display()
    );
    for tx in incomplete {
        eprintln!("  - {} (status: {})", tx.id, tx.status);
    }
    eprintln!("Run `opti-cli recover` to inspect and recover before applying changes.");
    eprintln!("==================================================================");
}

pub fn print_validate(report: &ValidateReport, json: bool) -> AppResult<()> {
    if json {
        return print_json(report);
    }
    println!(
        "Validated {} tweak manifest(s) from {}",
        report.total_tweaks, report.manifests_dir
    );
    println!("By category:");
    if report.by_category.is_empty() {
        println!("  (none)");
    } else {
        for (cat, count) in &report.by_category {
            println!("  {cat}: {count}");
        }
    }
    println!("By profile:");
    if report.by_profile.is_empty() {
        println!("  (none)");
    } else {
        for (profile, count) in &report.by_profile {
            println!("  {profile}: {count}");
        }
    }
    Ok(())
}

pub fn print_scan(facts: &SystemFacts, warnings: &[ProbeWarning], json: bool) -> AppResult<()> {
    if json {
        let report = ScanReport {
            facts: facts.clone(),
            warnings: warnings
                .iter()
                .map(|w| ProbeWarningJson {
                    query: w.query.to_string(),
                    error: w.error.clone(),
                })
                .collect(),
        };
        return print_json(&report);
    }

    println!(
        "OS: Windows 11 {} (build {}.{}, {})",
        facts.windows_display_version, facts.windows_build, facts.windows_ubr, facts.edition
    );
    println!(
        "CPU: {} ({} physical / {} logical cores)",
        facts.cpu.brand, facts.cpu.physical_cores, facts.cpu.logical_cores
    );
    println!("Memory: {} MiB", facts.total_memory_bytes / (1024 * 1024));
    if facts.gpus.is_empty() {
        println!("GPUs: (none detected)");
    } else {
        for (i, gpu) in facts.gpus.iter().enumerate() {
            println!(
                "GPU #{}: {} (vendor {:#06x}, VRAM {} MiB)",
                i + 1,
                gpu.name,
                gpu.vendor_id,
                gpu.dedicated_memory_bytes / (1024 * 1024)
            );
        }
    }
    println!(
        "Environment: laptop={}, on_battery={}, vm={}, domain_joined={}",
        facts.is_laptop, facts.on_battery, facts.is_virtual_machine, facts.is_domain_joined
    );
    let ac: Vec<String> = facts.anti_cheat.iter().map(enum_label).collect();
    println!(
        "Anti-cheat: {}",
        if ac.is_empty() {
            "(none)".to_owned()
        } else {
            ac.join(", ")
        }
    );
    if !warnings.is_empty() {
        println!("Probe warnings:");
        for w in warnings {
            println!("  - {w}");
        }
    }
    Ok(())
}

pub fn print_dry_run_human(diff: &DryRunDiff) {
    if diff.entries.is_empty() {
        println!("No actions planned.");
    } else {
        println!(
            "{:<28} {:<14} {:<10} {:<44} OLD -> NEW",
            "TWEAK", "PROVIDER", "CHANGE", "RESOURCE"
        );
        for entry in &diff.entries {
            let change = enum_label(&entry.change);
            let old = format_state(entry.before.as_ref());
            let new = format_state(entry.after.as_ref());
            println!(
                "{:<28} {:<14} {:<10} {:<44} {} -> {}",
                entry.tweak_id, entry.provider, change, entry.resource, old, new
            );
        }
    }

    if !diff.warnings.is_empty() {
        println!("Conflict warnings:");
        for w in &diff.warnings {
            println!("  - [{}] {w}", severity_label(w.severity));
        }
    }
    if diff.requires_reboot {
        println!("Note: one or more selected tweaks require a reboot.");
    }
}

pub fn print_history(summaries: &[TransactionSummary], json: bool) -> AppResult<()> {
    if json {
        return print_json(&summaries);
    }
    if summaries.is_empty() {
        println!("No transactions recorded.");
        return Ok(());
    }
    println!(
        "{:<38} {:<14} {:<10} {:<6} {:<16} TWEAKS",
        "ID", "STATUS", "PROFILE", "STEPS", "CREATED_MS"
    );
    for tx in summaries {
        let profile = tx
            .profile
            .as_ref()
            .map(enum_label)
            .unwrap_or_else(|| "-".to_owned());
        let tweaks: Vec<&str> = tx.tweak_ids.iter().map(|t| t.as_str()).collect();
        println!(
            "{:<38} {:<14} {:<10} {:<6} {:<16} {}",
            tx.id,
            tx.status,
            profile,
            tx.step_count,
            tx.created_at_unix_ms,
            tweaks.join(",")
        );
    }
    Ok(())
}

pub fn print_incomplete_list(incomplete: &[TransactionSummary], json: bool) -> AppResult<()> {
    if json {
        return print_json(&incomplete);
    }
    if incomplete.is_empty() {
        println!("No incomplete transactions found.");
        return Ok(());
    }
    println!("Incomplete transactions ({}):", incomplete.len());
    for tx in incomplete {
        let tweaks: Vec<&str> = tx.tweak_ids.iter().map(|t| t.as_str()).collect();
        println!(
            "  - {} status={} steps={} created_ms={} tweaks=[{}]",
            tx.id,
            tx.status,
            tx.step_count,
            tx.created_at_unix_ms,
            tweaks.join(", ")
        );
    }
    println!("Run `opti-cli recover <TX_ID>` to roll back a transaction.");
    Ok(())
}

pub fn print_drift(report: &DriftReport, json: bool) -> AppResult<()> {
    if json {
        return print_json(report);
    }
    println!("Checked {} committed resource(s).", report.checked);
    if report.is_clean() {
        println!("No drift detected.");
        return Ok(());
    }
    if !report.drifted.is_empty() {
        println!("Drifted resources ({}):", report.drifted.len());
        for entry in &report.drifted {
            println!(
                "  - {} ({}, tweak={}, tx={}): expected {}, actual {}",
                entry.resource,
                entry.provider,
                entry.tweak_id,
                entry.transaction_id,
                format_state(entry.expected.as_ref()),
                format_state(entry.actual.as_ref())
            );
        }
    }
    if !report.errors.is_empty() {
        println!("Check errors ({}):", report.errors.len());
        for err in &report.errors {
            println!("  - {}: {}", err.resource, err.error);
        }
    }
    Ok(())
}

pub fn print_doctor(report: &DoctorReport, json: bool) -> AppResult<()> {
    if json {
        return print_json(report);
    }
    for check in &report.checks {
        let badge = match check.status {
            CheckStatus::Pass => "PASS",
            CheckStatus::Warn => "WARN",
            CheckStatus::Fail => "FAIL",
        };
        println!("[{badge}] {}: {}", check.name, check.message);
    }
    Ok(())
}

fn severity_label(severity: Severity) -> &'static str {
    match severity {
        Severity::Info => "INFO",
        Severity::Warning => "WARNING",
        Severity::Blocking => "BLOCKING",
    }
}

fn format_state(state: Option<&ResourceState>) -> String {
    match state {
        None => "<absent>".to_owned(),
        Some(s) => serde_json::to_string(s).unwrap_or_else(|_| format!("{s:?}")),
    }
}
