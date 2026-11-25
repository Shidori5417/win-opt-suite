//! # system-probe
//!
//! Read-only collection of `shared_types::SystemFacts` and before/after
//! metrics. Must never modify the system. Implementation lands in Phase 1
//! (`hydrogen`).
#![forbid(unsafe_code)]

// Phase 0 placeholder: keeps the DAG edge declared until Phase 1 code uses it.
use shared_types as _;
