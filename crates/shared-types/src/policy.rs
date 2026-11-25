//! Project-wide safety policy encoded as data (AGENTS.md §3, "Yeni Tweak
//! Eklerken"): no placebo tweaks, never weaken Microsoft Defender or core
//! security services. Enforced at manifest validation time.

use crate::action::ActionSpec;

/// Registry key fragments (lowercase) that must never be written.
const FORBIDDEN_REGISTRY_KEY_FRAGMENTS: &[(&str, &str)] = &[
    ("\\windows defender", "weakens Microsoft Defender"),
    ("\\microsoft antimalware", "weakens Microsoft Defender"),
    (
        "\\windows defender security center",
        "weakens Windows Security",
    ),
    (
        "\\currentcontrolset\\control\\deviceguard",
        "alters VBS/HVCI security",
    ),
    ("\\currentcontrolset\\control\\lsa", "alters LSA protection"),
];

/// Registry value names (lowercase) that are known placebo tweaks.
const PLACEBO_VALUE_NAMES: &[&str] = &["networkthrottlingindex", "systemresponsiveness"];

/// Services (lowercase key names) that must never be reconfigured.
const PROTECTED_SERVICES: &[&str] = &[
    "windefend",
    "wdnissvc",
    "wdnisdrv",
    "wdfilter",
    "wdboot",
    "sense",
    "securityhealthservice",
    "wscsvc",
    "mpssvc",
    "bfe",
    "eventlog",
    "trustedinstaller",
];

/// Returns why an action is forbidden, or `None` if it is allowed.
pub fn forbidden_reason(action: &ActionSpec) -> Option<&'static str> {
    match action {
        ActionSpec::Registry(a) => {
            let key = format!("\\{}", a.key.trim_matches('\\').to_lowercase());
            if let Some((_, reason)) = FORBIDDEN_REGISTRY_KEY_FRAGMENTS
                .iter()
                .find(|(frag, _)| key.contains(frag))
            {
                return Some(reason);
            }
            let name = a.value_name.to_lowercase();
            if PLACEBO_VALUE_NAMES.contains(&name.as_str()) {
                return Some("known placebo tweak");
            }
            None
        }
        ActionSpec::Service(a) => PROTECTED_SERVICES
            .contains(&a.name.to_lowercase().as_str())
            .then_some("protected security/core service"),
        ActionSpec::ScheduledTask(a) => a
            .path
            .to_lowercase()
            .contains("\\windows defender\\")
            .then_some("weakens Microsoft Defender"),
        ActionSpec::PowerPlan(_) | ActionSpec::Firewall(_) => None,
    }
}
