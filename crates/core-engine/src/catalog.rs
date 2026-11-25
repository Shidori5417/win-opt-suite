//! Tweak catalog: loads, validates and indexes `manifests/tweaks/*.toml`.
//!
//! Loading is all-or-nothing: if any manifest is invalid the whole catalog is
//! rejected, so a broken file can never silently drop a safety property.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use shared_types::{AppError, AppResult, ManifestIssue, Profile, TweakId, TweakManifest};

/// Validated, id-indexed set of tweak manifests (deterministic order).
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    tweaks: BTreeMap<TweakId, TweakManifest>,
}

impl Catalog {
    /// Loads every `*.toml` file in `dir` (non-recursive, sorted by name).
    pub fn load_dir(dir: &Path) -> AppResult<Self> {
        let mut paths = Vec::new();
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "toml") && path.is_file() {
                paths.push(path);
            }
        }
        paths.sort();

        let mut manifests = Vec::with_capacity(paths.len());
        for path in paths {
            let text = fs::read_to_string(&path)?;
            let manifest = parse_manifest(&text).map_err(|message| AppError::InvalidManifest {
                id: path.display().to_string(),
                issues: vec![ManifestIssue::Unparseable { message }],
            })?;
            manifests.push(manifest);
        }
        Self::from_manifests(manifests)
    }

    /// Validates each manifest, id uniqueness and `conflicts_with` references.
    pub fn from_manifests(manifests: Vec<TweakManifest>) -> AppResult<Self> {
        let mut tweaks = BTreeMap::new();
        for manifest in manifests {
            manifest.validate()?;
            let id = manifest.id.clone();
            if tweaks.insert(id.clone(), manifest).is_some() {
                return Err(AppError::InvalidManifest {
                    id: id.to_string(),
                    issues: vec![ManifestIssue::DuplicateId],
                });
            }
        }
        for manifest in tweaks.values() {
            let unknown: Vec<ManifestIssue> = manifest
                .conflicts_with
                .iter()
                .filter(|other| !tweaks.contains_key(*other))
                .map(|other| ManifestIssue::UnknownConflictReference {
                    other: other.to_string(),
                })
                .collect();
            if !unknown.is_empty() {
                return Err(AppError::InvalidManifest {
                    id: manifest.id.to_string(),
                    issues: unknown,
                });
            }
        }
        Ok(Self { tweaks })
    }

    pub fn len(&self) -> usize {
        self.tweaks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tweaks.is_empty()
    }

    pub fn get(&self, id: &TweakId) -> Option<&TweakManifest> {
        self.tweaks.get(id)
    }

    /// All manifests ordered by id.
    pub fn iter(&self) -> impl Iterator<Item = &TweakManifest> {
        self.tweaks.values()
    }

    /// Manifests that belong to `profile`, ordered by id.
    pub fn by_profile(&self, profile: Profile) -> Vec<TweakManifest> {
        self.iter()
            .filter(|m| m.profiles.contains(&profile))
            .cloned()
            .collect()
    }

    /// Resolves explicit ids (in the given order); unknown id → `NotFound`.
    pub fn select(&self, ids: &[TweakId]) -> AppResult<Vec<TweakManifest>> {
        ids.iter()
            .map(|id| {
                self.get(id).cloned().ok_or_else(|| AppError::NotFound {
                    resource: format!("tweak:{id}"),
                })
            })
            .collect()
    }
}

/// Parses one manifest from TOML text.
pub fn parse_manifest(text: &str) -> Result<TweakManifest, String> {
    toml::from_str(text).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(id: &str, conflicts: &[&str], profiles: &str) -> String {
        let conflicts: Vec<String> = conflicts.iter().map(|c| format!("\"{c}\"")).collect();
        format!(
            r#"
schema_version = 1
id = "{id}"
title = "t"
description = "d"
category = "privacy"
risk = "low"
side_effects = "none"
min_windows_build = 22000
conflicts_with = [{}]
profiles = [{profiles}]

[[actions]]
provider = "registry"
hive = "HKLM"
key = 'SOFTWARE\Policies\Test\{id}'
value_name = "V"
value = {{ type = "dword", data = 1 }}
"#,
            conflicts.join(",")
        )
    }

    fn tmpdir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("catalog-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn loads_dir_sorted_and_filters_by_profile() {
        let dir = tmpdir("ok");
        fs::write(
            dir.join("b.toml"),
            manifest("test.b", &["test.a"], "\"safe\""),
        )
        .unwrap();
        fs::write(dir.join("a.toml"), manifest("test.a", &[], "\"gaming\"")).unwrap();
        fs::write(dir.join("README.md"), "ignored").unwrap();
        let c = Catalog::load_dir(&dir).unwrap();
        assert_eq!(c.len(), 2);
        let ids: Vec<&str> = c.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["test.a", "test.b"]);
        assert_eq!(c.by_profile(Profile::Safe).len(), 1);
        assert!(c.select(&[TweakId::new("test.missing")]).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rejects_duplicates_unknown_refs_and_garbage() {
        let a = parse_manifest(&manifest("test.a", &[], "")).unwrap();
        let err = Catalog::from_manifests(vec![a.clone(), a]).unwrap_err();
        assert!(
            matches!(err, AppError::InvalidManifest { ref issues, .. } if issues == &[ManifestIssue::DuplicateId])
        );

        let b = parse_manifest(&manifest("test.b", &["test.zzz"], "")).unwrap();
        let err = Catalog::from_manifests(vec![b]).unwrap_err();
        assert!(matches!(err, AppError::InvalidManifest { ref issues, .. }
            if matches!(issues.as_slice(), [ManifestIssue::UnknownConflictReference { .. }])));

        let dir = tmpdir("bad");
        fs::write(dir.join("x.toml"), "schema_version = ").unwrap();
        let err = Catalog::load_dir(&dir).unwrap_err();
        assert!(matches!(err, AppError::InvalidManifest { ref issues, .. }
            if matches!(issues.as_slice(), [ManifestIssue::Unparseable { .. }])));
        fs::remove_dir_all(dir).unwrap();
    }
}
