//! Integration tests invoking the compiled `opti-cli` binary.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use core_engine::Store;
use core_engine::store::NewTransaction;
use serde_json::Value;
use shared_types::{TransactionId, TransactionStatus, TweakId};

static DIR_COUNTER: AtomicU32 = AtomicU32::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let n = DIR_COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("opti-cli-{name}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn opti_cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_opti-cli"))
}

fn valid_manifest(id: &str, category: &str, profiles: &[&str]) -> String {
    let profiles_list: Vec<String> = profiles.iter().map(|p| format!("\"{p}\"")).collect();
    format!(
        r#"
schema_version = 1
id = "{id}"
title = "Test tweak {id}"
description = "Validates {id}"
category = "{category}"
risk = "low"
side_effects = "None"
min_windows_build = 22000
profiles = [{}]

[[actions]]
provider = "registry"
hive = "HKLM"
key = 'SOFTWARE\Policies\WinOptSuiteTest\{id}'
value_name = "TestValue"
value = {{ type = "dword", data = 1 }}
"#,
        profiles_list.join(", ")
    )
}

#[test]
fn validate_valid_and_invalid_manifests() {
    let dir = TempDir::new("validate");
    let manifests = dir.0.join("manifests");
    fs::create_dir_all(&manifests).unwrap();
    fs::write(
        manifests.join("a.toml"),
        valid_manifest("privacy.test-a", "privacy", &["safe", "privacy"]),
    )
    .unwrap();
    fs::write(
        manifests.join("b.toml"),
        valid_manifest("gaming.test-b", "gaming", &["gaming"]),
    )
    .unwrap();

    let ok_out = opti_cli()
        .args(["validate", "--manifests"])
        .arg(&manifests)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(
        ok_out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&ok_out.stderr)
    );
    let json: Value = serde_json::from_slice(&ok_out.stdout).unwrap();
    assert_eq!(json["total_tweaks"], 2);
    assert_eq!(json["by_category"]["privacy"], 1);
    assert_eq!(json["by_category"]["gaming"], 1);
    assert_eq!(json["by_profile"]["safe"], 1);
    assert_eq!(json["by_profile"]["privacy"], 1);
    assert_eq!(json["by_profile"]["gaming"], 1);

    // Add an invalid manifest (below Windows 11 build + forbidden Defender target).
    let invalid_toml = r#"
schema_version = 1
id = "security.bad-tweak"
title = "Bad"
description = "Bad"
category = "security"
risk = "high"
side_effects = "Breaks things"
min_windows_build = 19045

[[actions]]
provider = "service"
name = "WinDefend"
start_type = "disabled"
"#;
    fs::write(manifests.join("c.toml"), invalid_toml).unwrap();

    let bad_out = opti_cli()
        .args(["validate", "--manifests"])
        .arg(&manifests)
        .output()
        .unwrap();
    assert_eq!(bad_out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&bad_out.stderr);
    assert!(
        stderr.contains("19045") && stderr.contains("forbidden target"),
        "expected every manifest issue in stderr, got: {stderr}"
    );
}

#[test]
fn history_and_doctor_on_temp_db() {
    let dir = TempDir::new("history-doctor");
    let db = dir.0.join("nested").join("state.db");
    let manifests = dir.0.join("manifests");
    fs::create_dir_all(&manifests).unwrap();
    fs::write(
        manifests.join("a.toml"),
        valid_manifest("privacy.test-a", "privacy", &["safe"]),
    )
    .unwrap();

    let history_out = opti_cli()
        .args(["history", "--db"])
        .arg(&db)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(history_out.status.code(), Some(0));
    let history_json: Value = serde_json::from_slice(&history_out.stdout).unwrap();
    assert_eq!(history_json, Value::Array(vec![]));
    assert!(db.exists(), "history must create parent dir and state.db");

    let doctor_out = opti_cli()
        .args(["doctor", "--db"])
        .arg(&db)
        .arg("--manifests")
        .arg(&manifests)
        .arg("--json")
        .output()
        .unwrap();
    let doctor_json: Value = serde_json::from_slice(&doctor_out.stdout).unwrap();
    let checks = doctor_json["checks"].as_array().unwrap();
    assert_eq!(checks.len(), 6);

    #[cfg(not(windows))]
    {
        // Off Windows, doctor reports platform unsupported as `fail` (exit code 1).
        assert_eq!(doctor_out.status.code(), Some(1));
        assert_eq!(doctor_json["ok"], false);
        assert_eq!(checks[0]["name"], "platform");
        assert_eq!(checks[0]["status"], "fail");
    }
    #[cfg(windows)]
    {
        assert_eq!(doctor_out.status.code(), Some(0));
        assert_eq!(doctor_json["ok"], true);
    }
}

#[test]
fn drift_on_empty_db_is_clean() {
    let dir = TempDir::new("drift");
    let db = dir.0.join("state.db");

    let out = opti_cli()
        .args(["drift", "--db"])
        .arg(&db)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["checked"], 0);
    assert_eq!(json["drifted"], Value::Array(vec![]));
    assert_eq!(json["errors"], Value::Array(vec![]));
}

#[cfg(not(windows))]
#[test]
fn scan_returns_unsupported_platform_off_windows() {
    let out = opti_cli().args(["scan", "--json"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err: Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(err["code"], "unsupported_platform");
}

#[test]
fn startup_warns_on_incomplete_transactions_except_recover() {
    let dir = TempDir::new("startup-warn");
    let db = dir.0.join("state.db");
    {
        let store = Store::open(&db).unwrap();
        store
            .insert_transaction(&NewTransaction {
                id: TransactionId::new("tx-incomplete-1"),
                created_at_ms: 1_000,
                status: TransactionStatus::Applying,
                profile: None,
                tweak_ids: vec![TweakId::new("privacy.test-a")],
                windows_build: 26100,
                windows_ubr: 1,
                app_version: "0.1.0".into(),
            })
            .unwrap();
    }

    let history_out = opti_cli()
        .args(["history", "--db"])
        .arg(&db)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(history_out.status.code(), Some(0));
    let history_stderr = String::from_utf8_lossy(&history_out.stderr);
    assert!(
        history_stderr.contains("WARNING") && history_stderr.contains("tx-incomplete-1"),
        "expected prominent startup warning on stderr, got: {history_stderr}"
    );
    let history_json: Value = serde_json::from_slice(&history_out.stdout).unwrap();
    assert_eq!(history_json.as_array().unwrap().len(), 1);

    let recover_out = opti_cli()
        .args(["recover", "--db"])
        .arg(&db)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(recover_out.status.code(), Some(0));
    let recover_stderr = String::from_utf8_lossy(&recover_out.stderr);
    assert!(
        !recover_stderr.contains("WARNING"),
        "recover must not emit the startup warning: {recover_stderr}"
    );
    let recover_json: Value = serde_json::from_slice(&recover_out.stdout).unwrap();
    assert_eq!(recover_json.as_array().unwrap().len(), 1);
    assert_eq!(recover_json[0]["id"], "tx-incomplete-1");
}
