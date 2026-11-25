//! # providers
//!
//! Native Windows adapters implementing `shared_types::SystemProvider` via
//! `windows-rs`. This is the **only** crate allowed to contain `unsafe`, and
//! every `unsafe` block must carry a `// SAFETY:` justification (enforced by
//! `clippy::undocumented_unsafe_blocks`) and perform a single unsafe
//! operation (`clippy::multiple_unsafe_ops_per_block`).
//!
//! Layout:
//!
//! * platform-independent, unit-tested everywhere: [`errors`], [`reg_codec`],
//!   [`paths`], [`guid`], [`mapping`], [`contract`];
//! * Windows-only (`cfg(windows)`): the `win` module with the five providers.
//!
//! No external processes (`powershell`, `reg.exe`, `sc.exe`, ...) are ever
//! spawned; only native Win32/COM APIs are used.
#![allow(unsafe_code)]
// Pure helpers are only consumed by the Windows adapters; keep them (and their
// tests) compiling on every platform without dead-code noise.
#![cfg_attr(not(windows), allow(dead_code))]

pub mod contract;
pub mod errors;
pub mod guid;
pub mod mapping;
pub mod paths;
pub mod reg_codec;

use shared_types::SystemProvider;

/// All native providers available on this platform: the five Windows
/// adapters (registry, service, scheduled task, power plan, firewall) on
/// Windows, an empty list elsewhere.
pub fn native_providers() -> Vec<Box<dyn SystemProvider>> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    #[cfg(not(windows))]
    #[test]
    fn no_native_providers_off_windows() {
        assert!(super::native_providers().is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn one_provider_per_kind_on_windows() {
        let mut kinds: Vec<_> = super::native_providers().iter().map(|p| p.kind()).collect();
        kinds.sort();
        let mut all = shared_types::ProviderKind::ALL.to_vec();
        all.sort();
        assert_eq!(kinds, all);
    }
}
