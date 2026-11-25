//! WinOptSuite desktop entry point.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![forbid(unsafe_code)]

use std::process::ExitCode;

fn main() -> ExitCode {
    match desktop::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("WinOptSuite failed to start: {err}");
            eprintln!(
                "If WebView2 is unavailable on this system, use `opti-cli` as the headless fallback."
            );
            ExitCode::FAILURE
        }
    }
}
