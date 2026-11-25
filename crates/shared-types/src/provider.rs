//! The port every system adapter implements (hexagonal architecture).

use crate::action::{ActionSpec, ProviderKind};
use crate::error::AppResult;
use crate::snapshot::{StateSnapshot, VerifyOutcome};

/// Adapter between the platform-agnostic engine and one OS subsystem.
///
/// The `TransactionManager` only ever talks to `dyn SystemProvider` looked up
/// from the `ProviderRegistry` by [`ProviderKind`]; it never knows concrete
/// providers.
///
/// # Contract (all implementations, enforced by the shared test-suite)
///
/// * `read` is side-effect free and returns the *current* state of the
///   resource addressed by `action`; for a missing resource it returns
///   [`StateSnapshot::absent`] (not an error).
/// * `apply` brings the resource to `action.desired_state()`. It must be
///   idempotent. It must fail with a typed [`crate::AppError`] — never panic —
///   and must not leave the resource half-written when it can avoid it.
/// * `revert` restores exactly the snapshot taken by `read` before `apply`:
///   if `snapshot.existed == false` the resource is **deleted**, not reset;
///   otherwise its state (and SDDL, if captured) is restored. Reverting a
///   resource that is already in the snapshot state succeeds (idempotent).
/// * `verify` compares the live state with `action.desired_state()`.
/// * Every method returns [`crate::AppError::ActionProviderMismatch`] when
///   given an action of a different [`ProviderKind`].
pub trait SystemProvider: Send + Sync {
    /// The single provider kind this adapter handles.
    fn kind(&self) -> ProviderKind;

    fn read(&self, action: &ActionSpec) -> AppResult<StateSnapshot>;

    fn apply(&self, action: &ActionSpec) -> AppResult<()>;

    fn revert(&self, action: &ActionSpec, snapshot: &StateSnapshot) -> AppResult<()>;

    fn verify(&self, action: &ActionSpec) -> AppResult<VerifyOutcome>;
}
