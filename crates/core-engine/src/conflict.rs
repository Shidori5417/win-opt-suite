//! `ConflictGuard`: detects selections that must not (or should not) be
//! applied together or on this machine. Pure function of its inputs.

use std::collections::{BTreeMap, BTreeSet};

use shared_types::{
    ConflictKind, ConflictWarning, ResourceId, ResourceState, Severity, SystemFacts, TweakId,
    TweakManifest,
};

/// Stateless conflict analysis.
#[derive(Debug, Clone, Copy, Default)]
pub struct ConflictGuard;

impl ConflictGuard {
    /// Analyses the selected `manifests` against the machine `facts`.
    ///
    /// The result is deterministic and independent of the input order: groups
    /// appear as declared conflicts, resource contention, excluded
    /// environments, unsupported builds; each group is sorted by tweak id
    /// (contention by resource id). Duplicate manifests with the same id are
    /// analysed once.
    pub fn check(manifests: &[TweakManifest], facts: &SystemFacts) -> Vec<ConflictWarning> {
        let mut by_id: BTreeMap<&TweakId, &TweakManifest> = BTreeMap::new();
        for m in manifests {
            by_id.entry(&m.id).or_insert(m);
        }

        let mut out = Vec::new();
        declared_conflicts(&by_id, &mut out);
        resource_contention(&by_id, &mut out);
        excluded_environments(&by_id, facts, &mut out);
        unsupported_builds(&by_id, facts, &mut out);
        out
    }
}

fn declared_conflicts(by_id: &BTreeMap<&TweakId, &TweakManifest>, out: &mut Vec<ConflictWarning>) {
    // Normalised (smaller id, larger id) pairs so each pair is reported once
    // regardless of which side declared it.
    let mut pairs: BTreeSet<(&TweakId, &TweakId)> = BTreeSet::new();
    for (id, m) in by_id {
        for other in &m.conflicts_with {
            if other == *id {
                continue;
            }
            if let Some((other_id, _)) = by_id.get_key_value(other) {
                pairs.insert(if id <= other_id {
                    (id, other_id)
                } else {
                    (other_id, id)
                });
            }
        }
    }
    out.extend(pairs.into_iter().map(|(a, b)| ConflictWarning {
        severity: Severity::Blocking,
        kind: ConflictKind::DeclaredConflict {
            tweak: a.clone(),
            other: b.clone(),
        },
    }));
}

fn resource_contention(by_id: &BTreeMap<&TweakId, &TweakManifest>, out: &mut Vec<ConflictWarning>) {
    // resource -> (tweak -> desired state); the first action of a tweak on a
    // resource wins (duplicates inside one manifest are a validation error).
    let mut targets: BTreeMap<ResourceId, BTreeMap<&TweakId, Option<ResourceState>>> =
        BTreeMap::new();
    for (id, m) in by_id {
        for action in &m.actions {
            targets
                .entry(action.resource_id())
                .or_default()
                .entry(*id)
                .or_insert_with(|| action.desired_state());
        }
    }
    for (resource, tweaks) in targets {
        if tweaks.len() < 2 {
            continue;
        }
        let mut states = tweaks.values();
        let differs = states
            .next()
            .is_some_and(|first| states.any(|s| s != first));
        out.push(ConflictWarning {
            severity: if differs {
                Severity::Blocking
            } else {
                Severity::Info
            },
            kind: ConflictKind::ResourceContention {
                resource,
                tweaks: tweaks.keys().map(|t| (*t).clone()).collect(),
            },
        });
    }
}

fn excluded_environments(
    by_id: &BTreeMap<&TweakId, &TweakManifest>,
    facts: &SystemFacts,
    out: &mut Vec<ConflictWarning>,
) {
    let tags = facts.environment_tags();
    for (id, m) in by_id {
        let mut reported = Vec::new();
        for tag in &m.excluded_on {
            if tags.contains(tag) && !reported.contains(tag) {
                reported.push(*tag);
                out.push(ConflictWarning {
                    severity: Severity::Blocking,
                    kind: ConflictKind::ExcludedEnvironment {
                        tweak: (*id).clone(),
                        tag: *tag,
                    },
                });
            }
        }
    }
}

