//! # desktop
//!
//! Thin Tauri v2 desktop shell for WinOptSuite (ADR-0003). Wires
//! `core-engine`, `providers`, and `system-probe` together and exposes typed
//! IPC commands with automatic `tauri-specta` TypeScript bindings generation.
#![forbid(unsafe_code)]

pub mod commands;
pub mod state;

use std::path::PathBuf;

use tauri::Manager;
use tauri_specta::{Builder, ErrorHandlingMode, collect_commands};

use crate::state::{AppState, parse_db_arg, resolve_db_path, resolve_manifests_dir};

/// Constructs the `tauri-specta` builder with all registered IPC commands.
pub fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            commands::system_facts,
            commands::list_tweaks,
            commands::dry_run,
            commands::apply,
            commands::rollback,
            commands::history,
            commands::incomplete_transactions,
            commands::recover,
            commands::detect_drift,
            commands::transaction_metrics,
        ])
        .error_handling(ErrorHandlingMode::Result)
        .dangerously_cast_bigints_to_number()
}

/// Path to the generated TypeScript IPC bindings file.
pub fn bindings_output_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("src")
        .join("bindings.ts")
}

/// Launches the WinOptSuite Tauri v2 desktop application.
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let builder = specta_builder();

    #[cfg(debug_assertions)]
    {
        let out_path = bindings_output_path();
        if out_path.parent().is_some_and(std::path::Path::is_dir) {
            builder.export(specta_typescript::Typescript::default(), &out_path)?;
        }
    }

    let explicit_db = parse_db_arg(std::env::args_os());

    tauri::Builder::default()
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            let app_data_dir = app.path().app_local_data_dir().ok();
            let resource_dir = app.path().resource_dir().ok();

            let db_path = resolve_db_path(explicit_db.clone(), app_data_dir);
            let manifests_dir = resolve_manifests_dir(resource_dir);

            let state = AppState::open(&db_path, manifests_dir)?;
            app.manage(state);
            Ok(())
        })
        .run(tauri::generate_context!())?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use core_engine::{DriftDetector, FileLock, ProviderRegistry, Store, TransactionManager};
    use shared_types::{MetricKind, MetricPhase, MetricSample, TransactionId};

    use super::*;
    use crate::state::lock_manager;

    #[test]
    fn export_bindings() {
        let out_path = bindings_output_path();
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        specta_builder()
            .export(specta_typescript::Typescript::default(), &out_path)
            .unwrap();
        assert!(out_path.is_file());
    }

    #[test]
    fn app_state_store_operations_work_cross_platform() {
        let store = Store::open_in_memory().unwrap();
        let registry = ProviderRegistry::new();
        let lock_path = std::env::temp_dir().join(format!(
            "winoptsuite-desktop-test-{}.lock",
            std::process::id()
        ));
        let manager = TransactionManager::new(store, registry, Box::new(FileLock::new(&lock_path)));
        let state = AppState::from_parts(manager, resolve_manifests_dir(None));

        let mgr = state.manager();
        let guard = lock_manager(&mgr).unwrap();
        assert!(guard.history().unwrap().is_empty());
        assert!(guard.find_incomplete().unwrap().is_empty());

        let drift = DriftDetector::new(guard.store(), guard.registry())
            .detect()
            .unwrap();
        assert_eq!(drift.checked, 0);
        assert!(drift.is_clean());

        let tx_id = TransactionId::new("tx-metrics-test");
        assert!(guard.store().metrics(Some(&tx_id)).unwrap().is_empty());

        // Standalone metric round-trip verifies MetricSample store integration.
        let sample = MetricSample {
            kind: MetricKind::RunningProcesses,
            phase: MetricPhase::Before,
            value: 112.0,
            captured_at_unix_ms: 1_700_000_000_000,
        };
        guard.store().insert_metric(None, &sample).unwrap();
        let stored = guard.store().metrics(None).unwrap();
        assert_eq!(stored, vec![sample]);

        let _ = std::fs::remove_file(lock_path);
    }
}
