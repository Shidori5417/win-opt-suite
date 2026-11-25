# WinOptSuite Desktop (`apps/desktop`)

Thin Tauri v2 + React 19 desktop shell for WinOptSuite (see [ADR-0003](../../docs/adr/0003-desktop-shell-and-ipc-typegen.md)).

## Architecture

- **Rust shell (`src-tauri`, crate `desktop`)**: Composition root mirroring `opti-cli` (`%ProgramData%\WinOptSuite\state.db` by default, overrideable with `--db <path>`). Bundled `manifests/tweaks/*.toml` are loaded from Tauri's resource directory (falling back to `../../../manifests/tweaks` in development). All engine work runs inside `tauri::async_runtime::spawn_blocking`.
- **IPC Type Generation**: `src/bindings.ts` is generated automatically by `tauri-specta` via `cargo test -p desktop export_bindings`. Never edit `src/bindings.ts` manually; CI verifies freshness with `git diff --exit-code apps/desktop/src/bindings.ts`.
- **Frontend (`src/`)**: React 19, strict TypeScript (`strict`, `noUncheckedIndexedAccess`), Tailwind CSS v4, lightweight typed EN/TR i18n, and Vitest + Testing Library smoke tests.

## Windows Elevation & WebView2 Fallback

- **Administrator privileges**: `src-tauri/build.rs` embeds a Windows application manifest with `requestedExecutionLevel level="requireAdministrator"` because HKLM registry, service, scheduled task, power plan, and firewall providers require elevation.
- **WebView2 runtime**: `tauri.conf.json` configures `bundle.windows.webviewInstallMode = { type = "downloadBootstrapper" }` so the installer downloads and installs WebView2 when missing.
- **Headless fallback**: On environments where WebView2 cannot be installed (e.g. stripped Server/Enterprise images or offline recovery), use **`opti-cli`** (`crates/cli`) as the full-featured headless CLI fallback — it shares the exact same SQLite state database (`%ProgramData%\WinOptSuite\state.db`) and rollback engine.

## Commands

```bash
# Type-check, build (pure Vite — works without Tauri runtime), and run unit tests
pnpm --dir apps/desktop typecheck
pnpm --dir apps/desktop build
pnpm --dir apps/desktop test

# Regenerate src/bindings.ts and run Rust tests
cargo test -p desktop
```