fn unsupported_builds(
    by_id: &BTreeMap<&TweakId, &TweakManifest>,
    facts: &SystemFacts,
    out: &mut Vec<ConflictWarning>,
) {
    for (id, m) in by_id {
        if !m.supports_build(facts.windows_build) {
            out.push(ConflictWarning {
                severity: Severity::Blocking,
                kind: ConflictKind::UnsupportedBuild {
                    tweak: (*id).clone(),
                    current_build: facts.windows_build,
                    min_build: m.min_windows_build,
                    max_build: m.max_windows_build,
                },
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use shared_types::EnvironmentTag;

    use super::*;
    use crate::testing::{facts, manifest, reg};

    fn kinds(w: &[ConflictWarning]) -> Vec<(&Severity, &ConflictKind)> {
        w.iter().map(|w| (&w.severity, &w.kind)).collect()
    }

    #[test]
    fn clean_selection_has_no_warnings() {
        let a = manifest("tweak.a", vec![reg("A", 1)]);
        let b = manifest("tweak.b", vec![reg("B", 1)]);
        assert!(ConflictGuard::check(&[a, b], &facts()).is_empty());
    }

    #[test]
    fn declared_conflict_in_both_directions_is_reported_once() {
        let mut a = manifest("tweak.a", vec![reg("A", 1)]);
        let mut b = manifest("tweak.b", vec![reg("B", 1)]);
        a.conflicts_with = vec![TweakId::new("tweak.b")];
        b.conflicts_with = vec![TweakId::new("tweak.a")];
        let w = ConflictGuard::check(&[b.clone(), a.clone()], &facts());
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].severity, Severity::Blocking);
        assert_eq!(
            w[0].kind,
            ConflictKind::DeclaredConflict {
                tweak: TweakId::new("tweak.a"),
                other: TweakId::new("tweak.b"),
            }
        );
        // one-directional declaration is enough
        b.conflicts_with.clear();
        assert_eq!(ConflictGuard::check(&[a, b], &facts()).len(), 1);
    }

    #[test]
    fn declared_conflict_with_unselected_tweak_is_ignored() {
        let mut a = manifest("tweak.a", vec![reg("A", 1)]);
        a.conflicts_with = vec![TweakId::new("tweak.zzz")];
        assert!(ConflictGuard::check(&[a], &facts()).is_empty());
    }

    #[test]
    fn contention_blocking_vs_info() {
        let a = manifest("tweak.a", vec![reg("X", 1)]);
        let b = manifest("tweak.b", vec![reg("x", 2)]);
        let c = manifest("tweak.c", vec![reg("Y", 5)]);
        let d = manifest("tweak.d", vec![reg("Y", 5)]);
        let w = ConflictGuard::check(&[d, c, b, a], &facts());
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].severity, Severity::Blocking);
        assert!(matches!(
            &w[0].kind,
            ConflictKind::ResourceContention { resource, tweaks }
                if resource.as_str() == r"registry:hklm\software\test\x"
                && tweaks == &[TweakId::new("tweak.a"), TweakId::new("tweak.b")]
        ));
        assert_eq!(w[1].severity, Severity::Info);
        assert!(matches!(
            &w[1].kind,
            ConflictKind::ResourceContention { tweaks, .. }
                if tweaks == &[TweakId::new("tweak.c"), TweakId::new("tweak.d")]
        ));
    }

    #[test]
    fn excluded_environment_is_blocking() {
        let mut a = manifest("tweak.a", vec![reg("A", 1)]);
        a.excluded_on = vec![
            EnvironmentTag::Laptop,
            EnvironmentTag::VirtualMachine,
            EnvironmentTag::VirtualMachine,
        ];
        let mut f = facts();
        f.is_virtual_machine = true;
        let w = ConflictGuard::check(&[a.clone()], &f);
        assert_eq!(
            kinds(&w),
            vec![(
                &Severity::Blocking,
                &ConflictKind::ExcludedEnvironment {
                    tweak: TweakId::new("tweak.a"),
                    tag: EnvironmentTag::VirtualMachine
                }
            )]
        );
        assert!(ConflictGuard::check(&[a], &facts()).is_empty());
    }

    #[test]
    fn unsupported_build_is_blocking() {
        let mut a = manifest("tweak.a", vec![reg("A", 1)]);
        a.max_windows_build = Some(22631);
        let w = ConflictGuard::check(&[a], &facts());
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].severity, Severity::Blocking);
        assert_eq!(
            w[0].kind,
            ConflictKind::UnsupportedBuild {
                tweak: TweakId::new("tweak.a"),
                current_build: 26100,
                min_build: 22000,
                max_build: Some(22631),
            }
        );
    }

    #[test]
    fn output_is_independent_of_input_order() {
        let mut a = manifest("tweak.a", vec![reg("X", 1)]);
        let b = manifest("tweak.b", vec![reg("X", 2)]);
        let mut c = manifest("tweak.c", vec![reg("Z", 1)]);
        a.conflicts_with = vec![TweakId::new("tweak.c")];
        c.max_windows_build = Some(22100);
        let one = ConflictGuard::check(&[a.clone(), b.clone(), c.clone()], &facts());
        let two = ConflictGuard::check(&[c, b, a], &facts());
        assert_eq!(one, two);
        assert_eq!(one.len(), 3);
    }
}
