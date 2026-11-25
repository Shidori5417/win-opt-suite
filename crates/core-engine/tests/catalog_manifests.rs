//! Validates the shipped `manifests/tweaks` catalog: it loads, every file is
//! named after its id, every profile is populated, no profile produces a
//! blocking conflict on a default desktop, and the two Delivery Optimization
//! tweaks really do conflict.

use std::fs;
use std::path::Path;

use core_engine::catalog::parse_manifest;
use core_engine::{Catalog, ConflictGuard};
use shared_types::{ConflictKind, CpuInfo, Profile, Severity, SystemFacts, TweakId};

const TWEAKS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../manifests/tweaks");

const ALL_PROFILES: [Profile; 4] = [
    Profile::Safe,
    Profile::Privacy,
    Profile::Gaming,
    Profile::Laptop,
];

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// A typical physical Windows 11 24H2 desktop: not domain-joined, no VM,
/// no anti-cheat driver, not a laptop.
fn desktop_facts() -> SystemFacts {
    SystemFacts {
        windows_build: 26100,
        windows_ubr: 0,
        windows_display_version: "24H2".into(),
        edition: "Professional".into(),
        cpu: CpuInfo {
            brand: "test cpu".into(),
            physical_cores: 8,
            logical_cores: 16,
        },
        gpus: Vec::new(),
        total_memory_bytes: 16 * 1024 * 1024 * 1024,
        is_laptop: false,
        on_battery: false,
        is_virtual_machine: false,
        is_domain_joined: false,
        anti_cheat: Vec::new(),
    }
}

fn load() -> Result<Catalog, Box<dyn std::error::Error>> {
    Ok(Catalog::load_dir(Path::new(TWEAKS_DIR))?)
}

#[test]
fn shipped_catalog_loads_with_enough_tweaks() -> TestResult {
    let catalog = load()?;
    assert!(
        catalog.len() >= 15,
        "expected at least 15 tweaks, found {}",
        catalog.len()
    );
    Ok(())
}

#[test]
fn every_profile_is_populated() -> TestResult {
    let catalog = load()?;
    for profile in ALL_PROFILES {
        assert!(
            !catalog.by_profile(profile).is_empty(),
            "profile {profile:?} has no tweaks"
        );
    }
    Ok(())
}

#[test]
fn file_name_matches_id_for_every_manifest() -> TestResult {
    let mut checked = 0usize;
    for entry in fs::read_dir(TWEAKS_DIR)? {
        let path = entry?.path();
        if path.extension().is_none_or(|e| e != "toml") {
            continue;
        }
        let text = fs::read_to_string(&path)?;
        let manifest = parse_manifest(&text)?;
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or("manifest file name is not valid UTF-8")?;
        assert_eq!(
            manifest.id.as_str(),
            stem,
            "file {} declares id {}",
            path.display(),
            manifest.id
        );
        checked += 1;
    }
    assert!(checked >= 15, "only {checked} manifest files were checked");
    Ok(())
}

#[test]
fn no_profile_has_blocking_conflicts_on_a_default_desktop() -> TestResult {
    let catalog = load()?;
    let facts = desktop_facts();
    for profile in ALL_PROFILES {
        let selection = catalog.by_profile(profile);
        let blocking: Vec<_> = ConflictGuard::check(&selection, &facts)
            .into_iter()
            .filter(|w| w.severity == Severity::Blocking)
            .collect();
        assert!(
            blocking.is_empty(),
            "profile {profile:?} has blocking conflicts: {blocking:?}"
        );
    }
    Ok(())
}

#[test]
fn delivery_optimization_pair_is_blocking() -> TestResult {
    let catalog = load()?;
    let pair = catalog.select(&[
        TweakId::new("updates.delivery-optimization-lan-only"),
        TweakId::new("updates.delivery-optimization-http-only"),
    ])?;
    let warnings = ConflictGuard::check(&pair, &desktop_facts());

    assert!(
        warnings.iter().any(|w| w.severity == Severity::Blocking),
        "expected a blocking warning, got {warnings:?}"
    );
    assert!(
        warnings
            .iter()
            .any(|w| matches!(w.kind, ConflictKind::DeclaredConflict { .. })),
        "expected a declared conflict, got {warnings:?}"
    );
    Ok(())
}
