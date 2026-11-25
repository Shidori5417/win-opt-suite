//! Thin command handlers orchestrating `core-engine`, `providers`, and `system-probe`.

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use core_engine::store::migrations::MIGRATIONS;
use core_engine::{Catalog, DriftDetector, FileLock, ProviderRegistry, Store, TransactionManager};
use shared_types::{
    AppError, AppResult, MetricPhase, MetricSample, Profile, ProviderKind, SystemFacts,
    TransactionId, TransactionStatus, TweakId, TweakManifest,
};
use system_probe::ProbeWarning;

use crate::cli::{ApplyArgs, Command, SelectionArgs};
use crate::output::{
    ApplyReport, CheckStatus, DoctorCheck, DoctorReport, RecoverReport, RollbackReport,
    ValidateReport, enum_label, print_doctor, print_drift, print_dry_run_human, print_history,
    print_incomplete_list, print_incomplete_warning, print_json, print_scan, print_validate,
};

pub fn resolve_db_path(explicit: Option<PathBuf>) -> PathBuf {
    if let Some(path) = explicit {
        return path;
    }
    if let Some(program_data) = std::env::var_os("ProgramData")
        && !program_data.is_empty()
    {
        return PathBuf::from(program_data)
            .join("WinOptSuite")
            .join("state.db");
    }
    PathBuf::from("state.db")
}

pub fn resolve_manifests_dir(explicit: Option<PathBuf>) -> PathBuf {
    if let Some(dir) = explicit {
        return dir;
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        let beside_exe = parent.join("manifests").join("tweaks");
        if beside_exe.is_dir() {
            return beside_exe;
        }
    }
    PathBuf::from("manifests").join("tweaks")
}

pub fn warn_if_incomplete(db_path: &Path, command: &Command) {
    if matches!(command, Command::Recover { .. }) || !db_path.exists() {
        return;
    }
    let Ok(store) = Store::open(db_path) else {
        return;
    };
    let Ok(incomplete) = store.transactions_with_status(&[
        TransactionStatus::Pending,
        TransactionStatus::Applying,
        TransactionStatus::RollingBack,
    ]) else {
        return;
    };
    if !incomplete.is_empty() {
        print_incomplete_warning(db_path, &incomplete);
    }
}

pub fn execute(
    command: Command,
    db_path: &Path,
    manifests_dir: &Path,
    json: bool,
) -> AppResult<ExitCode> {
    match command {
        Command::Validate => cmd_validate(manifests_dir, json),
        Command::Scan => cmd_scan(json),
        Command::DryRun(selection) => cmd_dry_run(&selection, manifests_dir, json),
        Command::Apply(args) => cmd_apply(&args, db_path, manifests_dir, json),
        Command::Rollback { tx_id } => cmd_rollback(&tx_id, db_path, json),
        Command::Recover { tx_id } => cmd_recover(tx_id.as_deref(), db_path, json),
        Command::History => cmd_history(db_path, json),
        Command::Drift => cmd_drift(db_path, json),
        Command::Doctor => cmd_doctor(db_path, manifests_dir, json),
    }
}

fn cmd_validate(manifests_dir: &Path, json: bool) -> AppResult<ExitCode> {
    let catalog = Catalog::load_dir(manifests_dir)?;
    let mut by_category: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_profile: BTreeMap<String, usize> = BTreeMap::new();

    for manifest in catalog.iter() {
        *by_category
            .entry(enum_label(&manifest.category))
            .or_insert(0) += 1;
        for profile in &manifest.profiles {
            *by_profile.entry(enum_label(profile)).or_insert(0) += 1;
        }
    }

    let report = ValidateReport {
        manifests_dir: manifests_dir.display().to_string(),
        total_tweaks: catalog.len(),
        by_category,
        by_profile,
    };
    print_validate(&report, json)?;
    Ok(ExitCode::SUCCESS)
}

fn cmd_scan(json: bool) -> AppResult<ExitCode> {
    let (facts, warnings) = scan_facts_with_warnings()?;
    print_scan(&facts, &warnings, json)?;
    Ok(ExitCode::SUCCESS)
}

