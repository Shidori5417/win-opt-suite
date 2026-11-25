//! Declarative tweak manifests (`manifests/tweaks/*.toml`).

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::action::{ActionSpec, FirewallAction, PowerPlanAction, is_valid_guid};
use crate::error::AppError;
use crate::ids::TweakId;
use crate::policy;

/// The only manifest schema version this build understands.
pub const CURRENT_MANIFEST_SCHEMA_VERSION: u32 = 1;

/// First Windows 11 build (21H2). Manifests may not target older builds.
pub const WINDOWS_11_MIN_BUILD: u32 = 22000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum TweakCategory {
    Privacy,
    Telemetry,
    Performance,
    Gaming,
    Power,
    Network,
    Interface,
    Services,
    Updates,
    Security,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

/// Environment facts a tweak can be excluded on (see [`crate::SystemFacts::environment_tags`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum EnvironmentTag {
    Laptop,
    Desktop,
    OnBattery,
    VirtualMachine,
    DomainJoined,
    AntiCheatVanguard,
    AntiCheatFaceit,
    AntiCheatEasyAntiCheat,
    AntiCheatBattlEye,
}

/// Curated profile a tweak belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Profile {
    Safe,
    Privacy,
    Gaming,
    Laptop,
}

/// One declarative tweak. Unknown fields are rejected so that typos in TOML
/// never silently disable a safety property.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(deny_unknown_fields)]
pub struct TweakManifest {
    pub schema_version: u32,
    pub id: TweakId,
    pub title: String,
    pub description: String,
    pub category: TweakCategory,
    pub risk: RiskLevel,
    /// Mandatory, honest description of user-visible side effects.
    pub side_effects: String,
    pub min_windows_build: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_windows_build: Option<u32>,
    #[serde(default)]
    pub excluded_on: Vec<EnvironmentTag>,
    #[serde(default)]
    pub conflicts_with: Vec<TweakId>,
    #[serde(default)]
    pub profiles: Vec<Profile>,
    #[serde(default)]
    pub requires_reboot: bool,
    /// Documentation links (preferably Microsoft GPO/CSP references).
    #[serde(default)]
    pub references: Vec<String>,
    pub actions: Vec<ActionSpec>,
}

/// A single validation problem in a manifest.
#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "issue", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ManifestIssue {
    #[error("unsupported schema_version {found} (expected {expected})")]
    UnsupportedSchemaVersion { found: u32, expected: u32 },
    #[error("invalid tweak id format")]
    InvalidId,
    #[error("field `{field}` must not be empty")]
    EmptyField { field: String },
    #[error("min_windows_build {found} is below Windows 11 ({WINDOWS_11_MIN_BUILD})")]
    BuildBelowWindows11 { found: u32 },
    #[error("max_windows_build is lower than min_windows_build")]
    InvalidBuildRange,
    #[error("manifest declares no actions")]
    NoActions,
    #[error("tweak lists itself in conflicts_with")]
    SelfConflict,
    #[error("action #{index}: registry action needs exactly one of `value` or `delete = true`")]
    AmbiguousRegistryAction { index: usize },
    #[error("action #{index}: empty resource name/path")]
    EmptyResourceName { index: usize },
    #[error("action #{index}: invalid GUID `{value}`")]
    InvalidGuid { index: usize, value: String },
    #[error("resource `{resource}` is targeted by more than one action")]
    DuplicateResource { resource: String },
    #[error("action #{index}: forbidden target ({reason})")]
    ForbiddenTarget { index: usize, reason: String },
    #[error("file could not be parsed: {message}")]
    Unparseable { message: String },
    #[error("tweak id is defined more than once in the catalog")]
    DuplicateId,
    #[error("conflicts_with references unknown tweak `{other}`")]
    UnknownConflictReference { other: String },
}

impl TweakManifest {
    /// Validates every rule that can be checked without touching the system.
    /// Returns all issues at once.
    pub fn validate(&self) -> Result<(), AppError> {
        let issues = self.issues();
        if issues.is_empty() {
            Ok(())
        } else {
            Err(AppError::InvalidManifest {
                id: self.id.to_string(),
                issues,
            })
        }
    }

