//! The acquisition interpreter — `apply<E: AcquisitionEnvironment>`.
//!
//! The TYPED-SPEC + INTERPRETER triplet's third artifact: a working
//! interpreter that walks an authored `(defacquisition …)` spec and
//! drives the acquisition DAG forward, with **every side effect behind a
//! mockable `AcquisitionEnvironment` trait**. The trait IS the
//! testability contract — the sample seal, the (destination) proof
//! seal, the trusted-primitive migration, and the (destination)
//! equivalence gate all go through it, so tests mock them and never need
//! a real e-graph / Lean kernel / migration engine.
//!
//! The destination surfaces — `seal_proof` (§3.5.1 `Proof`) and
//! `check_equivalence` (§3.5.2 `EquivalenceChecked`) — return
//! `SpecError::Interp { phase }` **typed gaps** by default, never a
//! fake `Ok`. A real environment overrides them once saturation/Lean
//! and the per-fold-back gate land.

use crate::spec::{AcquisitionOp, AcquisitionSpec};
use crate::types::{
    AbsorbDecision, AcquisitionDAG, AcquisitionState, Dl, FoldBack, Frontier, Layer, ProofKind,
    SealWitness, SemanticsPreservingWitness, SpecError,
};
use mathscape_core::eval::RewriteRule;

/// The side-effect seam. Real implementations satisfy it against the
/// primitive evaluator / e-graph / Lean; tests mock it. Every method
/// that could reach the world lives here.
pub trait AcquisitionEnvironment {
    /// SHIPPED floor. Certify a rule sound by `k` random samples through
    /// the primitive evaluator (`validate_semantically`). Returns the
    /// sampled `SealWitness` iff all `k` agree, else `None` (rejected).
    /// Seal-tier `only-mitigated`.
    fn seal_sample(&self, rule: &RewriteRule, k: usize) -> Option<SealWitness>;

    /// DESTINATION. Certify a rule with a proof-grade seal — saturated
    /// e-graph equivalence (parse-time-rejected) or kernel-checked Lean
    /// (truly-unrepresentable). **Default: a typed gap.** `lean_export`
    /// is a `sorry`-stub, so a real proof seal is not yet buildable;
    /// this returns `SpecError::Interp { phase: "proof-seal" }` rather
    /// than fabricate a pass.
    fn seal_proof(&self, _rule: &RewriteRule, _kind: ProofKind) -> Result<SealWitness, SpecError> {
        Err(SpecError::Interp { phase: "proof-seal" })
    }

    /// SHIPPED. Estimate the total description length of a substrate
    /// (vocabulary + library) — the MDL currency `AbsorbDecision` gates
    /// on. Mocked in tests; a real impl reads the corpus/library DL the
    /// way `AcceptanceCertificate` does.
    fn description_length(&self, layers: &[Layer]) -> Dl;

    /// SHIPPED floor. Run the trusted-primitive migration
    /// (`migrate_library`): substitute the promoted primitive's
    /// `lhs`-pattern into earlier layers' `rhs`, trusting the confirmed
    /// primitive is meaning-preserving. Returns how many subterms were
    /// rewritten. Seal-tier `only-mitigated`.
    fn migrate_trusted(&self, primitive: &Layer, earlier: &[Layer]) -> usize;

    /// DESTINATION. Per-fold-back equivalence check —
    /// `check_equivalence(old_rhs, new_rhs, probes, SATURATED)`.
    /// **Default: a typed gap.** Until this lands, INV-2 is
    /// `only-mitigated`; this returns
    /// `SpecError::Interp { phase: "equivalence-gate" }` rather than
    /// claim the rewrite was checked.
    fn check_equivalence(&self, _primitive: &Layer, _earlier: &[Layer]) -> Result<bool, SpecError> {
        Err(SpecError::Interp {
            phase: "equivalence-gate",
        })
    }
}

/// The outcome of applying an acquisition spec: the DAG advanced by the
/// spec's ops, plus a per-op audit trail.
#[derive(Debug, Clone)]
pub struct AcquisitionOutcome {
    pub dag: AcquisitionDAG,
    pub steps: Vec<StepReport>,
}

/// One op's effect, for audit.
#[derive(Debug, Clone)]
pub enum StepReport {
    /// A layer was sealed (by the sampled floor) and committed.
    LayerSealed { tier: crate::types::Tier },
    /// A candidate layer was rejected by the seal (samples disagreed).
    LayerRejected,
    /// A layer was absorbed — the absorb paid for itself in bits.
    Absorbed { bits_saved: u64 },
    /// A candidate layer was NOT absorbed — it did not lower total DL.
    NotSignificant,
    /// A fold-back ran under the trusted-primitive witness.
    FoldedBack { rewritten: usize },
    /// The frontier advanced (a primitive raised `K`).
    FrontierAdvanced,
}

