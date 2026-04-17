//! Reduction — the layer-relative meter for layer-deep discovery.
//!
//! **"Maximally reduced" does not mean the same thing at every
//! layer.** At layer 0 (base corpus over base primitives) reduction
//! is about pairwise subsumption + status advancement. At deeper
//! layers it may also include meta-compression (library-on-library),
//! axis-independence (no two rules redundantly express the same
//! cross-operator invariant), dimensional orthogonality, and other
//! axioms specific to the expanded primitive set.
//!
//! This module therefore takes a [`ReductionPolicy`] that declares
//! *which* checks to run. The `layer_0_default()` policy captures
//! what makes sense at the base layer; downstream layers provide
//! their own policies. The verdict is *relative to the policy that
//! produced it* — never universal.
//!
//! A library is **maximally reduced under a policy P** when none of
//! the checks enabled in P can find a barrier. That is the stopping
//! condition for the layer associated with P: only once it holds can
//! the layer-K+1 expansion fire by migrating the library into the
//! new primitive vocabulary.
//!
//! See `docs/arch/axiomatization-pressure.md` and
//! `docs/arch/forced-realization.md` for the theory. This module
//! is the *computable* form of the reinforcement-plateau concept
//! for any given layer's notion of reduction.

use crate::epoch::{Artifact, Registry};
use crate::eval::pattern_match;
use crate::hash::TermRef;
use crate::lifecycle::ProofStatus;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// A specific obstacle to calling the library maximally reduced
/// *under the policy that emitted this verdict*. The same library
/// can be reduced under one policy and unreduced under another —
/// see `ReductionPolicy`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReductionBarrier {
    /// Active pair where one rule's lhs pattern-matches the other's
    /// lhs — the subsumer is strictly more general and the subsumed
    /// should be marked `Subsumed(subsumer)`.
    SubsumablePair {
        subsumer: TermRef,
        subsumed: TermRef,
    },
    /// An active rule's proof status has room to advance per the
    /// policy's `advance_ceiling`.
    AdvancableStatus {
        artifact: TermRef,
        current: ProofStatus,
    },
}

/// Policy declaring *which* reduction checks to run. Different
/// layers of the mathscape machine have different notions of
/// maximally-reduced; each provides its own policy.
///
/// The `layer_0_default()` policy captures the base-layer semantics:
/// pairwise subsumption + status advancement toward Axiomatized.
/// Deeper layers will extend this with meta-compression and
/// cross-operator invariant checks, surfacing additional barrier
/// variants as they mature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReductionPolicy {
    /// Report pairs where one rule's lhs pattern-subsumes another.
    pub check_pairwise_subsumption: bool,
    /// Report rules whose status is below `advance_ceiling`.
    pub check_status_advancement: bool,
    /// Rules below this status count as "advancable" when the check
    /// is enabled. Default: `Axiomatized`. Deeper layers may raise
    /// this to `Promoted` (demand every rule be promotion-ready) or
    /// lower it to `Verified` (allow unexported Verified rules).
    pub advance_ceiling: ProofStatus,
}

impl ReductionPolicy {
    /// The base-layer policy. Captures what "maximally reduced"
    /// means when the substrate is the original Term enum and rules
    /// are direct rewrites over base primitives.
    #[must_use]
    pub fn layer_0_default() -> Self {
        Self {
            check_pairwise_subsumption: true,
            check_status_advancement: true,
            advance_ceiling: ProofStatus::Axiomatized,
        }
    }

    /// A stricter policy that also demands every rule be promoted
    /// (not just axiomatized). Useful as the stopping condition for
    /// layer boundaries where a layer transitions into the next.
    #[must_use]
    pub fn layer_boundary() -> Self {
        Self {
            check_pairwise_subsumption: true,
            check_status_advancement: true,
            advance_ceiling: ProofStatus::Promoted,
        }
    }
}

