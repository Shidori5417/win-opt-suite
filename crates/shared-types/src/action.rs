//! Declarative actions (what a tweak wants) and resource states (what the
//! system has). Both are pure data; only providers know how to realise them.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::ids::ResourceId;

/// Kind of system provider able to execute an [`ActionSpec`].
///
/// Adding a variant here (plus a provider implementation registered in the
/// `ProviderRegistry`) is the *only* change needed to support a new subsystem;
/// the transaction manager stays untouched. Requires an ADR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ProviderKind {
    Registry,
    Service,
    ScheduledTask,
    PowerPlan,
    Firewall,
}

impl ProviderKind {
    pub const ALL: [ProviderKind; 5] = [
        ProviderKind::Registry,
        ProviderKind::Service,
        ProviderKind::ScheduledTask,
        ProviderKind::PowerPlan,
        ProviderKind::Firewall,
    ];

    /// Stable storage/IPC name (matches the serde representation).
    pub const fn as_str(self) -> &'static str {
        match self {
            ProviderKind::Registry => "registry",
            ProviderKind::Service => "service",
            ProviderKind::ScheduledTask => "scheduled_task",
            ProviderKind::PowerPlan => "power_plan",
            ProviderKind::Firewall => "firewall",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == value)
    }
}

impl fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One atomic, declarative change. Serialized form (TOML/JSON) is tagged by
/// `provider`, e.g.
///
/// ```toml
/// [[actions]]
/// provider = "registry"
/// hive = "HKLM"
/// key = 'SOFTWARE\Policies\Microsoft\Windows\DataCollection'
/// value_name = "AllowTelemetry"
/// value = { type = "dword", data = 0 }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "provider", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ActionSpec {
    Registry(RegistryAction),
    Service(ServiceAction),
    ScheduledTask(ScheduledTaskAction),
    PowerPlan(PowerPlanAction),
    Firewall(FirewallAction),
}

impl ActionSpec {
    pub fn provider_kind(&self) -> ProviderKind {
        match self {
            ActionSpec::Registry(_) => ProviderKind::Registry,
            ActionSpec::Service(_) => ProviderKind::Service,
            ActionSpec::ScheduledTask(_) => ProviderKind::ScheduledTask,
            ActionSpec::PowerPlan(_) => ProviderKind::PowerPlan,
            ActionSpec::Firewall(_) => ProviderKind::Firewall,
        }
    }

    /// Canonical identity of the resource this action touches. Names that are
    /// case-insensitive on Windows (registry paths, service names, task paths,
    /// GUIDs) are lowercased so that contention is detected reliably.
    pub fn resource_id(&self) -> ResourceId {
        let kind = self.provider_kind().as_str();
        let body = match self {
            ActionSpec::Registry(a) => format!(
                "{}\\{}\\{}",
                a.hive.as_str(),
                a.key.trim_matches('\\'),
                a.value_name
            )
            .to_lowercase(),
            ActionSpec::Service(a) => a.name.to_lowercase(),
            ActionSpec::ScheduledTask(a) => a.path.to_lowercase(),
            ActionSpec::PowerPlan(PowerPlanAction::ActivateScheme { .. }) => {
                "active_scheme".to_owned()
            }
            ActionSpec::PowerPlan(PowerPlanAction::SetValue {
                scheme_guid,
                subgroup_guid,
                setting_guid,
                ..
            }) => format!(
                "{}/{}/{}",
                scheme_guid.as_deref().unwrap_or("active"),
                subgroup_guid,
                setting_guid
            )
            .to_lowercase(),
            ActionSpec::Firewall(FirewallAction::SetRuleEnabled { rule_name, .. }) => {
                rule_name.clone()
            }
            ActionSpec::Firewall(FirewallAction::EnsureRule { rule }) => rule.name.clone(),
        };
        ResourceId::from_parts(kind, &body)
    }

    /// The state the resource must have after a successful `apply`.
    /// `None` means "the resource must not exist" (e.g. registry value delete).
    pub fn desired_state(&self) -> Option<ResourceState> {
        match self {
            ActionSpec::Registry(a) => {
                if a.delete {
                    None
                } else {
                    a.value
                        .clone()
                        .map(|value| ResourceState::RegistryValue { value })
                }
            }
            ActionSpec::Service(a) => Some(ResourceState::Service {
                start_type: a.start_type,
            }),
            ActionSpec::ScheduledTask(a) => {
                Some(ResourceState::ScheduledTask { enabled: a.enabled })
            }
            ActionSpec::PowerPlan(PowerPlanAction::ActivateScheme { scheme_guid }) => {
                Some(ResourceState::ActivePowerScheme {
                    scheme_guid: scheme_guid.to_lowercase(),
                })
            }
            ActionSpec::PowerPlan(PowerPlanAction::SetValue {
                ac_value, dc_value, ..
            }) => Some(ResourceState::PowerSetting {
                ac_value: *ac_value,
                dc_value: *dc_value,
            }),
            ActionSpec::Firewall(FirewallAction::SetRuleEnabled { enabled, .. }) => {
                Some(ResourceState::FirewallRuleEnabled { enabled: *enabled })
            }
            ActionSpec::Firewall(FirewallAction::EnsureRule { rule }) => {
                Some(ResourceState::FirewallRule { rule: rule.clone() })
            }
        }
    }
}

