//! `opti-cli` — thin headless shell over `core-engine`. Commands (`scan`,
//! `dry-run`, `apply`, `rollback`, `drift`, `doctor`) land in Phase 2
//! (`hydrogen`).
#![forbid(unsafe_code)]

use shared_types as _;

fn main() -> std::process::ExitCode {
    eprintln!(
        "opti-cli {}: not yet implemented (Phase 2)",
        env!("CARGO_PKG_VERSION")
    );
    std::process::ExitCode::from(2)
}