/// Walk the spec's phases against an environment, driving the
/// acquisition DAG. `apply(spec, env) -> Result<AcquisitionOutcome,
/// SpecError>`.
///
/// Every op respects the typed invariants of the border: a layer must
/// seal to be committed (INV-1); a fold-back needs a semantics-
/// preserving witness (INV-2); an absorb constructs only on net-negative
/// ΔDL (§1.5). The Proof / EquivalenceChecked destinations surface as
/// typed gaps from the environment.
pub fn apply<E: AcquisitionEnvironment>(
    spec: &AcquisitionSpec,
    env: &E,
) -> Result<AcquisitionOutcome, SpecError> {
    let mut dag = AcquisitionDAG::new();
    dag.commit(AcquisitionState::genesis(spec.genesis_ceiling));
    let mut steps = Vec::new();

    for op in &spec.ops {
        match op {
            AcquisitionOp::SealLayer { rule, k } => {
                match env.seal_sample(rule, *k) {
                    Some(seal) => {
                        let tier = seal.tier();
                        let layer = Layer {
                            rule: rule.clone(),
                            seal,
                        };
                        let head = dag
                            .head()
                            .ok_or(SpecError::Invariant { what: "no head node" })?;
                        let mut layers = head.layers.clone();
                        let parent = Some(head.content_hash);
                        let frontier = head.frontier;
                        layers.push(layer);
                        dag.commit(AcquisitionState::seal(parent, frontier, layers));
                        steps.push(StepReport::LayerSealed { tier });
                    }
                    None => steps.push(StepReport::LayerRejected),
                }
            }

            AcquisitionOp::Absorb { rule, k } => {
                // A candidate is absorbed iff (a) it seals, and (b)
                // folding it back strictly lowers total DL.
                let Some(seal) = env.seal_sample(rule, *k) else {
                    steps.push(StepReport::LayerRejected);
                    continue;
                };
                let candidate = Layer {
                    rule: rule.clone(),
                    seal,
                };
                let head = dag
                    .head()
                    .ok_or(SpecError::Invariant { what: "no head node" })?;
                let old_dl = env.description_length(&head.layers);
                let mut folded = head.layers.clone();
                folded.push(candidate.clone());
                let new_dl = env.description_length(&folded);
                match AbsorbDecision::try_new(old_dl, new_dl) {
                    Some(decision) => {
                        let parent = Some(head.content_hash);
                        let frontier = head.frontier;
                        dag.commit(AcquisitionState::seal(parent, frontier, folded));
                        steps.push(StepReport::Absorbed {
                            bits_saved: decision.bits_saved(),
                        });
                    }
                    None => steps.push(StepReport::NotSignificant),
                }
            }

            AcquisitionOp::FoldBack { primitive_index } => {
                let head = dag
                    .head()
                    .ok_or(SpecError::Invariant { what: "no head node" })?;
                let primitive =
                    head.layers
                        .get(*primitive_index)
                        .cloned()
                        .ok_or(SpecError::Invariant {
                            what: "fold-back primitive index out of range",
                        })?;
                let earlier: Vec<Layer> = head
                    .layers
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != *primitive_index)
                    .map(|(_, l)| l.clone())
                    .collect();
                let rewritten = env.migrate_trusted(&primitive, &earlier);
                // Trusted-primitive witness (the shipped floor). The
                // EquivalenceChecked destination is a typed gap in the
                // env; we do NOT round up to it here.
                let _witness = SemanticsPreservingWitness::TrustedPrimitive {
                    via_seal: primitive.seal.clone(),
                };
                steps.push(StepReport::FoldedBack { rewritten });
            }

            AcquisitionOp::AdvanceFrontier { to } => {
                let head = dag
                    .head()
                    .ok_or(SpecError::Invariant { what: "no head node" })?;
                let frontier = head.frontier.advance(*to);
                let parent = Some(head.content_hash);
                let layers = head.layers.clone();
                dag.commit(AcquisitionState::seal(parent, frontier, layers));
                steps.push(StepReport::FrontierAdvanced);
            }
        }
    }

    Ok(AcquisitionOutcome { dag, steps })
}

/// Build a `FoldBack` value under the trusted-primitive floor. A helper
/// for consumers that want the typed `FoldBack` directly (the
/// interpreter's `FoldBack` op records the migration but does not
/// re-park a `FoldBack` value on the DAG — that lives on the migration
/// report shape).
#[must_use]
pub fn fold_back_trusted(primitive: Layer, rewritten: usize) -> FoldBack {
    let via_seal = primitive.seal.clone();
    FoldBack {
        via_primitive: primitive,
        rewritten,
        witness: SemanticsPreservingWitness::TrustedPrimitive { via_seal },
    }
}

/// Attempt the DESTINATION equivalence-checked fold-back. Returns the
/// typed gap today (`SpecError::Interp { phase: "equivalence-gate" }`)
/// unless the environment overrides `check_equivalence`. Never
/// fabricates the `EquivalenceChecked` witness.
pub fn fold_back_checked<E: AcquisitionEnvironment>(
    primitive: Layer,
    earlier: &[Layer],
    env: &E,
) -> Result<FoldBack, SpecError> {
    let saturated = env.check_equivalence(&primitive, earlier)?;
    let rewritten = env.migrate_trusted(&primitive, earlier);
    Ok(FoldBack {
        via_primitive: primitive,
        rewritten,
        witness: SemanticsPreservingWitness::EquivalenceChecked { saturated },
    })
}

/// Attempt the DESTINATION proof-grade seal. Returns the typed gap today
/// unless the environment overrides `seal_proof`. Never fabricates the
/// `Proof` witness. `_ = Frontier` reference keeps the destination wired
/// to the frontier's incompleteness framing.
pub fn seal_proof_layer<E: AcquisitionEnvironment>(
    rule: &RewriteRule,
    kind: ProofKind,
    env: &E,
) -> Result<Layer, SpecError> {
    let _ = Frontier::open(crate::types::Complexity(0));
    let seal = env.seal_proof(rule, kind)?;
    Ok(Layer {
        rule: rule.clone(),
        seal,
    })
}
