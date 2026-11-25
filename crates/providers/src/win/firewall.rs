//! `FirewallProvider`: Windows Firewall rules through the `HNetCfg` COM API
//! (`INetFwPolicy2` -> `INetFwRules` -> `INetFwRule`).

use shared_types::{
    ActionSpec, AppError, AppResult, FirewallAction, FirewallRule, ProviderKind, ResourceState,
    StateSnapshot, SystemProvider, VerifyOutcome,
};
use windows::Win32::Foundation::{VARIANT_BOOL, VARIANT_FALSE, VARIANT_TRUE};
use windows::Win32::NetworkManagement::WindowsFirewall::{
    INetFwPolicy2, INetFwRule, INetFwRules, NET_FW_ACTION, NET_FW_RULE_DIRECTION, NetFwPolicy2,
    NetFwRule,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::core::BSTR;

use super::com::ComGuard;
use super::map_error;
use crate::contract::{ensure_kind, snapshot_of, verify_from_snapshot};
use crate::errors::{check_snapshot, internal, is_not_found, mismatch};
use crate::mapping::{
    NET_FW_PROFILE2_ALL, direction_from_raw, direction_to_raw, optional_text, rule_action_from_raw,
    rule_action_to_raw,
};

const KIND: ProviderKind = ProviderKind::Firewall;

/// Enables/disables existing rules and creates/updates/removes managed rules.
#[derive(Debug, Default, Clone, Copy)]
pub struct FirewallProvider;

impl FirewallProvider {
    pub fn new() -> Self {
        Self
    }
}

fn action_of(action: &ActionSpec) -> AppResult<&FirewallAction> {
    ensure_kind(KIND, action)?;
    match action {
        ActionSpec::Firewall(a) => Ok(a),
        other => Err(mismatch(KIND, other)),
    }
}

fn bool_to_variant(value: bool) -> VARIANT_BOOL {
    if value { VARIANT_TRUE } else { VARIANT_FALSE }
}

/// Runs `f` with the rule collection while COM is initialised.
fn with_rules<T>(resource: &str, f: impl FnOnce(&INetFwRules) -> AppResult<T>) -> AppResult<T> {
    // Declared first => dropped last: interfaces are released before
    // CoUninitialize.
    let _com = ComGuard::init()?;
    // SAFETY: COM is initialised on this thread (guard above); the CLSID and
    // the requested interface belong together.
    let created: windows::core::Result<INetFwPolicy2> =
        unsafe { CoCreateInstance(&NetFwPolicy2, None, CLSCTX_INPROC_SERVER) };
    let policy = created.map_err(|e| map_error(&e, resource))?;
    // SAFETY: `policy` is a live interface.
    let rules = unsafe { policy.Rules() };
    let rules = rules.map_err(|e| map_error(&e, resource))?;
    f(&rules)
}

fn find_rule(rules: &INetFwRules, name: &str, resource: &str) -> AppResult<Option<INetFwRule>> {
    // SAFETY: `rules` is a live interface; the BSTR outlives the call.
    let item = unsafe { rules.Item(&BSTR::from(name)) };
    match item {
        Ok(rule) => Ok(Some(rule)),
        Err(e) if is_not_found(&map_error(&e, resource)) => Ok(None),
        Err(e) => Err(map_error(&e, resource)),
    }
}

fn read_enabled(rule: &INetFwRule, resource: &str) -> AppResult<bool> {
    // SAFETY: `rule` is a live interface.
    let enabled = unsafe { rule.Enabled() };
    enabled
        .map(|v| v.0 != 0)
        .map_err(|e| map_error(&e, resource))
}

fn read_full_rule(rule: &INetFwRule, name: &str, resource: &str) -> AppResult<FirewallRule> {
    // SAFETY: `rule` is a live interface (each property getter below).
    let direction = unsafe { rule.Direction() }.map_err(|e| map_error(&e, resource))?;
    // SAFETY: as above.
    let action = unsafe { rule.Action() }.map_err(|e| map_error(&e, resource))?;
    // SAFETY: as above.
    let program = unsafe { rule.ApplicationName() }.map_err(|e| map_error(&e, resource))?;
    // SAFETY: as above.
    let remote = unsafe { rule.RemoteAddresses() }.map_err(|e| map_error(&e, resource))?;
    Ok(FirewallRule {
        name: name.to_owned(),
        direction: direction_from_raw(direction.0)?,
        action: rule_action_from_raw(action.0)?,
        program: optional_text(&program.to_string(), false),
        remote_addresses: optional_text(&remote.to_string(), true),
        enabled: read_enabled(rule, resource)?,
    })
}

fn set_enabled(rule: &INetFwRule, enabled: bool, resource: &str) -> AppResult<()> {
    // SAFETY: `rule` is a live interface.
    let res = unsafe { rule.SetEnabled(bool_to_variant(enabled)) };
    res.map_err(|e| map_error(&e, resource))
}

/// Creates the rule when `existing` is `None`, otherwise updates it in place.
fn write_rule(
    rules: &INetFwRules,
    existing: Option<INetFwRule>,
    desired: &FirewallRule,
    resource: &str,
) -> AppResult<()> {
    let is_new = existing.is_none();
    let rule = match existing {
        Some(rule) => rule,
        None => {
            // SAFETY: COM is initialised by the caller's guard; CLSID and
            // interface match.
            let created: windows::core::Result<INetFwRule> =
                unsafe { CoCreateInstance(&NetFwRule, None, CLSCTX_INPROC_SERVER) };
            let rule = created.map_err(|e| map_error(&e, resource))?;
            // SAFETY: `rule` is a live interface; the BSTR outlives the call.
            let res = unsafe { rule.SetName(&BSTR::from(desired.name.as_str())) };
            res.map_err(|e| map_error(&e, resource))?;
            // SAFETY: as above.
            let res = unsafe { rule.SetProfiles(NET_FW_PROFILE2_ALL) };
            res.map_err(|e| map_error(&e, resource))?;
            rule
        }
    };

    // SAFETY: `rule` is a live interface (every setter below).
    let res =
        unsafe { rule.SetDirection(NET_FW_RULE_DIRECTION(direction_to_raw(desired.direction))) };
    res.map_err(|e| map_error(&e, resource))?;
    // SAFETY: as above.
    let res = unsafe { rule.SetAction(NET_FW_ACTION(rule_action_to_raw(desired.action))) };
    res.map_err(|e| map_error(&e, resource))?;

    // `None` clears the program restriction (empty BSTR) so the rule matches
    // the desired state exactly when an existing rule is updated.
    let program = desired.program.as_deref().unwrap_or("");
    if !is_new || !program.is_empty() {
        // SAFETY: as above.
        let res = unsafe { rule.SetApplicationName(&BSTR::from(program)) };
        res.map_err(|e| map_error(&e, resource))?;
    }
    let remote = desired.remote_addresses.as_deref().unwrap_or("*");
    // SAFETY: as above.
    let res = unsafe { rule.SetRemoteAddresses(&BSTR::from(remote)) };
    res.map_err(|e| map_error(&e, resource))?;
    set_enabled(&rule, desired.enabled, resource)?;

    if is_new {
        // SAFETY: `rules` is a live interface; `rule` is fully configured.
        let res = unsafe { rules.Add(&rule) };
        res.map_err(|e| map_error(&e, resource))?;
    }
    Ok(())
}

fn remove_rule(rules: &INetFwRules, name: &str, resource: &str) -> AppResult<()> {
    // SAFETY: `rules` is a live interface; the BSTR outlives the call.
    let res = unsafe { rules.Remove(&BSTR::from(name)) };
    match res {
        Ok(()) => Ok(()),
        Err(e) if is_not_found(&map_error(&e, resource)) => Ok(()),
        Err(e) => Err(map_error(&e, resource)),
    }
}

fn not_found(resource: &str) -> AppError {
    AppError::NotFound {
        resource: resource.to_owned(),
    }
}

fn read_state(action: &FirewallAction, resource: &str) -> AppResult<Option<ResourceState>> {
    with_rules(resource, |rules| match action {
        FirewallAction::SetRuleEnabled { rule_name, .. } => {
            match find_rule(rules, rule_name, resource)? {
                None => Ok(None),
                Some(rule) => Ok(Some(ResourceState::FirewallRuleEnabled {
                    enabled: read_enabled(&rule, resource)?,
                })),
            }
        }
        FirewallAction::EnsureRule { rule } => match find_rule(rules, &rule.name, resource)? {
            None => Ok(None),
            Some(found) => Ok(Some(ResourceState::FirewallRule {
                rule: read_full_rule(&found, &rule.name, resource)?,
            })),
        },
        _ => Err(internal("unsupported firewall action")),
    })
}

impl SystemProvider for FirewallProvider {
    fn kind(&self) -> ProviderKind {
        KIND
    }

    fn read(&self, action: &ActionSpec) -> AppResult<StateSnapshot> {
        let a = action_of(action)?;
        let state = read_state(a, action.resource_id().as_str())?;
        Ok(snapshot_of(action, KIND, state))
    }

    fn apply(&self, action: &ActionSpec) -> AppResult<()> {
        let a = action_of(action)?;
        let resource = action.resource_id();
        let resource = resource.as_str();
        with_rules(resource, |rules| match a {
            FirewallAction::SetRuleEnabled { rule_name, enabled } => {
                let rule =
                    find_rule(rules, rule_name, resource)?.ok_or_else(|| not_found(resource))?;
                set_enabled(&rule, *enabled, resource)
            }
            FirewallAction::EnsureRule { rule } => {
                let existing = find_rule(rules, &rule.name, resource)?;
                write_rule(rules, existing, rule, resource)
            }
            _ => Err(internal("unsupported firewall action")),
        })
    }

    fn revert(&self, action: &ActionSpec, snapshot: &StateSnapshot) -> AppResult<()> {
        let a = action_of(action)?;
        check_snapshot(KIND, snapshot)?;
        let resource = action.resource_id();
        let resource = resource.as_str();
        with_rules(resource, |rules| match (a, &snapshot.state) {
            // The rule did not exist before: delete it (EnsureRule), or there
            // is nothing to undo (SetRuleEnabled never creates rules).
            (FirewallAction::EnsureRule { rule }, None) => remove_rule(rules, &rule.name, resource),
            (FirewallAction::SetRuleEnabled { .. }, None) => Ok(()),
            (
                FirewallAction::SetRuleEnabled { rule_name, .. },
                Some(ResourceState::FirewallRuleEnabled { enabled }),
            ) => {
                match find_rule(rules, rule_name, resource)? {
                    Some(rule) => set_enabled(&rule, *enabled, resource),
                    // Removed meanwhile: the previous state cannot be restored.
                    None => Err(not_found(resource)),
                }
            }
            (
                FirewallAction::EnsureRule { rule },
                Some(ResourceState::FirewallRule { rule: previous }),
            ) => {
                let existing = find_rule(rules, &rule.name, resource)?;
                write_rule(rules, existing, previous, resource)
            }
            _ => Err(internal(
                "snapshot state does not match the firewall action",
            )),
        })
    }

    fn verify(&self, action: &ActionSpec) -> AppResult<VerifyOutcome> {
        let live = self.read(action)?;
        Ok(verify_from_snapshot(action, &live))
    }
}
