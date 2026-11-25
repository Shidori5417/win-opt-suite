//! Pure value mappings between `shared-types` enums and Win32 constants.

use shared_types::{AppResult, FirewallDirection, FirewallRuleAction, ServiceStartType};

use crate::errors::internal;

// ------------------------------------------------------------------ Services

pub const SERVICE_BOOT_START: u32 = 0;
pub const SERVICE_SYSTEM_START: u32 = 1;
pub const SERVICE_AUTO_START: u32 = 2;
pub const SERVICE_DEMAND_START: u32 = 3;
pub const SERVICE_DISABLED: u32 = 4;

/// Raw `dwStartType` plus the delayed-auto-start flag for a start type.
pub fn start_type_to_raw(start: ServiceStartType) -> AppResult<(u32, bool)> {
    match start {
        ServiceStartType::Automatic => Ok((SERVICE_AUTO_START, false)),
        ServiceStartType::AutomaticDelayed => Ok((SERVICE_AUTO_START, true)),
        ServiceStartType::Manual => Ok((SERVICE_DEMAND_START, false)),
        ServiceStartType::Disabled => Ok((SERVICE_DISABLED, false)),
        _ => Err(internal("unsupported service start type")),
    }
}

/// Inverse of [`start_type_to_raw`]. The delayed flag only matters for
/// `SERVICE_AUTO_START`. Boot/system start (drivers) are not modelled.
pub fn start_type_from_raw(raw: u32, delayed: bool) -> AppResult<ServiceStartType> {
    match raw {
        SERVICE_AUTO_START if delayed => Ok(ServiceStartType::AutomaticDelayed),
        SERVICE_AUTO_START => Ok(ServiceStartType::Automatic),
        SERVICE_DEMAND_START => Ok(ServiceStartType::Manual),
        SERVICE_DISABLED => Ok(ServiceStartType::Disabled),
        SERVICE_BOOT_START | SERVICE_SYSTEM_START => Err(internal(format!(
            "driver start type {raw} is not supported"
        ))),
        other => Err(internal(format!("unknown service start type {other}"))),
    }
}

/// Whether the delayed-auto-start flag has to be queried/written.
pub fn is_auto_start(raw: u32) -> bool {
    raw == SERVICE_AUTO_START
}

// ------------------------------------------------------------------ Firewall

pub const NET_FW_RULE_DIR_IN: i32 = 1;
pub const NET_FW_RULE_DIR_OUT: i32 = 2;
pub const NET_FW_ACTION_BLOCK: i32 = 0;
pub const NET_FW_ACTION_ALLOW: i32 = 1;
/// `NET_FW_PROFILE2_ALL`.
pub const NET_FW_PROFILE2_ALL: i32 = 0x7FFF_FFFF;

pub fn direction_to_raw(d: FirewallDirection) -> i32 {
    match d {
        FirewallDirection::Inbound => NET_FW_RULE_DIR_IN,
        FirewallDirection::Outbound => NET_FW_RULE_DIR_OUT,
    }
}

pub fn direction_from_raw(raw: i32) -> AppResult<FirewallDirection> {
    match raw {
        NET_FW_RULE_DIR_IN => Ok(FirewallDirection::Inbound),
        NET_FW_RULE_DIR_OUT => Ok(FirewallDirection::Outbound),
        other => Err(internal(format!("unknown firewall direction {other}"))),
    }
}

pub fn rule_action_to_raw(a: FirewallRuleAction) -> i32 {
    match a {
        FirewallRuleAction::Allow => NET_FW_ACTION_ALLOW,
        FirewallRuleAction::Block => NET_FW_ACTION_BLOCK,
    }
}

pub fn rule_action_from_raw(raw: i32) -> AppResult<FirewallRuleAction> {
    match raw {
        NET_FW_ACTION_ALLOW => Ok(FirewallRuleAction::Allow),
        NET_FW_ACTION_BLOCK => Ok(FirewallRuleAction::Block),
        other => Err(internal(format!("unknown firewall action {other}"))),
    }
}

/// Maps a BSTR property of a firewall rule to the optional field of
/// `FirewallRule`: empty (and, for addresses, the wildcard `*`) means `None`.
pub fn optional_text(value: &str, wildcard_is_none: bool) -> Option<String> {
    let v = value.trim();
    if v.is_empty() || (wildcard_is_none && v == "*") {
        None
    } else {
        Some(v.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_type_roundtrip() {
        for s in [
            ServiceStartType::Automatic,
            ServiceStartType::AutomaticDelayed,
            ServiceStartType::Manual,
            ServiceStartType::Disabled,
        ] {
            let (raw, delayed) = start_type_to_raw(s).unwrap();
            assert_eq!(start_type_from_raw(raw, delayed).unwrap(), s);
        }
    }

    #[test]
    fn delayed_flag_is_ignored_for_non_auto() {
        assert_eq!(
            start_type_from_raw(SERVICE_DEMAND_START, true).unwrap(),
            ServiceStartType::Manual
        );
        assert!(is_auto_start(SERVICE_AUTO_START));
        assert!(!is_auto_start(SERVICE_DISABLED));
    }

    #[test]
    fn driver_and_unknown_start_types_error() {
        assert!(start_type_from_raw(SERVICE_BOOT_START, false).is_err());
        assert!(start_type_from_raw(SERVICE_SYSTEM_START, false).is_err());
        assert!(start_type_from_raw(99, false).is_err());
    }

    #[test]
    fn firewall_enums_roundtrip() {
        for d in [FirewallDirection::Inbound, FirewallDirection::Outbound] {
            assert_eq!(direction_from_raw(direction_to_raw(d)).unwrap(), d);
        }
        for a in [FirewallRuleAction::Allow, FirewallRuleAction::Block] {
            assert_eq!(rule_action_from_raw(rule_action_to_raw(a)).unwrap(), a);
        }
        assert!(direction_from_raw(7).is_err());
        assert!(rule_action_from_raw(7).is_err());
    }

    #[test]
    fn optional_text_rules() {
        assert_eq!(optional_text("", false), None);
        assert_eq!(optional_text("  ", true), None);
        assert_eq!(optional_text("*", true), None);
        assert_eq!(optional_text("*", false), Some("*".into()));
        assert_eq!(optional_text(r"C:\a.exe", false), Some(r"C:\a.exe".into()));
    }
}
