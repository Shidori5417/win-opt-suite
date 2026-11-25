//! # providers
//!
//! Native Windows adapters implementing `shared_types::SystemProvider` via
//! `windows-rs`. This is the **only** crate allowed to contain `unsafe`, and
//! every `unsafe` block must carry a `// SAFETY:` justification (enforced by
//! `clippy::undocumented_unsafe_blocks`). Implementation lands in Phase 1
//! (`claude-sonnet`).
#![allow(unsafe_code)]

// Phase 0 placeholder: keeps the DAG edge declared until Phase 1 code uses it.
use shared_types as _;
