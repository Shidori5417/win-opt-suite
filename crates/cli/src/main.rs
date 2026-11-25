//! `opti-cli` — thin headless shell over `core-engine`, `providers`, and `system-probe`.
#![forbid(unsafe_code)]

mod cli;
mod commands;
mod output;

use std::process::ExitCode;

use clap::Parser;
use shared_types::AppError;

use crate::cli::Cli;
use crate::commands::{execute, resolve_db_path, resolve_manifests_dir, warn_if_incomplete};
use crate::output::print_error;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let json = cli.json;
    let db_path = resolve_db_path(cli.db);
    let manifests_dir = resolve_manifests_dir(cli.manifests);

    warn_if_incomplete(&db_path, &cli.command);

    match execute(cli.command, &db_path, &manifests_dir, json) {
        Ok(code) => code,
        Err(err) => {
            print_error(&err, json);
            exit_code_for_error(&err)
        }
    }
}

fn exit_code_for_error(err: &AppError) -> ExitCode {
    match err {
        AppError::ConflictBlocked { .. } => ExitCode::from(2),
        _ => ExitCode::from(1),
    }
}
