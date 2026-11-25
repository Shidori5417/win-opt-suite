//! Thin Tauri IPC command handlers delegating directly to `core-engine`,
//! `system-probe`, and `providers`. Contains no business logic.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use core_engine::{Catalog, DriftDetector};
use serde::{Deserialize, Serialize};
use shared_types::{
    AppError, AppResult, DriftReport, DryRunDiff, MetricPhase, MetricSample, Profile, SystemFacts,
    TransactionId, TransactionStatus, TransactionSummary, TweakId, TweakManifest,
};
use tauri::State;

use crate::state::{AppState, lock_manager};

/// Selection passed to `dry_run` and `apply`: either a curated profile or an
/// explicit list of tweak identifiers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Selection {
    Profile { profile: Profile },
    Tweaks { ids: Vec<TweakId> },
}

async fn run_blocking<T, F>(task: F) -> AppResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> AppResult<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|err| AppError::Internal {
            message: format!("background worker failed: {err}"),
        })?
}

pub(crate) fn resolve_selection(
    catalog: &Catalog,
    selection: &Selection,
) -> AppResult<(Option<Profile>, Vec<TweakManifest>)> {
    match selection {
        Selection::Profile { profile } => Ok((Some(*profile), catalog.by_profile(*profile))),
        Selection::Tweaks { ids } => Ok((None, catalog.select(ids)?)),
    }
}

pub(crate) fn load_catalog_manifests(manifests_dir: &Path) -> AppResult<Vec<TweakManifest>> {
    let catalog = Catalog::load_dir(manifests_dir)?;
    Ok(catalog.iter().cloned().collect())
}

#[cfg(windows)]
pub(crate) fn scan_facts() -> AppResult<SystemFacts> {
    let probe = system_probe::SystemProbe::new(Box::new(providers::WindowsProbeSource::new()));
    probe.facts()
}

