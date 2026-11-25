//! Application state and composition root helpers mirroring `opti-cli`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use core_engine::{FileLock, ProviderRegistry, Store, TransactionManager};
use shared_types::{AppError, AppResult};

/// Shared desktop shell state managed by Tauri.
///
/// The `TransactionManager` is guarded by a `Mutex` inside an `Arc` so IPC
/// commands can offload blocking SQLite / Win32 provider calls onto
/// `tauri::async_runtime::spawn_blocking` without blocking the webview thread.
#[derive(Debug)]
pub struct AppState {
    manager: Arc<Mutex<TransactionManager>>,
    manifests_dir: PathBuf,
}

impl AppState {
    /// Opens the persistent state store, registers native platform providers,
    /// and prepares the shared state for Tauri IPC commands.
    pub fn open(db_path: &Path, manifests_dir: PathBuf) -> AppResult<Self> {
        let manager = open_manager(db_path)?;
        Ok(Self::from_parts(manager, manifests_dir))
    }

    /// Wraps an already-constructed `TransactionManager` (primarily for tests).
    pub fn from_parts(manager: TransactionManager, manifests_dir: PathBuf) -> Self {
        Self {
            manager: Arc::new(Mutex::new(manager)),
            manifests_dir,
        }
    }

    /// Clones the shared handle to the transaction manager for background tasks.
    pub fn manager(&self) -> Arc<Mutex<TransactionManager>> {
        Arc::clone(&self.manager)
    }

    /// Directory containing declarative `*.toml` tweak manifests.
    pub fn manifests_dir(&self) -> PathBuf {
        self.manifests_dir.clone()
    }
}

/// Acquires the transaction manager mutex, mapping poison errors to `AppError`.
pub fn lock_manager(
    manager: &Mutex<TransactionManager>,
) -> AppResult<MutexGuard<'_, TransactionManager>> {
    manager.lock().map_err(|_| AppError::Internal {
        message: "transaction manager lock poisoned".to_owned(),
    })
}

/// Extracts an optional `--db <path>` or `--db=<path>` flag from CLI args.
pub fn parse_db_arg<I>(args: I) -> Option<PathBuf>
where
    I: IntoIterator<Item = OsString>,
{
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        if arg == "--db" {
            return iter.next().map(PathBuf::from);
        }
        if let Some(s) = arg.to_str()
            && let Some(rest) = s.strip_prefix("--db=")
            && !rest.is_empty()
        {
            return Some(PathBuf::from(rest));
        }
    }
    None
}

/// Resolves the SQLite state database path:
/// 1. Explicit `--db` argument.
/// 2. `%ProgramData%\WinOptSuite\state.db` on Windows.
/// 3. Tauri app local data directory (`state.db`), if provided.
/// 4. Local `state.db` fallback.
pub fn resolve_db_path(explicit: Option<PathBuf>, app_data_dir: Option<PathBuf>) -> PathBuf {
    if let Some(path) = explicit {
        return path;
    }
    if let Some(program_data) = std::env::var_os("ProgramData")
        && !program_data.is_empty()
    {
        return PathBuf::from(program_data)
            .join("WinOptSuite")
            .join("state.db");
    }
    if let Some(data_dir) = app_data_dir {
        return data_dir.join("state.db");
    }
    PathBuf::from("state.db")
}

/// Resolves the tweak manifests directory:
/// 1. Bundled Tauri resource directory (`<resource_dir>/manifests/tweaks`).
/// 2. Adjacent to the current executable (`<exe_dir>/manifests/tweaks`).
/// 3. Development workspace fallback (`<CARGO_MANIFEST_DIR>/../../../manifests/tweaks`).
/// 4. Current working directory (`manifests/tweaks`).
pub fn resolve_manifests_dir(resource_dir: Option<PathBuf>) -> PathBuf {
    if let Some(res_dir) = resource_dir {
        let bundled = res_dir.join("manifests").join("tweaks");
        if bundled.is_dir() {
            return bundled;
        }
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        let beside_exe = parent.join("manifests").join("tweaks");
        if beside_exe.is_dir() {
            return beside_exe;
        }
    }
    let dev_fallback = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("manifests")
        .join("tweaks");
    if dev_fallback.is_dir() {
        return dev_fallback;
    }
    PathBuf::from("manifests").join("tweaks")
}

fn open_store(db_path: &Path) -> AppResult<Store> {
    if let Some(parent) = db_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    Store::open(db_path)
}

fn lock_path_for_db(db_path: &Path) -> PathBuf {
    let mut os = db_path.as_os_str().to_os_string();
    os.push(".lock");
    PathBuf::from(os)
}

fn build_registry() -> AppResult<ProviderRegistry> {
    let mut registry = ProviderRegistry::new();
    for provider in providers::native_providers() {
        registry.register(provider)?;
    }
    Ok(registry)
}

fn open_manager(db_path: &Path) -> AppResult<TransactionManager> {
    let store = open_store(db_path)?;
    let registry = build_registry()?;
    let lock = Box::new(FileLock::new(lock_path_for_db(db_path)));
    Ok(TransactionManager::new(store, registry, lock))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_db_arg_forms() {
        let args = vec![
            OsString::from("winoptsuite"),
            OsString::from("--db"),
            OsString::from("/tmp/custom.db"),
        ];
        assert_eq!(parse_db_arg(args), Some(PathBuf::from("/tmp/custom.db")));

        let eq_args = vec![
            OsString::from("winoptsuite"),
            OsString::from("--db=/tmp/eq.db"),
        ];
        assert_eq!(parse_db_arg(eq_args), Some(PathBuf::from("/tmp/eq.db")));

        let none_args = vec![OsString::from("winoptsuite")];
        assert_eq!(parse_db_arg(none_args), None);
    }

    #[test]
    fn resolves_dev_manifests_dir() {
        let dir = resolve_manifests_dir(None);
        assert!(dir.is_dir(), "expected manifests dir at {}", dir.display());
    }
}