impl Default for ReductionPolicy {
    fn default() -> Self {
        Self::layer_0_default()
    }
}

/// Verdict on a library's reduction state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReductionVerdict {
    /// No further reduction possible under the current policy.
    Reduced,
    /// One or more barriers listed. `Vec::is_empty()` implies
    /// `Reduced`, but we keep the variants distinct so callers can
    /// pattern-match cleanly.
    Barriers(Vec<ReductionBarrier>),
}

impl ReductionVerdict {
    #[must_use]
    pub fn is_reduced(&self) -> bool {
        matches!(self, ReductionVerdict::Reduced)
    }

    #[must_use]
    pub fn barrier_count(&self) -> usize {
        match self {
            ReductionVerdict::Reduced => 0,
            ReductionVerdict::Barriers(v) => v.len(),
        }
    }
}

/// An artifact is *active* iff its current status (taking any
/// overlay into account) is not terminal — i.e., not Subsumed and
/// not Demoted.
fn is_active(registry: &dyn Registry, artifact: &Artifact) -> bool {
    let status = registry
        .status_of(artifact.content_hash)
        .unwrap_or_else(|| artifact.certificate.status.clone());
    !matches!(
        status,
        ProofStatus::Subsumed(_) | ProofStatus::Demoted(_)
    )
}

/// Return the active rules plus their current statuses. Reads the
/// registry's overlay when present, falling back to the certificate
/// status on the artifact.
fn active_artifacts_with_status(
    registry: &dyn Registry,
) -> Vec<(TermRef, ProofStatus, &Artifact)> {
    registry
        .all()
        .iter()
        .filter_map(|a| {
            let status = registry
                .status_of(a.content_hash)
                .unwrap_or_else(|| a.certificate.status.clone());
            if matches!(
                status,
                ProofStatus::Subsumed(_) | ProofStatus::Demoted(_)
            ) {
                None
            } else {
                Some((a.content_hash, status, a))
            }
        })
        .collect()
}

/// Check the library for maximally-reduced status *under the given
/// policy*. The verdict is valid only for that policy. See
/// `ReductionPolicy` for the rationale.
///
/// Algorithm:
///   - Collect active artifacts (status not Subsumed / Demoted)
///   - If `check_pairwise_subsumption`: for every ordered pair,
///     check if lhs pattern-match detects subsumption; report
///     deduplicated pairs
///   - If `check_status_advancement`: for every active artifact,
///     report if its status is below `advance_ceiling`
#[must_use]
pub fn check_reduction(
    registry: &dyn Registry,
    policy: &ReductionPolicy,
) -> ReductionVerdict {
    let mut barriers = Vec::new();
    let active = active_artifacts_with_status(registry);

    if policy.check_pairwise_subsumption {
        let mut reported_subsumed: HashSet<[u8; 32]> = HashSet::new();
        for (ai, _, a_art) in &active {
            for (bi, _, b_art) in &active {
                if ai == bi {
                    continue;
                }
                // a subsumes b iff a.lhs pattern-matches b.lhs.
                if pattern_match(&a_art.rule.lhs, &b_art.rule.lhs).is_some()
                    && reported_subsumed.insert(*bi.as_bytes())
                {
                    barriers.push(ReductionBarrier::SubsumablePair {
                        subsumer: *ai,
                        subsumed: *bi,
                    });
                }
            }
        }
    }

    if policy.check_status_advancement {
        let ceiling_rank = policy.advance_ceiling.rank();
        for (hash, status, _) in &active {
            if status.rank() < ceiling_rank {
                barriers.push(ReductionBarrier::AdvancableStatus {
                    artifact: *hash,
                    current: status.clone(),
                });
            }
        }
    }

    if barriers.is_empty() {
        ReductionVerdict::Reduced
    } else {
        ReductionVerdict::Barriers(barriers)
    }
}

