//! Mathscape ↔ axiom-forge bridge — gates 6 and 7.
//!
//! See `pleme-io/mathscape/docs/arch/promotion-pipeline.md` and
//! `pleme-io/axiom-forge/docs/MATHSCAPE_HANDOFF.md`. This crate
//! converts a `PromotionSignal` + `Artifact` into an `AxiomProposal`,
//! runs axiom-forge's seven obligations, and — if they pass — invokes
//! the emitter to produce Rust source. Gate 7 (rustc typecheck) is
//! the caller's responsibility; the bridge returns the emitted
//! source plus a content-hash chain linking back to the mathscape
//! signal.

use axiom_forge::{
    emit::{emit_rust, EmissionError, EmissionOutput},
    proposal::{AxiomKind, AxiomProposal, FieldSpec, FieldTy},
    vector::FrozenVector,
    verify::{verify, Certificate, VerifyConfig, Violation},
};
use mathscape_core::{
    epoch::Artifact,
    lifecycle::AxiomIdentity,
    promotion::PromotionSignal,
    term::Term,
    hash::TermRef,
};

/// A successful promotion receipt. The caller (CI / operator / bridge
/// runner) passes gate 7 by actually compiling the emitted source.
pub struct PromotionReceipt {
    pub axiom_identity: AxiomIdentity,
    pub proposal: AxiomProposal,
    pub certificate: Certificate,
    pub emission: EmissionOutput,
    pub frozen_vector: FrozenVector,
}

/// Failure modes at the bridge boundary.
#[derive(Debug)]
pub enum PromotionFailure {
    /// Gate 6 rejected the proposal (one or more obligations failed).
    VerifyFailed(Vec<Violation>),
    /// Emission itself failed — rare; indicates a bug in the
    /// proposal-building step.
    EmitFailed(EmissionError),
}

impl std::fmt::Display for PromotionFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::VerifyFailed(vs) => {
                write!(f, "gate 6 failed: {} violations", vs.len())
            }
            Self::EmitFailed(e) => write!(f, "gate 6 emission failed: {e:?}"),
        }
    }
}

impl std::error::Error for PromotionFailure {}

/// Bridge configuration. Defaults place the new primitive on
/// `mathscape_core::term::Term` as an `EnumVariant`.
pub struct BridgeConfig {
    pub target: String,
    pub kind: AxiomKind,
    pub verify_config: VerifyConfig,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            target: "mathscape_core::term::Term".into(),
            kind: AxiomKind::EnumVariant,
            verify_config: VerifyConfig::default(),
        }
    }
}

/// Convert a PascalCase identifier from a mathscape rule name. Falls
/// back to `Axiom{hex_prefix}` when the rule name doesn't yield a
/// valid PascalCase string.
fn proposal_name_from_rule_name(name: &str, hash: TermRef) -> String {
    let mut out = String::new();
    let mut cap = true;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            if cap && ch.is_ascii_alphabetic() {
                out.extend(ch.to_uppercase());
                cap = false;
            } else {
                out.push(ch);
            }
        } else {
            cap = true;
        }
    }
    // axiom-forge requires name to start with a letter; fall back on bad input.
    if out.is_empty() || !out.chars().next().unwrap().is_ascii_alphabetic() {
        let h = hash.as_bytes();
        return format!("Axiom{:02x}{:02x}{:02x}{:02x}", h[0], h[1], h[2], h[3]);
    }
    out
}

/// Build an `AxiomProposal` from a mathscape PromotionSignal + the
/// Artifact it points at.
///
/// For v0, rules with zero free variables produce nullary variants
/// (no fields). Rules with free variables in their `lhs` are not yet
/// supported at the bridge — the proposal is rejected up-front so
/// axiom-forge never sees malformed input. Phase upgrade will infer
/// field types from lhs-variable usage.
pub fn signal_to_proposal(
    signal: &PromotionSignal,
    artifact: &Artifact,
    config: &BridgeConfig,
) -> AxiomProposal {
    let name = proposal_name_from_rule_name(&artifact.rule.name, artifact.content_hash);
    let doc = if signal.rationale.is_empty() {
        format!(
            "Mathscape-promoted primitive from rule {}. Signal epoch {}.",
            artifact.rule.name, signal.epoch_id
        )
    } else {
        signal.rationale.clone()
    };
    let mut proposal = AxiomProposal::new(config.kind.clone(), config.target.clone(), name)
        .with_doc(doc);
    // Include subsumption and cross-corpus facts as asserted invariants.
    if !signal.subsumed_hashes.is_empty() {
        proposal = proposal.with_invariant(format!(
            "subsumes: {} library entries",
            signal.subsumed_hashes.len()
        ));
    }
    if !signal.cross_corpus_support.is_empty() {
        proposal = proposal.with_invariant(format!(
            "cross-corpus: {}",
            signal.cross_corpus_support.join(", ")
        ));
    }
    proposal
}

