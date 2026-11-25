//! # shared-types
//!
//! Frozen, platform-independent contracts shared by every crate in the
//! workspace (see `docs/adr/0001-hexagonal-rollback-architecture.md`).
//!
//! **Rules (AGENTS.md §2):** this crate depends on no internal crate and on no
//! platform API. Changing a public type here is a contract change and requires
//! approval from the chief architect (`opus-5.5`); breaking data-model changes
//! require a new ADR.
#![forbid(unsafe_code)]

pub mod action;
pub mod conflict;
pub mod diff;
pub mod drift;
pub mod error;
pub mod ids;
pub mod manifest;
pub mod policy;
pub mod probe;
pub mod provider;
pub mod snapshot;
pub mod system;
pub mod transaction;

pub use action::{
    ActionSpec, FirewallAction, FirewallDirection, FirewallRule, FirewallRuleAction,
    PowerPlanAction, ProviderKind, RegistryAction, RegistryHive, RegistryValue, ResourceState,
    ScheduledTaskAction, ServiceAction, ServiceStartType,
};
pub use conflict::{ConflictKind, ConflictWarning, Severity};
pub use diff::{ChangeKind, DiffEntry, DryRunDiff};
pub use drift::{DriftCheckError, DriftEntry, DriftReport};
pub use error::{AppError, AppResult};
pub use ids::{ResourceId, TransactionId, TweakId};
pub use manifest::{
    CURRENT_MANIFEST_SCHEMA_VERSION, EnvironmentTag, ManifestIssue, Profile, RiskLevel,
    TweakCategory, TweakManifest, WINDOWS_11_MIN_BUILD,
};
pub use probe::{MemoryStatus, OsVersionInfo, PowerStatus, ProbeSource};
pub use provider::SystemProvider;
pub use snapshot::{StateSnapshot, VerifyOutcome};
pub use system::{
    AntiCheatDriver, CpuInfo, GpuInfo, MetricKind, MetricPhase, MetricSample, SystemFacts,
};
pub use transaction::{StepStatus, TransactionStatus, TransactionSummary};