/// Convenience: check with `ReductionPolicy::layer_0_default()`. The
/// most common caller; deeper layers should use `check_reduction`
/// with their own policy.
#[must_use]
pub fn check_maximally_reduced(registry: &dyn Registry) -> ReductionVerdict {
    check_reduction(registry, &ReductionPolicy::layer_0_default())
}

/// Reduction pressure — the leading indicator described in
/// `docs/arch/collapse-and-surprise.md`.
///
/// Defined as the count of unresolved subsumable pairs per active
/// artifact. When it rises above a policy-specific threshold, a
/// collapse event is scheduled: the reinforcement pass will start
/// firing Subsumption events that release the pressure.
///
/// Zero pressure means the library is either empty or maximally
/// reduced under its current policy — no collapse is imminent.
#[must_use]
pub fn reduction_pressure(registry: &dyn Registry) -> f64 {
    let verdict = check_maximally_reduced(registry);
    let active = active_artifacts_with_status(registry).len().max(1) as f64;
    match verdict {
        ReductionVerdict::Reduced => 0.0,
        ReductionVerdict::Barriers(bs) => {
            let pairs = bs
                .iter()
                .filter(|b| matches!(b, ReductionBarrier::SubsumablePair { .. }))
                .count();
            pairs as f64 / active
        }
    }
}

/// Enumerate subsumer→subsumed pairs the reinforcement pass can
/// act on. Canonical ordering: for each unordered pair {a, b} where
/// both subsume each other, the subsumer with the smaller
/// content_hash is chosen, keeping the emission deterministic.
///
/// Used by `Epoch::run_reinforce` to fire collapse events.
#[must_use]
pub fn detect_subsumption_pairs(
    registry: &dyn Registry,
) -> Vec<(TermRef, TermRef)> {
    let active = active_artifacts_with_status(registry);
    let mut claimed: HashSet<[u8; 32]> = HashSet::new();
    let mut out: Vec<(TermRef, TermRef)> = Vec::new();
    for (ai, _, a_art) in &active {
        for (bi, _, b_art) in &active {
            if ai == bi {
                continue;
            }
            // a subsumes b: a.lhs pattern-matches b.lhs.
            if pattern_match(&a_art.rule.lhs, &b_art.rule.lhs).is_some() {
                // Ensure the same unordered pair isn't emitted
                // twice. Canonicalize by sorting — the subsumed
                // entry is claimed; further discoveries of the
                // same subsumed are skipped.
                if claimed.insert(*bi.as_bytes()) {
                    out.push((*ai, *bi));
                }
            }
        }
    }
    // Sort by (subsumer, subsumed) bytes for fully deterministic
    // emission order across runs.
    out.sort_by(|x, y| {
        x.0.as_bytes()
            .cmp(y.0.as_bytes())
            .then_with(|| x.1.as_bytes().cmp(y.1.as_bytes()))
    });
    out
}

/// Convenience summary: a single struct with counts and a verdict
/// suitable for CLI display or MCP serialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReductionSummary {
    pub verdict: ReductionVerdict,
    pub active_count: usize,
    pub terminal_count: usize,
    pub subsumable_pairs: usize,
    pub advancable_artifacts: usize,
}