    /// Collects all validation issues (empty when valid).
    pub fn issues(&self) -> Vec<ManifestIssue> {
        let mut issues = Vec::new();

        if self.schema_version != CURRENT_MANIFEST_SCHEMA_VERSION {
            issues.push(ManifestIssue::UnsupportedSchemaVersion {
                found: self.schema_version,
                expected: CURRENT_MANIFEST_SCHEMA_VERSION,
            });
        }
        if !self.id.is_valid() {
            issues.push(ManifestIssue::InvalidId);
        }
        for (field, value) in [
            ("title", &self.title),
            ("description", &self.description),
            ("side_effects", &self.side_effects),
        ] {
            if value.trim().is_empty() {
                issues.push(ManifestIssue::EmptyField {
                    field: field.to_owned(),
                });
            }
        }
        if self.min_windows_build < WINDOWS_11_MIN_BUILD {
            issues.push(ManifestIssue::BuildBelowWindows11 {
                found: self.min_windows_build,
            });
        }
        if self
            .max_windows_build
            .is_some_and(|max| max < self.min_windows_build)
        {
            issues.push(ManifestIssue::InvalidBuildRange);
        }
        if self.conflicts_with.contains(&self.id) {
            issues.push(ManifestIssue::SelfConflict);
        }
        if self.actions.is_empty() {
            issues.push(ManifestIssue::NoActions);
        }

        let mut seen = HashSet::new();
        for (index, action) in self.actions.iter().enumerate() {
            validate_action(index, action, &mut issues);
            if let Some(reason) = policy::forbidden_reason(action) {
                issues.push(ManifestIssue::ForbiddenTarget {
                    index,
                    reason: reason.to_owned(),
                });
            }
            let resource = action.resource_id();
            if !seen.insert(resource.clone()) {
                issues.push(ManifestIssue::DuplicateResource {
                    resource: resource.to_string(),
                });
            }
        }
        issues
    }

    /// Whether the tweak is applicable on the given Windows build.
    pub fn supports_build(&self, build: u32) -> bool {
        build >= self.min_windows_build && self.max_windows_build.is_none_or(|max| build <= max)
    }
}