// ---------------------------------------------------------------- Registry --

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[non_exhaustive]
pub enum RegistryHive {
    #[serde(rename = "HKLM")]
    LocalMachine,
    #[serde(rename = "HKCU")]
    CurrentUser,
}

impl RegistryHive {
    pub const fn as_str(self) -> &'static str {
        match self {
            RegistryHive::LocalMachine => "HKLM",
            RegistryHive::CurrentUser => "HKCU",
        }
    }
}

/// Typed registry value. Serialized as `{ type = "dword", data = 0 }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
#[non_exhaustive]
pub enum RegistryValue {
    Dword(u32),
    Qword(u64),
    String(String),
    ExpandString(String),
    MultiString(Vec<String>),
    Binary(Vec<u8>),
}

/// Set (`value = ...`) or delete (`delete = true`) a single registry value.
/// Exactly one of the two must be given; enforced by manifest validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(deny_unknown_fields)]
pub struct RegistryAction {
    pub hive: RegistryHive,
    /// Key path below the hive, without leading/trailing backslash.
    pub key: String,
    /// Value name; empty string addresses the key's default value.
    pub value_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<RegistryValue>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub delete: bool,
}

// ----------------------------------------------------------------- Service --

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ServiceStartType {
    Automatic,
    AutomaticDelayed,
    Manual,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(deny_unknown_fields)]
pub struct ServiceAction {
    /// Service key name (not the display name), e.g. `DiagTrack`.
    pub name: String,
    pub start_type: ServiceStartType,
}

// ---------------------------------------------------------- Scheduled task --

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(deny_unknown_fields)]
pub struct ScheduledTaskAction {
    /// Full task path, e.g. `\Microsoft\Windows\Application Experience\ProgramDataUpdater`.
    pub path: String,
    pub enabled: bool,
}

// -------------------------------------------------------------- Power plan --

/// Power plan changes. GUIDs are lowercase `8-4-4-4-12` hex without braces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
#[non_exhaustive]
pub enum PowerPlanAction {
    ActivateScheme {
        scheme_guid: String,
    },
    SetValue {
        /// `None` targets the currently active scheme.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scheme_guid: Option<String>,
        subgroup_guid: String,
        setting_guid: String,
        ac_value: u32,
        dc_value: u32,
    },
}

// ---------------------------------------------------------------- Firewall --

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum FirewallDirection {
    Inbound,
    Outbound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum FirewallRuleAction {
    Allow,
    Block,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(deny_unknown_fields)]
pub struct FirewallRule {
    /// Unique rule name; resources owned by this app use the `WinOptSuite-` prefix.
    pub name: String,
    pub direction: FirewallDirection,
    pub action: FirewallRuleAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_addresses: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
#[non_exhaustive]
pub enum FirewallAction {
    SetRuleEnabled { rule_name: String, enabled: bool },
    EnsureRule { rule: FirewallRule },
}

// ---------------------------------------------------------- Resource state --

/// Observed (or desired) state of one resource. Absence is modelled as
/// `Option<ResourceState>::None` by the containing type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ResourceState {
    RegistryValue { value: RegistryValue },
    Service { start_type: ServiceStartType },
    ScheduledTask { enabled: bool },
    ActivePowerScheme { scheme_guid: String },
    PowerSetting { ac_value: u32, dc_value: u32 },
    FirewallRuleEnabled { enabled: bool },
    FirewallRule { rule: FirewallRule },
}

/// Returns `true` for a lowercase/uppercase `8-4-4-4-12` hex GUID without braces.
pub fn is_valid_guid(value: &str) -> bool {
    let groups: Vec<&str> = value.split('-').collect();
    let lens = [8usize, 4, 4, 4, 12];
    groups.len() == lens.len()
        && groups
            .iter()
            .zip(lens)
            .all(|(g, n)| g.len() == n && g.chars().all(|c| c.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reg(key: &str, name: &str) -> ActionSpec {
        ActionSpec::Registry(RegistryAction {
            hive: RegistryHive::LocalMachine,
            key: key.into(),
            value_name: name.into(),
            value: Some(RegistryValue::Dword(0)),
            delete: false,
        })
    }

    #[test]
    fn registry_resource_id_is_case_and_slash_insensitive() {
        let a = reg(r"SOFTWARE\Policies\X", "Val");
        let b = reg(r"\software\policies\x\", "VAL");
        assert_eq!(a.resource_id(), b.resource_id());
        assert_eq!(
            a.resource_id().as_str(),
            r"registry:hklm\software\policies\x\val"
        );
    }

    #[test]
    fn delete_has_no_desired_state() {
        let a = ActionSpec::Registry(RegistryAction {
            hive: RegistryHive::CurrentUser,
            key: "K".into(),
            value_name: "V".into(),
            value: None,
            delete: true,
        });
        assert_eq!(a.desired_state(), None);
    }

    #[test]
    fn provider_kind_roundtrip() {
        for k in ProviderKind::ALL {
            assert_eq!(ProviderKind::parse(k.as_str()), Some(k));
            let json = serde_json::to_string(&k).unwrap();
            assert_eq!(json, format!("\"{}\"", k.as_str()));
        }
    }

    #[test]
    fn guid_validation() {
        assert!(is_valid_guid("381b4222-f694-41f0-9685-ff5bb260df2e"));
        assert!(!is_valid_guid("{381b4222-f694-41f0-9685-ff5bb260df2e}"));
        assert!(!is_valid_guid("381b4222-f694-41f0-9685"));
    }
}
