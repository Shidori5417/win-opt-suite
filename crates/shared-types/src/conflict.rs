//! Conflict warnings produced by the `ConflictGuard`.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::ids::{ResourceId, TweakId};
use crate::manifest::EnvironmentTag;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    /// Apply is refused until the user deselects the offending tweak(s).
    Blocking,
}

/// Structured conflict reason. The UI localises it by `kind`; `Display` is an
/// English fallback for logs and the CLI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ConflictKind {
    /// Both tweaks are selected but one declares `conflicts_with` the other.
    DeclaredConflict { tweak: TweakId, other: TweakId },
    /// Several selected tweaks want different states for the same resource.
    ResourceContention {
        resource: ResourceId,
        tweaks: Vec<TweakId>,
    },
    /// The tweak is excluded on a detected environment (laptop, anti-cheat...).
    ExcludedEnvironment { tweak: TweakId, tag: EnvironmentTag },
    /// The running Windows build is outside the tweak's supported range.
    UnsupportedBuild {
        tweak: TweakId,
        current_build: u32,
        min_build: u32,
        max_build: Option<u32>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ConflictWarning {
    pub severity: Severity,
    pub kind: ConflictKind,
}

impl fmt::Display for ConflictWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ConflictKind::DeclaredConflict { tweak, other } => {
                write!(f, "{tweak} conflicts with {other}")
            }
            ConflictKind::ResourceContention { resource, tweaks } => {
                let names: Vec<&str> = tweaks.iter().map(TweakId::as_str).collect();
                write!(f, "{resource} is targeted by {}", names.join(", "))
            }
            ConflictKind::ExcludedEnvironment { tweak, tag } => {
                write!(f, "{tweak} is excluded on {tag:?}")
            }
            ConflictKind::UnsupportedBuild {
                tweak,
                current_build,
                min_build,
                max_build,
            } => match max_build {
                Some(max) => write!(
                    f,
                    "{tweak} supports builds {min_build}..={max}, current is {current_build}"
                ),
                None => write!(
                    f,
                    "{tweak} requires build >= {min_build}, current is {current_build}"
                ),
            },
        }
    }
}
