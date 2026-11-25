//! Strongly typed identifiers.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Stable identifier of a tweak manifest, e.g. `privacy.disable-telemetry`.
///
/// Format: lowercase ASCII alphanumeric segments separated by `.` or `-`,
/// 3..=64 characters. Validated by [`TweakId::is_valid`] during manifest
/// validation (deserialization itself is lenient so that all issues can be
/// reported at once).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(transparent)]
pub struct TweakId(String);

impl TweakId {
    pub const MIN_LEN: usize = 3;
    pub const MAX_LEN: usize = 64;

    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns `true` when the identifier follows the documented format.
    pub fn is_valid(&self) -> bool {
        let s = self.0.as_str();
        if !(Self::MIN_LEN..=Self::MAX_LEN).contains(&s.len()) {
            return false;
        }
        let is_sep = |c: char| c == '.' || c == '-';
        if s.starts_with(is_sep) || s.ends_with(is_sep) {
            return false;
        }
        let mut prev_sep = false;
        for c in s.chars() {
            if is_sep(c) {
                if prev_sep {
                    return false;
                }
                prev_sep = true;
            } else if c.is_ascii_lowercase() || c.is_ascii_digit() {
                prev_sep = false;
            } else {
                return false;
            }
        }
        true
    }
}

impl fmt::Display for TweakId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Identifier of an apply/rollback transaction (opaque string; the engine
/// decides the generation scheme, e.g. UUIDv7).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(transparent)]
pub struct TransactionId(String);

impl TransactionId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TransactionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Canonical, provider-prefixed identity of a single system resource, e.g.
/// `registry:hklm\software\policies\...\allowtelemetry`.
///
/// Two actions touching the same [`ResourceId`] contend for the same resource
/// (used by the conflict guard and the drift detector). Produced only by
/// [`crate::ActionSpec::resource_id`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(transparent)]
pub struct ResourceId(String);

impl ResourceId {
    pub(crate) fn from_parts(prefix: &str, body: &str) -> Self {
        Self(format!("{prefix}:{body}"))
    }

    /// Reconstructs an id previously produced by [`crate::ActionSpec::resource_id`]
    /// (e.g. when loading from storage).
    pub fn from_stored(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ResourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tweak_id_format() {
        for ok in ["privacy.disable-telemetry", "abc", "gaming.gpu-hags-2"] {
            assert!(TweakId::new(ok).is_valid(), "{ok}");
        }
        for bad in [
            "",
            "ab",
            "Privacy.x",
            "a..b",
            "-abc",
            "abc.",
            "a b c",
            "ğüş",
            &"a".repeat(65),
        ] {
            assert!(!TweakId::new(bad).is_valid(), "{bad}");
        }
    }
}
