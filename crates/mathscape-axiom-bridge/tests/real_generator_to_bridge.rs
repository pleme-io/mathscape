//! End-to-end: CompressionGenerator's real output (not hand-crafted
//! nullary rules) survives the PromotionGate + bridge and produces
//! axiom-forge Rust source. This is the most important integration
//! test — it proves the machine's actual production path works.

use mathscape_axiom_bridge::{run_promotion, BridgeConfig};
use mathscape_compress::{extract::ExtractConfig, CompressionGenerator};
use mathscape_core::{
    epoch::{AcceptanceCertificate, Artifact, Generator, InMemoryRegistry, Registry},
    promotion_gate::{ArtifactHistory, PromotionGate, ThresholdGate},
    term::Term,
    value::Value,
};
use std::collections::BTreeSet;

fn var(id: u32) -> Term {
    Term::Var(id)
}
fn nat(n: u64) -> Term {
    Term::Number(Value::Nat(n))
}
fn apply(f: Term, args: Vec<Term>) -> Term {
    Term::Apply(Box::new(f), args)
}

fn corpus_with_clear_pattern() -> Vec<Term> {
    // Every term is add(?x, 0) — strong shared structure. The
    // anti-unifier should produce a rule with lhs containing a free
    // variable, which the bridge handles via string-field inference.
    vec![
        apply(var(2), vec![nat(3), nat(0)]),
        apply(var(2), vec![nat(5), nat(0)]),
        apply(var(2), vec![nat(7), nat(0)]),
        apply(var(2), vec![nat(11), nat(0)]),
    ]
}

fn fabricated_history() -> ArtifactHistory {
    // In a real run this comes from CorpusLog.history_for after the
    // artifact has accumulated evidence across epochs and corpora.
    ArtifactHistory {
        corpus_matches: BTreeSet::from(["arith".to_string(), "combinators".to_string()]),
        epochs_alive: 100,
        usage_in_window: 42,
    }
}

#[test]
fn compression_generator_output_survives_bridge() {
    // 1. Real CompressionGenerator produces a RewriteRule.
    let mut g = CompressionGenerator::new(
        ExtractConfig {
            min_shared_size: 2,
            min_matches: 2,
            max_new_rules: 1,
        },
        1,
    );
    let candidates = g.propose(0, &corpus_with_clear_pattern(), &[]);
    assert!(
        !candidates.is_empty(),
        "generator must produce at least one candidate for this corpus"
    );
    let candidate = candidates.into_iter().next().unwrap();

    // 2. Wrap as Artifact (as the real Emitter would).
    let artifact = Artifact::seal(
        candidate.rule.clone(),
        0,
        AcceptanceCertificate::trivial_conjecture(1.0),
        vec![],
    );

    // 3. Sanity: the generated rule has free variables in lhs (that's
    // the whole point — it's a pattern). The bridge's String-field
    // fallback must handle this.
    let mut free_var_count = 0;
    fn walk(t: &Term, c: &mut usize) {
        use std::collections::BTreeSet;
        fn w(t: &Term, s: &mut BTreeSet<u32>) {
            match t {
                Term::Var(id) => {
                    s.insert(*id);
                }
                Term::Fn(_, b) => w(b, s),
                Term::Apply(f, a) => {
                    w(f, s);
                    for x in a {
                        w(x, s)
                    }
                }
                Term::Symbol(_, a) => {
                    for x in a {
                        w(x, s)
                    }
                }
                _ => {}
            }
        }
        let mut s = BTreeSet::new();
        w(t, &mut s);
        *c = s.len();
    }
    walk(&candidate.rule.lhs, &mut free_var_count);
    // If the generator produces a nullary rule that's also fine; we
    // just note it so future test readers understand the shape being
    // exercised.
    println!("candidate rule lhs free-var count: {free_var_count}");

    // 4. PromotionGate evaluates. We relax k=0 so the subsumption
    // heuristic doesn't block (real mathscape uses e-graph
    // subsumption for this). n=2 matches our fabricated history.
    let gate = ThresholdGate::new(0, 2);
    let history = fabricated_history();
    let signal = gate
        .evaluate(&artifact, &[artifact.clone()], &history, 10)
        .expect("gate with k=0 + n=2 should fire with 2-corpus history");

    // 5. Bridge runs axiom-forge.
    let receipt = run_promotion(&signal, &artifact, &BridgeConfig::default())
        .expect("bridge must accept real compression-generator output");

    // 6. The emission should be non-empty and name-matched. For rules
    // with free variables, the declaration must include the arg0
    // field.
    assert!(!receipt.emission.declaration.is_empty());
    if free_var_count > 0 {
        assert!(
            receipt.emission.declaration.contains("arg0"),
            "free-var rule emission should contain arg0 field, got: {}",
            receipt.emission.declaration
        );
        assert_eq!(receipt.proposal.fields.len(), free_var_count);
    }

    // 7. FrozenVector integrity check.
    assert_eq!(receipt.frozen_vector.b3sum_hex.len(), 64);

    // 8. Every field's FieldTy is String (v0 inference).
    for f in &receipt.proposal.fields {
        assert_eq!(f.ty, axiom_forge::proposal::FieldTy::String);
    }
}

#[test]
fn registry_state_post_integration_stays_coherent() {
    // After the bridge fires and the caller (real orchestration
    // code) stashes the artifact in a registry, the Merkle root of
    // the registry is stable and reproducible.
    let mut g = CompressionGenerator::new(ExtractConfig::default(), 1);
    let candidates = g.propose(0, &corpus_with_clear_pattern(), &[]);
    let mut registry = InMemoryRegistry::new();
    for cand in candidates {
        let artifact = Artifact::seal(
            cand.rule,
            0,
            AcceptanceCertificate::trivial_conjecture(1.0),
            vec![],
        );
        registry.insert(artifact);
    }
    let root = registry.root();

    // Build a parallel registry with the same inputs — roots must match.
    let mut g2 = CompressionGenerator::new(ExtractConfig::default(), 1);
    let candidates2 = g2.propose(0, &corpus_with_clear_pattern(), &[]);
    let mut registry2 = InMemoryRegistry::new();
    for cand in candidates2 {
        let artifact = Artifact::seal(
            cand.rule,
            0,
            AcceptanceCertificate::trivial_conjecture(1.0),
            vec![],
        );
        registry2.insert(artifact);
    }
    assert_eq!(root, registry2.root());
}
