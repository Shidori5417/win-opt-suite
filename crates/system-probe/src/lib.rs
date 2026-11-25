//! # system-probe
//!
//! Read-only collection of `shared_types::SystemFacts`, degradation warnings,
//! and before/after [`shared_types::MetricSample`] snapshots via an injected
//! [`shared_types::ProbeSource`]. Must never modify the system.
#![forbid(unsafe_code)]

pub mod detect;
mod probe;

pub use detect::{
    PORTABLE_CHASSIS_CODES, build_metrics, detect_anti_cheat, detect_laptop, detect_on_battery,
    idle_memory_used_bytes, is_portable_chassis,
};
pub use probe::{ProbeQuery, ProbeWarning, SystemProbe};
