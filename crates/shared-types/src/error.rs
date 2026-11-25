//! Typed application errors. Serializable so they cross the Tauri IPC
//! boundary unchanged; the UI localises them by `code`.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::action::ProviderKind;
use crate::conflict::ConflictWarning;
use crate::manifest::ManifestIssue;
use crate::transaction::TransactionStatus;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "code", rename_all = "snake_case")]
#[non_exhaustive]
pub enum AppError {
    #[error("access denied: {resource}")]
    AccessDenied { resource: String },

    #[error("resource not found: {resource}")]
    NotFound { resource: String },

    #[error("unsupported platform: {reason}")]
    UnsupportedPlatform { reason: String },

    #[error("no provider registered for `{provider}`")]
    ProviderNotRegistered { provider: ProviderKind },

    #[error("provider `{expected}` received an action for `{found}`")]
    ActionProviderMismatch {
        expected: ProviderKind,
        found: ProviderKind,
    },

    #[error("invalid manifest `{id}`: {} issue(s)", issues.len())]
    InvalidManifest {
        id: String,
        issues: Vec<ManifestIssue>,
    },

    #[error("apply blocked by {} conflict(s)", warnings.len())]
    ConflictBlocked { warnings: Vec<ConflictWarning> },

    #[error("another optimisation transaction is already running")]
    LockUnavailable,

    #[error("incomplete transaction `{transaction_id}` must be recovered first")]
    RecoveryRequired { transaction_id: String },

    #[error("provider `{provider}` is already registered")]
    DuplicateProvider { provider: ProviderKind },

    #[error("transaction `{transaction_id}` is {status}, which does not allow this operation")]
    InvalidTransactionState {
        transaction_id: String,
        status: TransactionStatus,
    },

    /// Rolling back would overwrite changes made by a newer committed
    /// transaction to the same resources; roll back the newer one first.
    #[error(
        "transaction `{transaction_id}` is superseded by `{newer_transaction_id}` on {resources:?}"
    )]
    RollbackSuperseded {
        transaction_id: String,
        newer_transaction_id: String,
        resources: Vec<String>,
    },

    #[error("verification failed for {resource}")]
    VerificationFailed { resource: String },

    #[error("rollback of `{transaction_id}` incomplete; failed resources: {failed_resources:?}")]
    RollbackIncomplete {
        transaction_id: String,
        failed_resources: Vec<String>,
    },

    #[error("storage error: {message}")]
    Storage { message: String },

    #[error("migration {version} failed: {message}")]
    Migration { version: u32, message: String },

    #[error("serialization error: {message}")]
    Serialization { message: String },

    #[error("I/O error: {message}")]
    Io { message: String },

    /// Raw OS failure (Win32 error / HRESULT) not mapped to a specific variant.
    #[error("OS error {os_code:#010x}: {message}")]
    Os { os_code: i32, message: String },

    #[error("internal error: {message}")]
    Internal { message: String },
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        AppError::Io {
            message: err.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_with_code_tag() {
        let e = AppError::AccessDenied {
            resource: "registry:hklm\\x".into(),
        };
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(json["code"], "access_denied");
        let back: AppError = serde_json::from_value(json).unwrap();
        assert_eq!(back, e);
    }
}
