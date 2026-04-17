//! Library migration — real rhs rewriting when a primitive promotes.
//!
//! After gates 6 + 7 accept a promotion, `migrate_library` does the
//! full work that makes layer N+1 possible:
//!
//! 1. Every subsumed entry (gate-4-level subsumption) is marked
//!    `ProofStatus::Subsumed(promoted)` in the registry overlay
//! 2. Every *active* library entry's rhs is walked; subterms that
//!    match the promoted rule's lhs pattern are replaced with the
//!    promoted rule's rhs form (typically a `Symbol` reference to
//!    the new primitive). Rules whose rhs actually changed are
//!    added to the registry as *new* artifacts, and the old
//!    versions marked `Subsumed(new_version)`
//! 3. The promoted artifact's status advances to `Primitive`
//! 4. A `MigrationReport` is produced capturing every rewritten +
//!    deduplicated hash, with its own content hash
//!
//! The rewriting is a single-pass (not fixed-point) walk. Multi-
//! pass rewriting until fixed-point is a Phase I+ improvement;
//! single-pass catches the vast majority of real substitutions and
//! keeps the migration cost bounded by `|library|`.

use crate::epoch::{Artifact, Registry};
use crate::eval::{pattern_match, RewriteRule};
use crate::lifecycle::{AxiomIdentity, ProofStatus};
use crate::promotion::{MigrationReport, PromotionSignal};
use crate::term::Term;

/// Walk a term; at every subterm that matches `rule.lhs`, substitute
/// the captured bindings into `rule.rhs` and return that in place
/// of the subterm. Used by `migrate_library` to rewrite library
/// rules' rhs using the promoted rule.
#[must_use]
pub fn rewrite_term_using_rule(term: &Term, rule: &RewriteRule) -> Term {
    // If the whole term matches, substitute and return.
    if let Some(bindings) = pattern_match(&rule.lhs, term) {
        let mut result = rule.rhs.clone();
        for (var_id, value) in &bindings {
            result = result.substitute(*var_id, value);
        }
        return result;
    }
    // Otherwise recurse structurally.
    match term {
        Term::Apply(f, args) => {
            let new_f = rewrite_term_using_rule(f, rule);
            let new_args: Vec<Term> = args
                .iter()
                .map(|a| rewrite_term_using_rule(a, rule))
                .collect();
            Term::Apply(Box::new(new_f), new_args)
        }
        Term::Fn(params, body) => Term::Fn(
            params.clone(),
            Box::new(rewrite_term_using_rule(body, rule)),
        ),
        Term::Symbol(id, args) => Term::Symbol(
            *id,
            args.iter().map(|a| rewrite_term_using_rule(a, rule)).collect(),
        ),
        // Leaves: no rewriting.
        Term::Var(_) | Term::Number(_) | Term::Point(_) => term.clone(),
    }
}

/// Whether a rule's rhs can be rewritten using `promoted_rule`'s
/// pattern — i.e., the rewrite changes the rhs.
#[must_use]
fn rule_rewrites_under(rule: &RewriteRule, promoted_rule: &RewriteRule) -> bool {
    let new_rhs = rewrite_term_using_rule(&rule.rhs, promoted_rule);
    new_rhs != rule.rhs
}

