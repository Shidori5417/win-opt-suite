//! # core-engine
//!
//! Platform-agnostic engine. Talks to the OS **only** through
//! `dyn shared_types::SystemProvider`, so it is fully testable on any OS with
//! in-memory providers. Implementation lands in Phase 1 (`claude-sonnet`).
#![forbid(unsafe_code)]

// Phase 0 placeholder: keeps the DAG edge declared until Phase 1 code uses it.
use shared_types as _;
