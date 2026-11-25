//! Tauri v2 build script.
//!
//! The Windows application manifest (administrator elevation for HKLM,
//! service, task, power and firewall writes + Common Controls v6) is embedded
//! with the MSVC linker into EVERY target, not via tauri-build's resource.
//! tauri-build only embeds it into the binary, so the unit-test executable
//! would lack the Common Controls v6 dependency and fail to start with
//! `STATUS_ENTRYPOINT_NOT_FOUND` (TaskDialogIndirect). This is the approach
//! documented by Tauri for crates with tests.

use std::path::Path;

const WINDOWS_APP_MANIFEST: &str = "windows-app-manifest.xml";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let windows = tauri_build::WindowsAttributes::new_without_app_manifest();
    let attributes = tauri_build::Attributes::new().windows_attributes(windows);
    tauri_build::try_build(attributes)?;

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_os == "windows" && target_env == "msvc" {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join(WINDOWS_APP_MANIFEST);
        println!("cargo:rerun-if-changed={}", manifest.display());
        // The linker would otherwise add its own asInvoker UAC fragment,
        // which conflicts with our requestedExecutionLevel.
        println!("cargo:rustc-link-arg=/MANIFESTUAC:NO");
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
        // Turn linker warnings (e.g. duplicate manifests) into errors.
        println!("cargo:rustc-link-arg=/WX");
    }
    Ok(())
}