fn validate_action(index: usize, action: &ActionSpec, issues: &mut Vec<ManifestIssue>) {
    let empty = |s: &str| s.trim().is_empty();
    let mut check_guid = |value: &str| {
        if !is_valid_guid(value) {
            issues.push(ManifestIssue::InvalidGuid {
                index,
                value: value.to_owned(),
            });
        }
    };
    let mut empty_name = false;
    let mut ambiguous = false;
    match action {
        ActionSpec::Registry(a) => {
            empty_name = empty(&a.key);
            ambiguous = a.delete == a.value.is_some();
        }
        ActionSpec::Service(a) => empty_name = empty(&a.name),
        ActionSpec::ScheduledTask(a) => empty_name = empty(&a.path),
        ActionSpec::PowerPlan(PowerPlanAction::ActivateScheme { scheme_guid }) => {
            check_guid(scheme_guid);
        }
        ActionSpec::PowerPlan(PowerPlanAction::SetValue {
            scheme_guid,
            subgroup_guid,
            setting_guid,
            ..
        }) => {
            if let Some(g) = scheme_guid {
                check_guid(g);
            }
            check_guid(subgroup_guid);
            check_guid(setting_guid);
        }
        ActionSpec::Firewall(FirewallAction::SetRuleEnabled { rule_name, .. }) => {
            empty_name = empty(rule_name);
        }
        ActionSpec::Firewall(FirewallAction::EnsureRule { rule }) => {
            empty_name = empty(&rule.name);
        }
    }
    if empty_name {
        issues.push(ManifestIssue::EmptyResourceName { index });
    }
    if ambiguous {
        issues.push(ManifestIssue::AmbiguousRegistryAction { index });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::{RegistryHive, RegistryValue};

    const SAMPLE: &str = r#"
schema_version = 1
id = "privacy.disable-telemetry"
title = "Limit diagnostic data"
description = "Sets the AllowTelemetry policy to the minimum allowed level."
category = "privacy"
risk = "low"
side_effects = "Windows Insider builds and some feedback features stop working."
min_windows_build = 22000
excluded_on = ["domain_joined"]
conflicts_with = ["privacy.full-telemetry"]
profiles = ["safe", "privacy"]
references = ["https://learn.microsoft.com/windows/client-management/mdm/policy-csp-system#allowtelemetry"]

[[actions]]
provider = "registry"
hive = "HKLM"
key = 'SOFTWARE\Policies\Microsoft\Windows\DataCollection'
value_name = "AllowTelemetry"
value = { type = "dword", data = 0 }

[[actions]]
provider = "service"
name = "DiagTrack"
start_type = "disabled"

[[actions]]
provider = "scheduled_task"
path = '\Microsoft\Windows\Application Experience\ProgramDataUpdater'
enabled = false

[[actions]]
provider = "power_plan"
op = "set_value"
subgroup_guid = "238c9fa8-0aad-41ed-83f4-97be242c8f20"
setting_guid = "29f6c1db-86da-48c5-9fdb-f2b67b1f44da"
ac_value = 0
dc_value = 900

[[actions]]
provider = "firewall"
op = "set_rule_enabled"
rule_name = "WinOptSuite-Example"
enabled = false
"#;

    #[test]
    fn parses_and_validates_sample_toml() {
        let m: TweakManifest = toml::from_str(SAMPLE).unwrap();
        assert_eq!(m.actions.len(), 5);
        m.validate().unwrap();
        match &m.actions[0] {
            ActionSpec::Registry(a) => {
                assert_eq!(a.hive, RegistryHive::LocalMachine);
                assert_eq!(a.value, Some(RegistryValue::Dword(0)));
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(m.supports_build(26100));
        assert!(!m.supports_build(19045));
    }

    #[test]
    fn json_roundtrip_is_lossless() {
        let m: TweakManifest = toml::from_str(SAMPLE).unwrap();
        let json = serde_json::to_string(&m).unwrap();
        let back: TweakManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn rejects_unknown_fields() {
        let bad = SAMPLE.replace("risk = \"low\"", "risk = \"low\"\nrisky = true");
        assert!(toml::from_str::<TweakManifest>(&bad).is_err());
        let bad_action = SAMPLE.replace(
            "start_type = \"disabled\"",
            "start_type = \"disabled\"\nstartup = 1",
        );
        assert!(toml::from_str::<TweakManifest>(&bad_action).is_err());
    }

    #[test]
    fn reports_all_issues() {
        let mut m: TweakManifest = toml::from_str(SAMPLE).unwrap();
        m.schema_version = 2;
        m.id = TweakId::new("Bad Id");
        m.side_effects = "  ".into();
        m.min_windows_build = 19045;
        m.max_windows_build = Some(100);
        if let ActionSpec::Registry(a) = &mut m.actions[0] {
            a.delete = true; // both value and delete -> ambiguous
        }
        let dup = m.actions[1].clone();
        m.actions.push(dup);
        let issues = m.issues();
        assert!(issues.contains(&ManifestIssue::UnsupportedSchemaVersion {
            found: 2,
            expected: 1
        }));
        assert!(issues.contains(&ManifestIssue::InvalidId));
        assert!(issues.contains(&ManifestIssue::EmptyField {
            field: "side_effects".into()
        }));
        assert!(issues.contains(&ManifestIssue::BuildBelowWindows11 { found: 19045 }));
        assert!(issues.contains(&ManifestIssue::InvalidBuildRange));
        assert!(issues.contains(&ManifestIssue::AmbiguousRegistryAction { index: 0 }));
        assert!(issues.contains(&ManifestIssue::DuplicateResource {
            resource: "service:diagtrack".into()
        }));
        assert!(matches!(
            m.validate(),
            Err(AppError::InvalidManifest { .. })
        ));
    }

    #[test]
    fn rejects_defender_and_placebo_targets() {
        let defender = SAMPLE.replace("name = \"DiagTrack\"", "name = \"WinDefend\"");
        let m: TweakManifest = toml::from_str(&defender).unwrap();
        assert!(
            m.issues()
                .iter()
                .any(|i| matches!(i, ManifestIssue::ForbiddenTarget { index: 1, .. }))
        );

        let placebo = SAMPLE.replace(
            "value_name = \"AllowTelemetry\"",
            "value_name = \"NetworkThrottlingIndex\"",
        );
        let m: TweakManifest = toml::from_str(&placebo).unwrap();
        assert!(
            m.issues()
                .iter()
                .any(|i| matches!(i, ManifestIssue::ForbiddenTarget { index: 0, .. }))
        );
    }
}