#[cfg(not(windows))]
pub(crate) fn scan_facts() -> AppResult<SystemFacts> {
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

/// Collects read-only hardware, OS, environment, and anti-cheat facts.
#[tauri::command]
#[specta::specta]
pub async fn system_facts() -> AppResult<SystemFacts> {
    run_blocking(scan_facts).await
}

/// Loads and returns all validated tweak manifests from the catalog directory.
#[tauri::command]
#[specta::specta]
pub async fn list_tweaks(state: State<'_, AppState>) -> AppResult<Vec<TweakManifest>> {
    let manifests_dir = state.manifests_dir();
    run_blocking(move || load_catalog_manifests(&manifests_dir)).await
}

/// Builds a read-only dry-run diff and conflict report for `selection`.
#[tauri::command]
#[specta::specta]
pub async fn dry_run(selection: Selection, state: State<'_, AppState>) -> AppResult<DryRunDiff> {
    let manager = state.manager();
    let manifests_dir = state.manifests_dir();
    run_blocking(move || {
        let facts = scan_facts()?;
        let catalog = Catalog::load_dir(&manifests_dir)?;
        let (_, manifests) = resolve_selection(&catalog, &selection)?;
        let guard = lock_manager(&manager)?;
        guard.planner().plan(&manifests, &facts)
    })
    .await
}

/// Applies `selection` as one atomic, rollback-capable transaction and records
/// before/after metric baselines.
#[tauri::command]
#[specta::specta]
pub async fn apply(
    selection: Selection,
    profile: Option<Profile>,
    state: State<'_, AppState>,
) -> AppResult<TransactionId> {
    let manager = state.manager();
    let manifests_dir = state.manifests_dir();
    run_blocking(move || {
        let facts = scan_facts()?;
        let catalog = Catalog::load_dir(&manifests_dir)?;
        let (inferred_profile, manifests) = resolve_selection(&catalog, &selection)?;
        let effective_profile = profile.or(inferred_profile);

        let before_metrics = collect_metrics(MetricPhase::Before)?;
        let guard = lock_manager(&manager)?;
        let tx_id = guard.apply(&manifests, effective_profile, &facts)?;
        let after_metrics = collect_metrics(MetricPhase::After)?;

        for sample in before_metrics.iter().chain(after_metrics.iter()) {
            guard.store().insert_metric(Some(&tx_id), sample)?;
        }
        Ok(tx_id)
    })
    .await
}

/// Rolls back a `COMMITTED` transaction in reverse step order.
#[tauri::command]
#[specta::specta]
pub async fn rollback(id: TransactionId, state: State<'_, AppState>) -> AppResult<()> {
    let manager = state.manager();
    run_blocking(move || {
        let guard = lock_manager(&manager)?;
        guard.rollback(&id)
    })
    .await
}

/// Lists all recorded transactions, newest first.
#[tauri::command]
#[specta::specta]
pub async fn history(state: State<'_, AppState>) -> AppResult<Vec<TransactionSummary>> {
    let manager = state.manager();
    run_blocking(move || {
        let guard = lock_manager(&manager)?;
        guard.history()
    })
    .await
}

/// Lists incomplete transactions (`PENDING`, `APPLYING`, `ROLLING_BACK`) that
/// require crash recovery before new changes can be applied.
#[tauri::command]
#[specta::specta]
pub async fn incomplete_transactions(
    state: State<'_, AppState>,
) -> AppResult<Vec<TransactionSummary>> {
    let manager = state.manager();
    run_blocking(move || {
        let guard = lock_manager(&manager)?;
        guard.find_incomplete()
    })
    .await
}

/// Runs startup crash recovery on an incomplete (or `FAILED`) transaction.
#[tauri::command]
#[specta::specta]
pub async fn recover(
    id: TransactionId,
    state: State<'_, AppState>,
) -> AppResult<TransactionStatus> {
    let manager = state.manager();
    run_blocking(move || {
        let guard = lock_manager(&manager)?;
        guard.recover(&id)
    })
    .await
}

/// Compares live system state against the latest `COMMITTED` baseline.
#[tauri::command]
#[specta::specta]
pub async fn detect_drift(state: State<'_, AppState>) -> AppResult<DriftReport> {
    let manager = state.manager();
    run_blocking(move || {
        let guard = lock_manager(&manager)?;
        DriftDetector::new(guard.store(), guard.registry()).detect()
    })
    .await
}

/// Returns recorded before/after metric samples for a single transaction.
#[tauri::command]
#[specta::specta]
pub async fn transaction_metrics(
    id: TransactionId,
    state: State<'_, AppState>,
) -> AppResult<Vec<MetricSample>> {
    let manager = state.manager();
    run_blocking(move || {
        let guard = lock_manager(&manager)?;
        guard.store().metrics(Some(&id))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::resolve_manifests_dir;

    #[test]
    fn loads_real_catalog_and_resolves_selections() {
        let dir = resolve_manifests_dir(None);
        let manifests = load_catalog_manifests(&dir).unwrap();
        assert!(!manifests.is_empty());

        let catalog = Catalog::load_dir(&dir).unwrap();
        let (profile, safe) = resolve_selection(
            &catalog,
            &Selection::Profile {
                profile: Profile::Safe,
            },
        )
        .unwrap();
        assert_eq!(profile, Some(Profile::Safe));
        assert!(!safe.is_empty());

        let first_id = manifests[0].id.clone();
        let (none_profile, explicit) = resolve_selection(
            &catalog,
            &Selection::Tweaks {
                ids: vec![first_id.clone()],
            },
        )
        .unwrap();
        assert_eq!(none_profile, None);
        assert_eq!(explicit.len(), 1);
        assert_eq!(explicit[0].id, first_id);
    }

    #[cfg(not(windows))]
    #[test]
    fn scan_facts_returns_unsupported_platform_off_windows() {
        let err = scan_facts().unwrap_err();
        assert!(matches!(err, AppError::UnsupportedPlatform { .. }));
    }
}
