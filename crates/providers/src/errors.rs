//! Platform-independent error mapping shared by every provider.

use shared_types::{ActionSpec, AppError, ProviderKind, StateSnapshot};

pub const ERROR_FILE_NOT_FOUND: u32 = 2;
pub const ERROR_PATH_NOT_FOUND: u32 = 3;
pub const ERROR_ACCESS_DENIED: u32 = 5;
pub const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
pub const ERROR_MORE_DATA: u32 = 234;
pub const ERROR_SERVICE_DOES_NOT_EXIST: u32 = 1060;

/// `FACILITY_WIN32` HRESULT prefix (`HRESULT_FROM_WIN32`).
const HRESULT_WIN32_MASK: u32 = 0xFFFF_0000;
const HRESULT_WIN32_PREFIX: u32 = 0x8007_0000;

/// Maps a raw Win32 error code to a typed [`AppError`].
///
/// `ERROR_ACCESS_DENIED` becomes [`AppError::AccessDenied`];
/// `ERROR_FILE_NOT_FOUND`, `ERROR_PATH_NOT_FOUND` and
/// `ERROR_SERVICE_DOES_NOT_EXIST` become [`AppError::NotFound`]; everything
/// else is a raw [`AppError::Os`] carrying the system message.
pub fn from_win32(code: u32, resource: &str) -> AppError {
    let message = std::io::Error::from_raw_os_error(code as i32).to_string();
    classify(code, resource, message)
}

/// Same as [`from_win32`] for an `HRESULT`. `HRESULT_FROM_WIN32` values
/// (`0x8007xxxx`, e.g. `E_ACCESSDENIED`) are unwrapped to their Win32 code.
pub fn from_hresult(hr: i32, resource: &str, message: &str) -> AppError {
    let raw = hr as u32;
    if raw & HRESULT_WIN32_MASK == HRESULT_WIN32_PREFIX {
        return classify(raw & 0xFFFF, resource, message.to_owned());
    }
    AppError::Os {
        os_code: hr,
        message: message.to_owned(),
    }
}

fn classify(code: u32, resource: &str, message: String) -> AppError {
    match code {
        ERROR_ACCESS_DENIED => AppError::AccessDenied {
            resource: resource.to_owned(),
        },
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND | ERROR_SERVICE_DOES_NOT_EXIST => {
            AppError::NotFound {
                resource: resource.to_owned(),
            }
        }
        _ => AppError::Os {
            os_code: code as i32,
            message,
        },
    }
}

/// `true` when the error means "the addressed resource does not exist".
pub fn is_not_found(err: &AppError) -> bool {
    matches!(err, AppError::NotFound { .. })
}

/// Error for an action addressed to the wrong provider.
pub fn mismatch(expected: ProviderKind, action: &ActionSpec) -> AppError {
    AppError::ActionProviderMismatch {
        expected,
        found: action.provider_kind(),
    }
}

/// Generic internal error.
pub fn internal(message: impl Into<String>) -> AppError {
    AppError::Internal {
        message: message.into(),
    }
}

/// Validates that a snapshot belongs to `kind` and honours its invariant.
pub fn check_snapshot(kind: ProviderKind, snapshot: &StateSnapshot) -> Result<(), AppError> {
    if snapshot.provider != kind {
        return Err(AppError::ActionProviderMismatch {
            expected: kind,
            found: snapshot.provider,
        });
    }
    if !snapshot.is_consistent() {
        return Err(internal(format!(
            "inconsistent snapshot for {}",
            snapshot.resource
        )));
    }
    Ok(())
}

/// Outcome of `CoInitializeEx`, decided from its raw `HRESULT`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComInit {
    /// `S_OK` or `S_FALSE`: initialised (or ref-counted again); must call
    /// `CoUninitialize` exactly once.
    Owned,
    /// `RPC_E_CHANGED_MODE`: the thread already uses another apartment model;
    /// COM is usable but must not be uninitialised by us.
    Borrowed,
    /// Any other failure.
    Failed,
}

pub fn classify_com_init(hr: i32) -> ComInit {
    match hr as u32 {
        0 | 1 => ComInit::Owned,
        0x8001_0106 => ComInit::Borrowed,
        _ => ComInit::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn win32_mapping() {
        assert!(matches!(
            from_win32(5, "r"),
            AppError::AccessDenied { resource } if resource == "r"
        ));
        for code in [2, 3, 1060] {
            assert!(is_not_found(&from_win32(code, "r")), "code {code}");
        }
        match from_win32(87, "r") {
            AppError::Os { os_code, .. } => assert_eq!(os_code, 87),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn hresult_mapping() {
        assert!(matches!(
            from_hresult(0x8007_0005_u32 as i32, "r", "denied"),
            AppError::AccessDenied { .. }
        ));
        assert!(is_not_found(&from_hresult(0x8007_0002_u32 as i32, "r", "")));
        match from_hresult(0x8000_4005_u32 as i32, "r", "E_FAIL") {
            AppError::Os { os_code, message } => {
                assert_eq!(os_code, 0x8000_4005_u32 as i32);
                assert_eq!(message, "E_FAIL");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn com_init_classification() {
        assert_eq!(classify_com_init(0), ComInit::Owned);
        assert_eq!(classify_com_init(1), ComInit::Owned);
        assert_eq!(classify_com_init(0x8001_0106_u32 as i32), ComInit::Borrowed);
        assert_eq!(classify_com_init(0x8007_000E_u32 as i32), ComInit::Failed);
    }

    #[test]
    fn mismatch_reports_both_kinds() {
        let action = ActionSpec::Service(shared_types::ServiceAction {
            name: "x".into(),
            start_type: shared_types::ServiceStartType::Manual,
        });
        assert_eq!(
            mismatch(ProviderKind::Registry, &action),
            AppError::ActionProviderMismatch {
                expected: ProviderKind::Registry,
                found: ProviderKind::Service
            }
        );
    }
}