/// Collect the free-variable ids in a term, in deterministic order.
fn free_vars(term: &Term) -> Vec<u32> {
    fn walk(t: &Term, seen: &mut std::collections::BTreeSet<u32>) {
        match t {
            Term::Var(id) => {
                seen.insert(*id);
            }
            Term::Fn(_, body) => walk(body, seen),
            Term::Apply(f, args) => {
                walk(f, seen);
                for a in args {
                    walk(a, seen);
                }
            }
            Term::Symbol(_, args) => {
                for a in args {
                    walk(a, seen);
                }
            }
            _ => {}
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    walk(term, &mut seen);
    seen.into_iter().collect()
}

/// Hard cap on field count — axiom-forge's `MAX_FIELDS` is 8; stay
/// under it to keep the bridge deterministic.
const MAX_BRIDGED_FIELDS: usize = 8;

/// Infer a `FieldSpec` per free variable in the rule's lhs by
/// scanning the rhs for the variable's usage shape. Rules:
///
/// - variable appears in the rhs inside a `Number` context → `U64`
///   (mathscape natural numbers map to `u64` in the emitted Rust)
/// - variable appears at the top level of the rhs without wrappers
///   → `String` (default — it could be anything carried through)
/// - variable is used inside a `Symbol` or `Apply` head position
///   → `String` (same default; could be refined later to `SelfRef`)
/// - variable does not appear in the rhs at all → `String` (bridge
///   cannot infer anything about it)
///
/// This is still a coarse inference — Phase G+ upgrades it to full
/// type tracking via takumi / typescape. v1 is enough to produce
/// non-String fields where the pattern is obvious.
fn fields_from_rule(rule_lhs: &Term, rule_rhs: &Term) -> Option<Vec<FieldSpec>> {
    let vars = free_vars(rule_lhs);
    if vars.len() > MAX_BRIDGED_FIELDS {
        return None;
    }
    Some(
        vars.into_iter()
            .enumerate()
            .map(|(i, v)| FieldSpec {
                name: format!("arg{i}"),
                ty: infer_field_ty(v, rule_rhs),
                doc: format!("mathscape free variable slot {i}"),
            })
            .collect(),
    )
}

/// Infer a single variable's FieldTy from its usage in a term.
fn infer_field_ty(var_id: u32, term: &Term) -> FieldTy {
    // First look for numeric usage — if the var appears as an
    // argument to an Apply whose head is BUILTIN_ADD/SUCC/MUL, it is
    // numeric.
    if appears_in_numeric_position(var_id, term) {
        return FieldTy::U64;
    }
    // Otherwise fall back to String. Future upgrades: detect Vec/
    // Option contexts.
    FieldTy::String
}

/// Heuristic: the variable appears somewhere in the term as a
/// direct argument to a known numeric builtin (BUILTIN_SUCC=1,
/// BUILTIN_ADD=2, BUILTIN_MUL=3 from mathscape-core::eval).
fn appears_in_numeric_position(var_id: u32, term: &Term) -> bool {
    const NUMERIC_BUILTINS: [u32; 3] = [1, 2, 3];
    match term {
        Term::Apply(f, args) => {
            let head_is_numeric = matches!(f.as_ref(), Term::Var(id) if NUMERIC_BUILTINS.contains(id));
            if head_is_numeric
                && args
                    .iter()
                    .any(|a| matches!(a, Term::Var(id) if *id == var_id))
            {
                return true;
            }
            // Recurse.
            appears_in_numeric_position(var_id, f)
                || args.iter().any(|a| appears_in_numeric_position(var_id, a))
        }
        Term::Fn(_, body) => appears_in_numeric_position(var_id, body),
        Term::Symbol(_, args) => args.iter().any(|a| appears_in_numeric_position(var_id, a)),
        _ => false,
    }
}

/// Run the bridge end-to-end: build the proposal, run axiom-forge's
/// gate 6, and on success emit Rust source + frozen vector.
///
/// Free variables in `artifact.rule.lhs` become `FieldTy::String`
/// fields on the generated enum variant. Rules with more than
/// `MAX_BRIDGED_FIELDS` free variables are rejected (axiom-forge's
/// own cap).
pub fn run_promotion(
    signal: &PromotionSignal,
    artifact: &Artifact,
    config: &BridgeConfig,
) -> Result<PromotionReceipt, PromotionFailure> {
    // Pre-check: bridge's arity cap.
    let fields = fields_from_rule(&artifact.rule.lhs, &artifact.rule.rhs).ok_or_else(|| {
        PromotionFailure::VerifyFailed(vec![Violation::new(
            axiom_forge::verify::ProofObligation::FieldCountBounded,
            format!(
                "bridge: rule has more than {MAX_BRIDGED_FIELDS} free vars; arity cap exceeded"
            ),
        )])
    })?;

    let mut proposal = signal_to_proposal(signal, artifact, config);
    for f in fields {
        proposal = proposal.with_field(f);
    }

    let certificate = verify(&proposal, &config.verify_config)
        .map_err(PromotionFailure::VerifyFailed)?;

    let emission = emit_rust(&proposal, &certificate).map_err(PromotionFailure::EmitFailed)?;

    let frozen_vector = FrozenVector::from_emission(&proposal, &certificate, &emission);

    let axiom_identity = AxiomIdentity {
        target: proposal.target.clone(),
        name: proposal.name.clone(),
        // `proposal_hash` from axiom-forge's Certificate is a
        // `ContentHash([u8; 32])`. Convert to mathscape's TermRef.
        proposal_hash: TermRef(certificate.proposal_hash.0),
        typescape_coord: mathscape_core::lifecycle::TypescapeCoord::precommit(
            &proposal.target,
            &proposal.name,
        ),
    };

    Ok(PromotionReceipt {
        axiom_identity,
        proposal,
        certificate,
        emission,
        frozen_vector,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mathscape_core::{
        epoch::AcceptanceCertificate,
        eval::RewriteRule,
        promotion::PromotionSignal,
        term::Term,
    };

    fn nullary_artifact(name: &str) -> Artifact {
        let rule = RewriteRule {
            name: name.into(),
            lhs: Term::Symbol(1, vec![]),
            rhs: Term::Point(0),
        };
        Artifact::seal(
            rule,
            0,
            AcceptanceCertificate::trivial_conjecture(1.0),
            vec![],
        )
    }

    fn signal_for(artifact: &Artifact) -> PromotionSignal {
        PromotionSignal {
            artifact_hash: artifact.content_hash,
            subsumed_hashes: vec![TermRef([0xa; 32]), TermRef([0xb; 32]), TermRef([0xc; 32])],
            cross_corpus_support: vec!["arith".into(), "diff".into()],
            rationale: "gates 4+5 cleared via test fabrication".into(),
            epoch_id: 42,
        }
    }

    #[test]
    fn name_converts_kebab_to_pascal() {
        let h = TermRef([0; 32]);
        assert_eq!(
            proposal_name_from_rule_name("add-identity", h),
            "AddIdentity"
        );
        assert_eq!(proposal_name_from_rule_name("s_007", h), "S007");
        assert_eq!(
            proposal_name_from_rule_name("foo.bar-baz", h),
            "FooBarBaz"
        );
    }

    #[test]
    fn name_falls_back_on_numeric_start() {
        let h = TermRef([0xde, 0xad, 0xbe, 0xef, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                         0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        // "123abc" has no alphabetic prefix, so it's empty-initial uppercase-skipped
        // ... actually let me just verify: digit "1" is alphanumeric but not alphabetic,
        // so it stays lowercase; "a" triggers cap; "b", "c" stay. Result: "1Abc" —
        // doesn't start with letter, so we get fallback.
        let out = proposal_name_from_rule_name("123abc", h);
        assert!(out.starts_with("Axiom"));
        assert_eq!(out.len(), "Axiom".len() + 8); // 8 hex chars
    }

    #[test]
    fn bridge_emits_string_fields_for_rules_with_free_vars() {
        use mathscape_core::test_helpers::var;
        let rule = RewriteRule {
            name: "with-slot".into(),
            // lhs has one free variable — becomes arg0: String.
            lhs: Term::Apply(Box::new(Term::Symbol(1, vec![])), vec![var(42)]),
            rhs: var(42),
        };
        let artifact = Artifact::seal(
            rule,
            0,
            AcceptanceCertificate::trivial_conjecture(1.0),
            vec![],
        );
        let signal = signal_for(&artifact);
        let receipt = run_promotion(&signal, &artifact, &BridgeConfig::default())
            .expect("bridge must accept single-slot rules");
        assert_eq!(receipt.proposal.fields.len(), 1);
        assert_eq!(receipt.proposal.fields[0].name, "arg0");
        assert_eq!(receipt.proposal.fields[0].ty, FieldTy::String);
        assert!(receipt.emission.declaration.contains("arg0"));
    }

    #[test]
    fn bridge_infers_u64_for_numeric_position_vars() {
        use mathscape_core::test_helpers::{apply, var};
        // Rule: (Symbol(1, [?100])) => add(?100, 1)
        // ?100 appears as an argument to BUILTIN_ADD (id=2) in the rhs
        // → should be inferred as U64.
        let rule = RewriteRule {
            name: "numeric-slot".into(),
            lhs: Term::Apply(Box::new(Term::Symbol(1, vec![])), vec![var(100)]),
            rhs: apply(
                var(2),
                vec![var(100), Term::Number(mathscape_core::value::Value::Nat(1))],
            ),
        };
        let artifact = Artifact::seal(
            rule,
            0,
            AcceptanceCertificate::trivial_conjecture(1.0),
            vec![],
        );
        let signal = signal_for(&artifact);
        let receipt = run_promotion(&signal, &artifact, &BridgeConfig::default())
            .expect("numeric-position rule should pass gates 6");
        assert_eq!(receipt.proposal.fields.len(), 1);
        assert_eq!(
            receipt.proposal.fields[0].ty,
            FieldTy::U64,
            "var in numeric-builtin argument position should be inferred as U64"
        );
    }

    #[test]
    fn bridge_rejects_rules_over_arity_cap() {
        use mathscape_core::test_helpers::var;
        // 9 free vars > MAX_BRIDGED_FIELDS (8) → rejected.
        let rule = RewriteRule {
            name: "too-many".into(),
            lhs: Term::Apply(
                Box::new(Term::Symbol(1, vec![])),
                (0..9).map(var).collect(),
            ),
            rhs: Term::Point(0),
        };
        let artifact = Artifact::seal(
            rule,
            0,
            AcceptanceCertificate::trivial_conjecture(1.0),
            vec![],
        );
        let signal = signal_for(&artifact);
        let result = run_promotion(&signal, &artifact, &BridgeConfig::default());
        assert!(matches!(result, Err(PromotionFailure::VerifyFailed(_))));
    }

    #[test]
    fn v0_bridge_emits_rust_for_nullary_rule() {
        let artifact = nullary_artifact("FlashAttention");
        let signal = signal_for(&artifact);
        let result = run_promotion(&signal, &artifact, &BridgeConfig::default())
            .expect("expected gate 6 to pass for nullary rule");
        // Emission should contain the declared variant.
        assert!(
            result.emission.declaration.contains("FlashAttention"),
            "emission declaration should mention the variant name, got: {}",
            result.emission.declaration
        );
        // FrozenVector pairs canonical_text + b3sum.
        assert!(!result.frozen_vector.canonical_text.is_empty());
        assert_eq!(result.frozen_vector.b3sum_hex.len(), 64);
        // Identity has the expected target.
        assert_eq!(
            result.axiom_identity.target,
            "mathscape_core::term::Term"
        );
        assert_eq!(result.axiom_identity.name, "FlashAttention");
    }

    #[test]
    fn bridge_preserves_signal_rationale_in_doc() {
        let artifact = nullary_artifact("GoodName");
        let mut signal = signal_for(&artifact);
        signal.rationale = "specific gate-clearing reason".into();
        let receipt = run_promotion(&signal, &artifact, &BridgeConfig::default()).unwrap();
        assert_eq!(receipt.proposal.doc, "specific gate-clearing reason");
    }

    #[test]
    fn bridge_lists_subsumption_and_cross_corpus_invariants() {
        let artifact = nullary_artifact("GoodName");
        let signal = signal_for(&artifact);
        let receipt = run_promotion(&signal, &artifact, &BridgeConfig::default()).unwrap();
        let invs = &receipt.proposal.asserted_invariants;
        assert!(invs.iter().any(|i| i.contains("subsumes")));
        assert!(invs.iter().any(|i| i.contains("cross-corpus")));
    }
}