/// Apply a successful promotion to the library: mark subsumed
/// entries, rewrite rhs terms across active library entries to
/// reference the new primitive, advance the promoted artifact's
/// status. Returns a `MigrationReport` capturing everything.
pub fn migrate_library<R: Registry + ?Sized>(
    registry: &mut R,
    signal: &PromotionSignal,
    primitive: AxiomIdentity,
    epoch_id: u64,
) -> MigrationReport {
    // 1. Mark gate-4 subsumed entries.
    for subsumed in &signal.subsumed_hashes {
        registry.mark_status(
            *subsumed,
            ProofStatus::Subsumed(signal.artifact_hash),
        );
    }

    // 2. Fetch the promoted artifact's rule so we can use it as a
    //    rewriter. If the artifact isn't in the registry (edge
    //    case), fall back to no rewriting.
    let promoted_rule: Option<RewriteRule> = registry
        .find(signal.artifact_hash)
        .map(|a| a.rule.clone());

    let mut rewritten_hashes = Vec::new();

    if let Some(promoted_rule) = &promoted_rule {
        // 3. Walk active library entries; rewrite where possible.
        //    Collect changes first to avoid borrow conflict during
        //    registry mutation.
        struct Rewrite {
            old_hash: crate::hash::TermRef,
            new_artifact: Artifact,
        }

        let mut rewrites: Vec<Rewrite> = Vec::new();
        for artifact in registry.all() {
            // Skip the promoted artifact itself.
            if artifact.content_hash == signal.artifact_hash {
                continue;
            }
            // Skip inactive artifacts.
            let status = registry
                .status_of(artifact.content_hash)
                .unwrap_or_else(|| artifact.certificate.status.clone());
            if matches!(
                status,
                ProofStatus::Subsumed(_) | ProofStatus::Demoted(_)
            ) {
                continue;
            }
            // Attempt rewrite.
            if !rule_rewrites_under(&artifact.rule, promoted_rule) {
                continue;
            }
            let new_rhs = rewrite_term_using_rule(&artifact.rule.rhs, promoted_rule);
            let new_rule = RewriteRule {
                name: artifact.rule.name.clone(),
                lhs: artifact.rule.lhs.clone(),
                rhs: new_rhs,
            };
            // The new artifact carries the old status (not forward
            // motion — a rewrite is a refinement, not a promotion).
            let new_artifact = Artifact::seal(
                new_rule,
                epoch_id,
                artifact.certificate.clone(),
                vec![artifact.content_hash],
            );
            rewrites.push(Rewrite {
                old_hash: artifact.content_hash,
                new_artifact,
            });
        }

        // Apply the collected rewrites.
        for rw in rewrites {
            // Insert the rewritten version.
            let new_hash = rw.new_artifact.content_hash;
            registry.insert(rw.new_artifact);
            // Mark the old one as Subsumed(new).
            registry.mark_status(rw.old_hash, ProofStatus::Subsumed(new_hash));
            rewritten_hashes.push(rw.old_hash);
        }
    }

    // 4. Advance the promoted artifact to Primitive.
    registry.mark_status(
        signal.artifact_hash,
        ProofStatus::Primitive(primitive.clone()),
    );

    // 5. Build + return the report.
    MigrationReport::seal(
        primitive,
        rewritten_hashes,
        signal.subsumed_hashes.clone(),
        epoch_id,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::epoch::{AcceptanceCertificate, Artifact, InMemoryRegistry};
    use crate::eval::RewriteRule;
    use crate::hash::TermRef;
    use crate::term::Term;

    fn mk_artifact(name: &str, sym: u32) -> Artifact {
        let rule = RewriteRule {
            name: name.into(),
            lhs: Term::Symbol(sym, vec![]),
            rhs: Term::Point(sym as u64),
        };
        Artifact::seal(
            rule,
            0,
            AcceptanceCertificate::trivial_conjecture(1.0),
            vec![],
        )
    }

    fn sample_identity() -> AxiomIdentity {
        AxiomIdentity {
            target: "mathscape_core::term::Term".into(),
            name: "Promoted".into(),
            proposal_hash: TermRef([0xaa; 32]),
            typescape_coord: crate::lifecycle::TypescapeCoord::precommit(
                "mathscape_core::term::Term",
                "Promoted",
            ),
        }
    }

    #[test]
    fn migrate_marks_subsumed_entries() {
        let mut reg = InMemoryRegistry::new();
        let promoted = mk_artifact("promoted", 1);
        let sub1 = mk_artifact("sub1", 2);
        let sub2 = mk_artifact("sub2", 3);
        reg.insert(promoted.clone());
        reg.insert(sub1.clone());
        reg.insert(sub2.clone());

        let signal = PromotionSignal {
            artifact_hash: promoted.content_hash,
            subsumed_hashes: vec![sub1.content_hash, sub2.content_hash],
            cross_corpus_support: vec!["arith".into(), "diff".into()],
            rationale: "test".into(),
            epoch_id: 5,
        };

        let report = migrate_library(&mut reg, &signal, sample_identity(), 5);

        // Subsumed entries carry the new status.
        assert!(matches!(
            reg.status_of(sub1.content_hash),
            Some(ProofStatus::Subsumed(h)) if h == promoted.content_hash
        ));
        assert!(matches!(
            reg.status_of(sub2.content_hash),
            Some(ProofStatus::Subsumed(h)) if h == promoted.content_hash
        ));
        // Promoted artifact advanced to Primitive.
        assert!(matches!(
            reg.status_of(promoted.content_hash),
            Some(ProofStatus::Primitive(_))
        ));
        // Report carries the expected deduplicated list.
        assert_eq!(report.deduplicated.len(), 2);
    }

    #[test]
    fn migrate_is_deterministic() {
        let mut reg_a = InMemoryRegistry::new();
        let mut reg_b = InMemoryRegistry::new();
        let promoted = mk_artifact("promoted", 1);
        let sub = mk_artifact("sub", 2);
        reg_a.insert(promoted.clone());
        reg_a.insert(sub.clone());
        reg_b.insert(promoted.clone());
        reg_b.insert(sub.clone());

        let signal = PromotionSignal {
            artifact_hash: promoted.content_hash,
            subsumed_hashes: vec![sub.content_hash],
            cross_corpus_support: vec!["arith".into()],
            rationale: "test".into(),
            epoch_id: 1,
        };
        let identity = sample_identity();

        let r1 = migrate_library(&mut reg_a, &signal, identity.clone(), 1);
        let r2 = migrate_library(&mut reg_b, &signal, identity, 1);
        assert_eq!(r1.content_hash, r2.content_hash);
    }

    fn apply(f: Term, args: Vec<Term>) -> Term {
        Term::Apply(Box::new(f), args)
    }
    fn nat(n: u64) -> Term {
        Term::Number(crate::value::Value::Nat(n))
    }
    fn var(id: u32) -> Term {
        Term::Var(id)
    }

    fn promoted_identity_artifact() -> Artifact {
        // Promoted rule: add(?x, 0) → Symbol(42, [?x])
        // This is what a real compression promotion looks like: the
        // pattern add(_, 0) gets compressed into an invocation of
        // primitive Symbol 42.
        let rule = RewriteRule {
            name: "identity-primitive".into(),
            lhs: apply(var(2), vec![var(100), nat(0)]),
            rhs: Term::Symbol(42, vec![var(100)]),
        };
        Artifact::seal(
            rule,
            0,
            AcceptanceCertificate::trivial_conjecture(1.0),
            vec![],
        )
    }

    fn rule_with_rhs(name: &str, lhs: Term, rhs: Term) -> Artifact {
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

    #[test]
    fn rewrite_term_using_rule_handles_direct_match() {
        let rule = RewriteRule {
            name: "id".into(),
            lhs: apply(var(2), vec![var(100), nat(0)]),
            rhs: Term::Symbol(42, vec![var(100)]),
        };
        let term = apply(var(2), vec![nat(5), nat(0)]);
        let rewritten = rewrite_term_using_rule(&term, &rule);
        // add(5, 0) → Symbol(42, [5])
        assert_eq!(rewritten, Term::Symbol(42, vec![nat(5)]));
    }

    #[test]
    fn rewrite_term_using_rule_recurses_into_subterms() {
        let rule = RewriteRule {
            name: "id".into(),
            lhs: apply(var(2), vec![var(100), nat(0)]),
            rhs: Term::Symbol(42, vec![var(100)]),
        };
        // mul(add(7, 0), 3) → mul(Symbol(42, [7]), 3)
        let term = apply(var(3), vec![apply(var(2), vec![nat(7), nat(0)]), nat(3)]);
        let rewritten = rewrite_term_using_rule(&term, &rule);
        let expected = apply(
            var(3),
            vec![Term::Symbol(42, vec![nat(7)]), nat(3)],
        );
        assert_eq!(rewritten, expected);
    }

    #[test]
    fn rewrite_term_no_match_returns_term_unchanged() {
        let rule = RewriteRule {
            name: "id".into(),
            lhs: apply(var(2), vec![var(100), nat(0)]),
            rhs: Term::Symbol(42, vec![var(100)]),
        };
        let term = apply(var(3), vec![nat(1), nat(2)]); // different head
        let rewritten = rewrite_term_using_rule(&term, &rule);
        assert_eq!(rewritten, term);
    }

    #[test]
    fn migrate_rewrites_library_entries_using_promoted_rule() {
        let mut reg = InMemoryRegistry::new();
        let promoted = promoted_identity_artifact();
        // A library rule whose rhs contains the pattern add(_, 0).
        let user_rule = rule_with_rhs(
            "user",
            Term::Symbol(1, vec![]),
            apply(var(2), vec![nat(5), nat(0)]),
        );
        let user_hash = user_rule.content_hash;
        reg.insert(promoted.clone());
        reg.insert(user_rule);

        let signal = PromotionSignal {
            artifact_hash: promoted.content_hash,
            subsumed_hashes: vec![],
            cross_corpus_support: vec!["c".into()],
            rationale: "t".into(),
            epoch_id: 1,
        };
        let report = migrate_library(&mut reg, &signal, sample_identity(), 1);

        // user_rule appears in the rewritten list.
        assert!(
            report.rewritten.contains(&user_hash),
            "user rule should be rewritten; report.rewritten = {:?}",
            report.rewritten
        );
        // The old user_rule is now Subsumed(new_user_rule).
        let status = reg.status_of(user_hash).unwrap();
        match status {
            ProofStatus::Subsumed(new_hash) => {
                // The new artifact exists in the registry with the
                // rewritten rhs.
                let new_artifact = reg.find(new_hash).expect("new rewritten artifact must exist");
                // Rewritten rhs: Symbol(42, [5])
                assert_eq!(new_artifact.rule.rhs, Term::Symbol(42, vec![nat(5)]));
            }
            other => panic!("expected Subsumed status, got {other:?}"),
        }
    }

    #[test]
    fn migrate_does_not_rewrite_if_no_pattern_match() {
        let mut reg = InMemoryRegistry::new();
        let promoted = promoted_identity_artifact();
        let user_rule = rule_with_rhs(
            "user",
            Term::Symbol(9, vec![]),
            // rhs has no add(_, 0) pattern.
            apply(var(3), vec![nat(7), nat(11)]),
        );
        reg.insert(promoted.clone());
        reg.insert(user_rule);

        let signal = PromotionSignal {
            artifact_hash: promoted.content_hash,
            subsumed_hashes: vec![],
            cross_corpus_support: vec!["c".into()],
            rationale: "t".into(),
            epoch_id: 1,
        };
        let report = migrate_library(&mut reg, &signal, sample_identity(), 1);

        assert!(report.rewritten.is_empty());
    }

    #[test]
    fn migrate_rewrites_multiple_entries_and_reports_all() {
        let mut reg = InMemoryRegistry::new();
        let promoted = promoted_identity_artifact();
        reg.insert(promoted.clone());
        // Three rules that all reference add(_, 0) in their rhs.
        for i in 0..3u32 {
            reg.insert(rule_with_rhs(
                &format!("r{i}"),
                Term::Symbol(100 + i, vec![]),
                apply(var(2), vec![nat(i as u64 + 1), nat(0)]),
            ));
        }

        let signal = PromotionSignal {
            artifact_hash: promoted.content_hash,
            subsumed_hashes: vec![],
            cross_corpus_support: vec!["c".into()],
            rationale: "t".into(),
            epoch_id: 1,
        };
        let report = migrate_library(&mut reg, &signal, sample_identity(), 1);

        assert_eq!(report.rewritten.len(), 3);
    }

    #[test]
    fn migrate_skips_rewriting_the_promoted_artifact_itself() {
        let mut reg = InMemoryRegistry::new();
        let promoted = promoted_identity_artifact();
        let promoted_hash = promoted.content_hash;
        reg.insert(promoted);

        let signal = PromotionSignal {
            artifact_hash: promoted_hash,
            subsumed_hashes: vec![],
            cross_corpus_support: vec!["c".into()],
            rationale: "t".into(),
            epoch_id: 1,
        };
        let report = migrate_library(&mut reg, &signal, sample_identity(), 1);

        // The promoted artifact is not in rewritten; it's in Primitive status.
        assert!(!report.rewritten.contains(&promoted_hash));
        assert!(matches!(
            reg.status_of(promoted_hash),
            Some(ProofStatus::Primitive(_))
        ));
    }

    #[test]
    fn migrate_skips_inactive_entries() {
        let mut reg = InMemoryRegistry::new();
        let promoted = promoted_identity_artifact();
        let user_rule = rule_with_rhs(
            "user",
            Term::Symbol(1, vec![]),
            apply(var(2), vec![nat(5), nat(0)]),
        );
        let user_hash = user_rule.content_hash;
        reg.insert(promoted.clone());
        reg.insert(user_rule);
        // Mark the user rule inactive BEFORE migration.
        reg.mark_status(
            user_hash,
            ProofStatus::Demoted(crate::lifecycle::DemotionReason::StaleConjecture),
        );

        let signal = PromotionSignal {
            artifact_hash: promoted.content_hash,
            subsumed_hashes: vec![],
            cross_corpus_support: vec!["c".into()],
            rationale: "t".into(),
            epoch_id: 1,
        };
        let report = migrate_library(&mut reg, &signal, sample_identity(), 1);

        // user rule was inactive — not rewritten.
        assert!(!report.rewritten.contains(&user_hash));
    }

    #[test]
    fn migrate_preserves_registry_append_only_semantics() {
        let mut reg = InMemoryRegistry::new();
        let promoted = mk_artifact("p", 1);
        let sub = mk_artifact("s", 2);
        reg.insert(promoted.clone());
        reg.insert(sub.clone());
        let before_len = reg.len();

        let signal = PromotionSignal {
            artifact_hash: promoted.content_hash,
            subsumed_hashes: vec![sub.content_hash],
            cross_corpus_support: vec!["c".into()],
            rationale: "t".into(),
            epoch_id: 0,
        };
        migrate_library(&mut reg, &signal, sample_identity(), 0);
        // Registry size unchanged (append-only, overlay only).
        assert_eq!(reg.len(), before_len);
        // Both artifacts still accessible.
        assert!(reg.find(promoted.content_hash).is_some());
        assert!(reg.find(sub.content_hash).is_some());
    }
}