impl ReductionSummary {
    #[must_use]
    pub fn of(registry: &dyn Registry) -> Self {
        let all = registry.all();
        let total = all.len();
        let active_count = all
            .iter()
            .filter(|a| is_active(registry, a))
            .count();
        let terminal_count = total - active_count;
        let verdict = check_maximally_reduced(registry);
        let (subsumable_pairs, advancable_artifacts) = match &verdict {
            ReductionVerdict::Reduced => (0, 0),
            ReductionVerdict::Barriers(bs) => {
                let mut s = 0;
                let mut a = 0;
                for b in bs {
                    match b {
                        ReductionBarrier::SubsumablePair { .. } => s += 1,
                        ReductionBarrier::AdvancableStatus { .. } => a += 1,
                    }
                }
                (s, a)
            }
        };
        Self {
            verdict,
            active_count,
            terminal_count,
            subsumable_pairs,
            advancable_artifacts,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::epoch::{AcceptanceCertificate, Artifact, InMemoryRegistry};
    use crate::eval::RewriteRule;
    use crate::lifecycle::{AxiomIdentity, DemotionReason, ProofStatus, TypescapeCoord};
    use crate::term::Term;
    use crate::value::Value;

    fn apply(f: Term, args: Vec<Term>) -> Term {
        Term::Apply(Box::new(f), args)
    }
    fn nat(n: u64) -> Term {
        Term::Number(Value::Nat(n))
    }
    fn var(id: u32) -> Term {
        Term::Var(id)
    }

    fn mk(name: &str, lhs: Term, rhs: Term) -> Artifact {
        Artifact::seal(
            RewriteRule {
                name: name.into(),
                lhs,
                rhs,
            },
            0,
            AcceptanceCertificate::trivial_conjecture(1.0),
            vec![],
        )
    }

    fn axiomatized_cert() -> AcceptanceCertificate {
        let mut c = AcceptanceCertificate::trivial_conjecture(1.0);
        c.status = ProofStatus::Axiomatized;
        c
    }

    fn mk_axiomatized(name: &str, lhs: Term, rhs: Term) -> Artifact {
        Artifact::seal(
            RewriteRule {
                name: name.into(),
                lhs,
                rhs,
            },
            0,
            axiomatized_cert(),
            vec![],
        )
    }

    #[test]
    fn empty_registry_is_reduced() {
        let reg = InMemoryRegistry::new();
        let verdict = check_maximally_reduced(&reg);
        assert!(verdict.is_reduced());
    }

    #[test]
    fn single_conjectured_rule_is_advancable() {
        let mut reg = InMemoryRegistry::new();
        reg.insert(mk("r", Term::Symbol(1, vec![]), nat(1)));
        let verdict = check_maximally_reduced(&reg);
        let ReductionVerdict::Barriers(bs) = verdict else {
            panic!("expected barriers, got Reduced");
        };
        assert!(bs.iter().any(|b| matches!(
            b,
            ReductionBarrier::AdvancableStatus { current: ProofStatus::Conjectured, .. }
        )));
    }

    #[test]
    fn single_axiomatized_rule_is_reduced() {
        let mut reg = InMemoryRegistry::new();
        reg.insert(mk_axiomatized("r", Term::Symbol(1, vec![]), nat(1)));
        let verdict = check_maximally_reduced(&reg);
        assert!(verdict.is_reduced(), "single Axiomatized rule should be reduced, got {verdict:?}");
    }

    #[test]
    fn detects_subsumable_pair() {
        // add(?x, 0) subsumes add(42, 0) — pattern match from
        // candidate lhs to other lhs succeeds.
        let mut reg = InMemoryRegistry::new();
        // Both at Axiomatized so status advancement is not a barrier.
        reg.insert(mk_axiomatized(
            "add-id",
            apply(var(2), vec![var(100), nat(0)]),
            var(100),
        ));
        reg.insert(mk_axiomatized(
            "add-42",
            apply(var(2), vec![nat(42), nat(0)]),
            nat(42),
        ));
        let verdict = check_maximally_reduced(&reg);
        let ReductionVerdict::Barriers(bs) = verdict else {
            panic!("expected Barriers, got Reduced");
        };
        // Exactly one subsumable pair reported (the specific one
        // can also be flipped, but the subsumed hash must dedupe).
        let pair_count = bs
            .iter()
            .filter(|b| matches!(b, ReductionBarrier::SubsumablePair { .. }))
            .count();
        assert_eq!(pair_count, 1);
    }

    #[test]
    fn subsumed_rules_are_excluded_from_pair_check() {
        let mut reg = InMemoryRegistry::new();
        let a = mk_axiomatized(
            "add-id",
            apply(var(2), vec![var(100), nat(0)]),
            var(100),
        );
        let b = mk_axiomatized(
            "add-42",
            apply(var(2), vec![nat(42), nat(0)]),
            nat(42),
        );
        let a_hash = a.content_hash;
        let b_hash = b.content_hash;
        reg.insert(a);
        reg.insert(b);
        // Mark b as Subsumed — it should no longer count for pair detection.
        reg.mark_status(b_hash, ProofStatus::Subsumed(a_hash));
        let verdict = check_maximally_reduced(&reg);
        assert!(verdict.is_reduced(), "after marking subsumed, verdict should be Reduced; got {verdict:?}");
    }

    #[test]
    fn demoted_rules_are_excluded_from_advance_check() {
        let mut reg = InMemoryRegistry::new();
        let a = mk("r", Term::Symbol(1, vec![]), nat(1));
        let h = a.content_hash;
        reg.insert(a);
        reg.mark_status(h, ProofStatus::Demoted(DemotionReason::StaleConjecture));
        let verdict = check_maximally_reduced(&reg);
        assert!(
            verdict.is_reduced(),
            "demoted artifact should not block reduction; got {verdict:?}"
        );
    }

    #[test]
    fn promoted_primitive_counts_as_terminal_advance() {
        let mut reg = InMemoryRegistry::new();
        let mut cert = AcceptanceCertificate::trivial_conjecture(1.0);
        cert.status = ProofStatus::Primitive(AxiomIdentity {
            target: "t::T".into(),
            name: "X".into(),
            proposal_hash: TermRef([0; 32]),
            typescape_coord: TypescapeCoord::precommit("t::T", "X"),
        });
        reg.insert(Artifact::seal(
            RewriteRule {
                name: "r".into(),
                lhs: Term::Symbol(1, vec![]),
                rhs: nat(1),
            },
            0,
            cert,
            vec![],
        ));
        let verdict = check_maximally_reduced(&reg);
        assert!(verdict.is_reduced(), "Primitive status should be terminal for advancement; got {verdict:?}");
    }

    #[test]
    fn summary_counts_are_consistent() {
        let mut reg = InMemoryRegistry::new();
        // 2 Axiomatized, 1 Conjectured, 1 Subsumed
        reg.insert(mk_axiomatized(
            "a",
            apply(var(2), vec![var(100), nat(0)]),
            var(100),
        ));
        reg.insert(mk_axiomatized(
            "b",
            apply(var(2), vec![nat(42), nat(0)]),
            nat(42),
        ));
        reg.insert(mk("c", Term::Symbol(3, vec![]), nat(3)));
        let d = mk("d", Term::Symbol(4, vec![]), nat(4));
        let d_hash = d.content_hash;
        reg.insert(d);
        reg.mark_status(d_hash, ProofStatus::Subsumed(TermRef([0xaa; 32])));

        let summary = ReductionSummary::of(&reg);
        assert_eq!(summary.active_count, 3); // a, b, c
        assert_eq!(summary.terminal_count, 1); // d
        // a and b subsume each other (one pair after dedup)
        assert!(summary.subsumable_pairs >= 1);
        // c is Conjectured → advancable
        assert!(summary.advancable_artifacts >= 1);
        assert!(!summary.verdict.is_reduced());
    }

    #[test]
    fn verdict_serde_round_trips() {
        let v = ReductionVerdict::Reduced;
        let bytes = bincode::serialize(&v).unwrap();
        let decoded: ReductionVerdict = bincode::deserialize(&bytes).unwrap();
        assert!(decoded.is_reduced());

        let v2 = ReductionVerdict::Barriers(vec![ReductionBarrier::AdvancableStatus {
            artifact: TermRef([0; 32]),
            current: ProofStatus::Conjectured,
        }]);
        let bytes = bincode::serialize(&v2).unwrap();
        let decoded: ReductionVerdict = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.barrier_count(), 1);
    }
}