fn cmd_dry_run(selection: &SelectionArgs, manifests_dir: &Path, json: bool) -> AppResult<ExitCode> {
    let (facts, warnings) = scan_facts_with_warnings()?;
    emit_probe_warnings(&warnings);
    let catalog = Catalog::load_dir(manifests_dir)?;
    let (_, manifests) = resolve_selection(&catalog, selection)?;
    let registry = build_registry()?;
    let diff = core_engine::DryRunPlanner::new(&registry).plan(&manifests, &facts)?;

    if json {
        print_json(&diff)?;
    } else {
        print_dry_run_human(&diff);
    }

    if diff.has_blocking_conflicts() {
        Ok(ExitCode::from(2))
    } else {
        Ok(ExitCode::SUCCESS)
    }
}

fn cmd_apply(
    args: &ApplyArgs,
    db_path: &Path,
    manifests_dir: &Path,
    json: bool,
) -> AppResult<ExitCode> {
    let (facts, warnings) = scan_facts_with_warnings()?;
    emit_probe_warnings(&warnings);
    let catalog = Catalog::load_dir(manifests_dir)?;
    let (profile, manifests) = resolve_selection(&catalog, &args.selection)?;
    let manager = open_manager(db_path)?;

    let diff = manager.planner().plan(&manifests, &facts)?;
    if !json {
        print_dry_run_human(&diff);
    }
    if diff.has_blocking_conflicts() {
        return Err(AppError::ConflictBlocked {
            warnings: diff.warnings,
        });
    }

    if !args.yes && !confirm_apply()? {
        if json {
            let report = ApplyReport {
                aborted: true,
                transaction_id: None,
                dry_run: diff,
                before_metrics: Vec::new(),
                after_metrics: Vec::new(),
            };
            print_json(&report)?;
        } else {
            println!("Aborted; no changes were made.");
        }
        return Ok(ExitCode::SUCCESS);
    }

    let before_metrics = collect_metrics(MetricPhase::Before)?;
    let tx_id = manager.apply(&manifests, profile, &facts)?;
    let after_metrics = collect_metrics(MetricPhase::After)?;

    for sample in before_metrics.iter().chain(after_metrics.iter()) {
        manager.store().insert_metric(Some(&tx_id), sample)?;
    }

    if json {
        let report = ApplyReport {
            aborted: false,
            transaction_id: Some(tx_id),
            dry_run: diff,
            before_metrics,
            after_metrics,
        };
        print_json(&report)?;
    } else {
        println!("Applied transaction: {tx_id}");
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_rollback(tx_id: &str, db_path: &Path, json: bool) -> AppResult<ExitCode> {
    let id = TransactionId::new(tx_id);
    let manager = open_manager(db_path)?;
    manager.rollback(&id)?;
    if json {
        print_json(&RollbackReport {
            transaction_id: id,
            status: TransactionStatus::RolledBack,
        })?;
    } else {
        println!("Rolled back transaction {tx_id}.");
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_recover(tx_id: Option<&str>, db_path: &Path, json: bool) -> AppResult<ExitCode> {
    let manager = open_manager(db_path)?;
    match tx_id {
        None => {
            let incomplete = manager.find_incomplete()?;
            print_incomplete_list(&incomplete, json)?;
        }
        Some(raw) => {
            let id = TransactionId::new(raw);
            let status = manager.recover(&id)?;
            if json {
                print_json(&RecoverReport {
                    transaction_id: id,
                    status,
                })?;
            } else {
                println!("Recovered transaction {raw} (status: {status}).");
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_history(db_path: &Path, json: bool) -> AppResult<ExitCode> {
    let manager = open_manager(db_path)?;
    let summaries = manager.history()?;
    print_history(&summaries, json)?;
    Ok(ExitCode::SUCCESS)
}

fn cmd_drift(db_path: &Path, json: bool) -> AppResult<ExitCode> {
    let store = open_store(db_path)?;
    let registry = build_registry()?;
    let report = DriftDetector::new(&store, &registry).detect()?;
    print_drift(&report, json)?;
    if !report.drifted.is_empty() {
        Ok(ExitCode::from(2))
    } else if !report.errors.is_empty() {
        Ok(ExitCode::from(1))
    } else {
        Ok(ExitCode::SUCCESS)
    }
}

fn cmd_doctor(db_path: &Path, manifests_dir: &Path, json: bool) -> AppResult<ExitCode> {
    let mut checks = Vec::new();

    // 1. Platform support
    #[cfg(windows)]
    checks.push(DoctorCheck {
        name: "platform".to_owned(),
        status: CheckStatus::Pass,
        message: "Running on Windows".to_owned(),
    });
    #[cfg(not(windows))]
    checks.push(DoctorCheck {
        name: "platform".to_owned(),
        status: CheckStatus::Fail,
        message: format!(
            "Unsupported OS `{}`: Windows 11 is required for native providers and system probe",
            std::env::consts::OS
        ),
    });

    // 2. Database opens + migrations version, and 3. Incomplete transactions
    let migration_version = MIGRATIONS.iter().map(|m| m.version).max().unwrap_or(0);
    match open_store(db_path) {
        Ok(store) => {
            checks.push(DoctorCheck {
                name: "database".to_owned(),
                status: CheckStatus::Pass,
                message: format!(
                    "Opened {} (schema version {})",
                    db_path.display(),
                    migration_version
                ),
            });
            match store.transactions_with_status(&[
                TransactionStatus::Pending,
                TransactionStatus::Applying,
                TransactionStatus::RollingBack,
            ]) {
                Ok(incomplete) if incomplete.is_empty() => checks.push(DoctorCheck {
                    name: "incomplete_transactions".to_owned(),
                    status: CheckStatus::Pass,
                    message: "No incomplete transactions".to_owned(),
                }),
                Ok(incomplete) => {
                    let ids: Vec<&str> = incomplete.iter().map(|t| t.id.as_str()).collect();
                    checks.push(DoctorCheck {
                        name: "incomplete_transactions".to_owned(),
                        status: CheckStatus::Warn,
                        message: format!(
                            "{} incomplete transaction(s) require recovery: {}",
                            incomplete.len(),
                            ids.join(", ")
                        ),
                    });
                }
                Err(err) => checks.push(DoctorCheck {
                    name: "incomplete_transactions".to_owned(),
                    status: CheckStatus::Fail,
                    message: err.to_string(),
                }),
            }
        }
        Err(err) => {
            checks.push(DoctorCheck {
                name: "database".to_owned(),
                status: CheckStatus::Fail,
                message: format!("Failed to open {}: {err}", db_path.display()),
            });
            checks.push(DoctorCheck {
                name: "incomplete_transactions".to_owned(),
                status: CheckStatus::Warn,
                message: "Skipped because database could not be opened".to_owned(),
            });
        }
    }

    // 4. Catalog valid
    match Catalog::load_dir(manifests_dir) {
        Ok(catalog) => checks.push(DoctorCheck {
            name: "catalog".to_owned(),
            status: CheckStatus::Pass,
            message: format!(
                "Loaded {} valid manifest(s) from {}",
                catalog.len(),
                manifests_dir.display()
            ),
        }),
        Err(err) => checks.push(DoctorCheck {
            name: "catalog".to_owned(),
            status: CheckStatus::Fail,
            message: format!("Catalog invalid in {}: {err}", manifests_dir.display()),
        }),
    }

    // 5. Registered provider kinds
    match build_registry() {
        Ok(registry) => {
            let kinds = registry.kinds();
            #[cfg(windows)]
            {
                let labels: Vec<&str> = kinds.iter().map(|k| k.as_str()).collect();
                let missing: Vec<&str> = ProviderKind::ALL
                    .iter()
                    .filter(|k| !kinds.contains(k))
                    .map(|k| k.as_str())
                    .collect();
                if missing.is_empty() {
                    checks.push(DoctorCheck {
                        name: "providers".to_owned(),
                        status: CheckStatus::Pass,
                        message: format!("Registered: {}", labels.join(", ")),
                    });
                } else {
                    checks.push(DoctorCheck {
                        name: "providers".to_owned(),
                        status: CheckStatus::Fail,
                        message: format!("Missing providers: {}", missing.join(", ")),
                    });
                }
            }
            #[cfg(not(windows))]
            checks.push(DoctorCheck {
                name: "providers".to_owned(),
                status: CheckStatus::Warn,
                message: format!(
                    "{} of {} native providers registered (expected off Windows)",
                    kinds.len(),
                    ProviderKind::ALL.len()
                ),
            });
        }
        Err(err) => checks.push(DoctorCheck {
            name: "providers".to_owned(),
            status: CheckStatus::Fail,
            message: err.to_string(),
        }),
    }

    // 6. Probe availability
    #[cfg(windows)]
    match scan_facts_with_warnings() {
        Ok((facts, warnings)) if warnings.is_empty() => checks.push(DoctorCheck {
            name: "probe".to_owned(),
            status: CheckStatus::Pass,
            message: format!(
                "Windows build {}.{} ({})",
                facts.windows_build, facts.windows_ubr, facts.edition
            ),
        }),
        Ok((facts, warnings)) => {
            let degraded: Vec<String> = warnings.iter().map(|w| w.query.to_string()).collect();
            checks.push(DoctorCheck {
                name: "probe".to_owned(),
                status: CheckStatus::Warn,
                message: format!(
                    "Windows build {}.{} ({}); degraded queries: {}",
                    facts.windows_build,
                    facts.windows_ubr,
                    facts.edition,
                    degraded.join(", ")
                ),
            });
        }
        Err(err) => checks.push(DoctorCheck {
            name: "probe".to_owned(),
            status: CheckStatus::Fail,
            message: format!("System probe unavailable: {err}"),
        }),
    }
    #[cfg(not(windows))]
    checks.push(DoctorCheck {
        name: "probe".to_owned(),
        status: CheckStatus::Warn,
        message: "System probe requires Windows".to_owned(),
    });

    let ok = !checks.iter().any(|c| c.status == CheckStatus::Fail);
    let report = DoctorReport { ok, checks };
    print_doctor(&report, json)?;
    if ok {
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::from(1))
    }
}

fn open_store(db_path: &Path) -> AppResult<Store> {
    if let Some(parent) = db_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    Store::open(db_path)
}

fn lock_path_for_db(db_path: &Path) -> PathBuf {
    let mut os = db_path.as_os_str().to_os_string();
    os.push(".lock");
    PathBuf::from(os)
}

fn open_manager(db_path: &Path) -> AppResult<TransactionManager> {
    let store = open_store(db_path)?;
    let registry = build_registry()?;
    let lock = Box::new(FileLock::new(lock_path_for_db(db_path)));
    Ok(TransactionManager::new(store, registry, lock))
}

fn build_registry() -> AppResult<ProviderRegistry> {
    let mut registry = ProviderRegistry::new();
    for provider in providers::native_providers() {
        registry.register(provider)?;
    }
    Ok(registry)
}

fn resolve_selection(
    catalog: &Catalog,
    selection: &SelectionArgs,
) -> AppResult<(Option<Profile>, Vec<TweakManifest>)> {
    if let Some(cli_profile) = selection.profile {
        let profile = Profile::from(cli_profile);
        Ok((Some(profile), catalog.by_profile(profile)))
    } else {
        let ids: Vec<TweakId> = selection.tweaks.iter().map(TweakId::new).collect();
        Ok((None, catalog.select(&ids)?))
    }
}

fn confirm_apply() -> AppResult<bool> {
    eprint!("Apply these changes? [y/N]: ");
    io::stderr().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let trimmed = input.trim();
    Ok(trimmed.eq_ignore_ascii_case("y") || trimmed.eq_ignore_ascii_case("yes"))
}

fn emit_probe_warnings(warnings: &[ProbeWarning]) {
    for warning in warnings {
        eprintln!("warning: {warning}");
    }
}

#[cfg(windows)]
fn scan_facts_with_warnings() -> AppResult<(SystemFacts, Vec<ProbeWarning>)> {
    let probe = system_probe::SystemProbe::new(Box::new(providers::WindowsProbeSource::new()));
    probe.facts_with_warnings()
}

#[cfg(not(windows))]
fn scan_facts_with_warnings() -> AppResult<(SystemFacts, Vec<ProbeWarning>)> {
    Err(AppError::UnsupportedPlatform {
        reason: "system probe requires Windows 11".to_owned(),
    })
}

#[cfg(windows)]
fn collect_metrics(phase: MetricPhase) -> AppResult<Vec<MetricSample>> {
    let probe = system_probe::SystemProbe::new(Box::new(providers::WindowsProbeSource::new()));
    probe.collect_metrics(phase, system_now_ms())
}

#[cfg(not(windows))]
fn collect_metrics(_phase: MetricPhase) -> AppResult<Vec<MetricSample>> {
    Err(AppError::UnsupportedPlatform {
        reason: "metric collection requires Windows 11".to_owned(),
    })
}

#[cfg_attr(not(windows), allow(dead_code))]
fn system_now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}
