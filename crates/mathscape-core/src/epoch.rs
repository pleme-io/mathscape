//! Epoch — the generate/prove/emit/register quad.
//!
//! The epoch loop is the unit of progress. Each step produces zero or
//! more [`Artifact`]s that enter the [`Registry`] monotonically. Every
//! Artifact carries a BLAKE3 content hash of its canonical form and a
//! list of `parent_hashes` pointing to the library entries from which
//! it was derived. The registry is not just a library — it is a
//! cryptographically-chained derivation DAG.
//!
//! See `docs/arch/epoch-quad.md` for the design narrative.

use crate::eval::RewriteRule;
use crate::hash::TermRef;
use crate::lifecycle::ProofStatus;
use crate::term::{SymbolId, Term};
use serde::{Deserialize, Serialize};

/// A candidate rewrite rule entering the pipeline. The rule may have
/// been produced by anti-unification, enumeration, evolution, or RL —
/// the pipeline treats all sources uniformly.
///
/// `origin` is a free-form tag (e.g. `"compress/antiunify"`,
/// `"evolve/mutate"`, `"rl/sample"`) that audits can use to attribute
/// artifacts back to the generator that produced them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    pub rule: RewriteRule,
    pub origin: String,
}

/// The verdict on a candidate.
#[derive(Debug, Clone)]
pub enum Verdict {
    Accept(AcceptanceCertificate),
    Reject(Vec<Rejection>),
}

/// Evidence that a candidate passed the prover. The three reward axes
/// (`compression_ratio`, `condensation_ratio`, `coverage_delta`) are
/// kept separately so the regime detector and promotion gate can read
/// each axis independently — see `docs/arch/condensation-reward.md`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcceptanceCertificate {
    /// Scalar composite score under the prover's active weighting.
    pub score: f64,
    /// Corpus compression: `1 - DL(C|L_new) / DL(C|L_old)` ∈ [0, 1].
    pub compression_ratio: f64,
    /// Library shrinkage: `(|L_old| - |L_new|) / |L_old|` ∈ [0, 1].
    pub condensation_ratio: f64,
    /// Coverage preservation: `matches_new - matches_old`. Must be ≥ 0
    /// for the prover to accept (hard constraint — see MDL doc).
    pub coverage_delta: i64,
    /// Statistical novelty (generality × irreducibility).
    pub novelty: f64,
    /// Recursive-compression score when the library is treated as
    /// corpus-for-itself.
    pub meta_compression: f64,
    /// The single-currency reward value (bits saved under MDL).
    pub delta_dl: f64,
    /// Position in the lifecycle lattice. See `lifecycle::ProofStatus`.
    pub status: ProofStatus,
    /// Present when `status >= Verified` — hash of the equivalence-class
    /// representative produced by the e-graph.
    pub equivalence_hash: Option<TermRef>,
}

impl AcceptanceCertificate {
    /// A minimal certificate used by tests and stub provers. Real
    /// provers compute the axes from corpus + library snapshots.
    #[must_use]
    pub fn trivial_conjecture(score: f64) -> Self {
        Self {
            score,
            compression_ratio: 0.0,
            condensation_ratio: 0.0,
            coverage_delta: 0,
            novelty: 0.0,
            meta_compression: 0.0,
            delta_dl: score,
            status: ProofStatus::Conjectured,
            equivalence_hash: None,
        }
    }
}

/// Why a candidate was rejected — enough context for diagnostics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rejection {
    pub reason: String,
    pub threshold: f64,
    pub actual: f64,
}

/// An accepted candidate, materialized and content-addressed.
///
/// The pair `(rule, content_hash)` is the cross-language portability
/// contract (the same role axiom-forge's `FrozenVector` plays).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub rule: RewriteRule,
    pub epoch_id: u64,
    pub certificate: AcceptanceCertificate,
    /// BLAKE3 of the canonical bincode of `(rule, epoch_id, certificate)`.
    pub content_hash: TermRef,
    /// Hashes of library entries this artifact was derived from —
    /// i.e., artifacts whose `SymbolId` appears inside `rule.rhs`.
    /// Empty when the rule is derived only from primitives.
    pub parent_hashes: Vec<TermRef>,
}

impl Artifact {
    /// The canonical constructor. Computes `content_hash` from
    /// `(rule, epoch_id, certificate)` — the hash invariant cannot be
    /// bypassed short of mutating fields post-construction. Prefer
    /// this over building `Artifact` directly.
    #[must_use]
    pub fn seal(
        rule: RewriteRule,
        epoch_id: u64,
        certificate: AcceptanceCertificate,
        parent_hashes: Vec<TermRef>,
    ) -> Self {
        let content_hash = Self::canonical_hash(&rule, epoch_id, &certificate);
        Self {
            rule,
            epoch_id,
            certificate,
            content_hash,
            parent_hashes,
        }
    }

    /// Canonical content hash — same inputs produce the same bytes
    /// produce the same hash.
    pub fn canonical_hash(
        rule: &RewriteRule,
        epoch_id: u64,
        cert: &AcceptanceCertificate,
    ) -> TermRef {
        let bytes = bincode::serialize(&(rule, epoch_id, cert))
            .expect("Artifact canonical_hash: bincode serialization infallible for owned types");
        TermRef::from_bytes(&bytes)
    }

    /// Walk `rule.rhs` collecting every `Symbol` id referenced, then
    /// look those ids up in the library to produce parent hashes.
    /// The returned list is sorted + deduplicated for determinism.
    pub fn parents_from_rhs(rule: &RewriteRule, library: &[Artifact]) -> Vec<TermRef> {
        let mut ids = std::collections::BTreeSet::<SymbolId>::new();
        collect_symbol_ids(&rule.rhs, &mut ids);

        let mut hashes: Vec<TermRef> = library
            .iter()
            .filter_map(|a| {
                let sym_id = rule_symbol_id(&a.rule)?;
                ids.contains(&sym_id).then_some(a.content_hash)
            })
            .collect();
        hashes.sort_by_key(|h| *h.as_bytes());
        hashes.dedup();
        hashes
    }
}

fn collect_symbol_ids(t: &Term, out: &mut std::collections::BTreeSet<SymbolId>) {
    match t {
        Term::Point(_) | Term::Number(_) | Term::Var(_) => {}
        Term::Fn(_, body) => collect_symbol_ids(body, out),
        Term::Apply(f, args) => {
            collect_symbol_ids(f, out);
            for a in args {
                collect_symbol_ids(a, out);
            }
        }
        Term::Symbol(id, args) => {
            out.insert(*id);
            for a in args {
                collect_symbol_ids(a, out);
            }
        }
    }
}

/// The rewrite rule's symbol id — the top-level `Symbol(id, _)` this
/// rule introduces on its lhs. Returns `None` if the lhs isn't a
/// symbol form (shouldn't happen for compression rules but we are
/// lenient here rather than panicking).
fn rule_symbol_id(rule: &RewriteRule) -> Option<SymbolId> {
    match &rule.lhs {
        Term::Symbol(id, _) => Some(*id),
        _ => None,
    }
}

/// Audit record of one epoch. Carries the event stream + summary
/// counters + the action that ran + the regime the machine believes
/// it is in. See `docs/arch/machine-synthesizer.md`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EpochTrace {
    pub epoch_id: u64,
    pub proposals: usize,
    pub accepted: usize,
    pub rejected: usize,
    pub artifact_hashes: Vec<TermRef>,
    /// The full event sequence in the order they occurred this epoch.
    /// `V(epoch) = Σ event.delta_dl()`.
    pub events: Vec<crate::event::Event>,
    /// Which of Reinforce / Discover / Promote / Migrate this epoch ran.
    pub action: Option<crate::control::EpochAction>,
    /// Regime the allocator believed was active entering this epoch.
    pub regime: Option<crate::control::Regime>,
}

impl EpochTrace {
    /// Total ΔDL across all events — the unified epoch score.
    #[must_use]
    pub fn total_delta_dl(&self) -> f64 {
        self.events.iter().map(crate::event::Event::delta_dl).sum()
    }

    /// ΔDL contributed by events in a specific category.
    #[must_use]
    pub fn delta_dl_by(&self, category: crate::event::EventCategory) -> f64 {
        self.events
            .iter()
            .filter(|e| e.category() == category)
            .map(crate::event::Event::delta_dl)
            .sum()
    }
}

/// Propose candidates from a population / search strategy.
///
/// The generator sees the corpus (input evidence) and the current
/// library (to avoid re-proposing what's already accepted). The
/// returned list is a batch of candidates to be considered one-by-one
/// by the prover in the same epoch.
pub trait Generator {
    fn propose(
        &mut self,
        epoch_id: u64,
        corpus: &[Term],
        library: &[Artifact],
    ) -> Vec<Candidate>;
}

/// Decide whether a candidate is worth emitting.
pub trait Prover {
    fn prove(&self, candidate: &Candidate, corpus: &[Term], library: &[Artifact]) -> Verdict;
}

/// Materialize an accepted candidate as an Artifact.
///
/// The emitter is responsible for:
/// 1. Building a canonical [`RewriteRule`] from the candidate
/// 2. Computing `content_hash` via [`Artifact::canonical_hash`]
/// 3. Computing `parent_hashes` via [`Artifact::parents_from_rhs`] or
///    domain-specific logic
pub trait Emitter {
    fn emit(
        &self,
        candidate: &Candidate,
        cert: &AcceptanceCertificate,
        epoch_id: u64,
        library: &[Artifact],
    ) -> Option<Artifact>;
}

/// Append-only knowledge store. Artifacts are never removed; the
/// current status of an artifact is tracked via an overlay map that
/// implementers can choose to persist independently.
pub trait Registry {
    fn insert(&mut self, artifact: Artifact);
    fn all(&self) -> &[Artifact];

    fn len(&self) -> usize {
        self.all().len()
    }
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Update the active-status overlay for an artifact. Default impl
    /// is a no-op so simple storage backends need no overlay. Persistent
    /// backends override to store the overlay alongside artifacts.
    fn mark_status(&mut self, _artifact_hash: TermRef, _status: ProofStatus) {}

    /// Current status for an artifact, taking the overlay into
    /// account. Default impl falls back to the artifact's embedded
    /// `certificate.status`.
    fn status_of(&self, artifact_hash: TermRef) -> Option<ProofStatus> {
        self.all()
            .iter()
            .find(|a| a.content_hash == artifact_hash)
            .map(|a| a.certificate.status.clone())
    }

    /// Find an artifact by content hash. Default is a linear scan;
    /// backends with indexing can override.
    fn find(&self, artifact_hash: TermRef) -> Option<&Artifact> {
        self.all().iter().find(|a| a.content_hash == artifact_hash)
    }
}

/// The unified epoch. Owns nothing; composes four roles.
pub struct Epoch<G, P, E, R> {
    pub generator: G,
    pub prover: P,
    pub emitter: E,
    pub registry: R,
    pub epoch_id: u64,
}

impl<G, P, E, R> Epoch<G, P, E, R>
where
    G: Generator,
    P: Prover,
    E: Emitter,
    R: Registry,
{
    pub fn new(generator: G, prover: P, emitter: E, registry: R) -> Self {
        Self {
            generator,
            prover,
            emitter,
            registry,
            epoch_id: 0,
        }
    }

    /// Run one discovery epoch. Returns an audit trace populated with
    /// `Event::Proposal` / `Event::Accept` / `Event::Reject` entries.
    ///
    /// This is the v0 dispatch — only the Discovery pass is
    /// implemented. Reinforce / Promote / Migrate dispatch lands in
    /// later phases of `docs/arch/realization-plan.md`.
    pub fn step(&mut self, corpus: &[Term]) -> EpochTrace {
        use crate::event::Event;
        let proposals = self
            .generator
            .propose(self.epoch_id, corpus, self.registry.all());
        let mut trace = EpochTrace {
            epoch_id: self.epoch_id,
            proposals: proposals.len(),
            action: Some(crate::control::EpochAction::Discover),
            ..Default::default()
        };

        for cand in &proposals {
            trace.events.push(Event::Proposal {
                candidate: cand.clone(),
                delta_dl: 0.0,
            });
            let verdict = self.prover.prove(cand, corpus, self.registry.all());
            match verdict {
                Verdict::Accept(cert) => {
                    let library = self.registry.all();
                    if let Some(artifact) = self.emitter.emit(cand, &cert, self.epoch_id, library) {
                        let delta_dl = cert.delta_dl;
                        trace.artifact_hashes.push(artifact.content_hash);
                        trace.events.push(Event::Accept {
                            artifact: artifact.clone(),
                            delta_dl,
                        });
                        self.registry.insert(artifact);
                        trace.accepted += 1;
                    } else {
                        trace.rejected += 1;
                    }
                }
                Verdict::Reject(reasons) => {
                    let candidate_hash = Artifact::canonical_hash(
                        &cand.rule,
                        self.epoch_id,
                        &AcceptanceCertificate::trivial_conjecture(0.0),
                    );
                    trace.events.push(Event::Reject {
                        candidate_hash,
                        reasons,
                    });
                    trace.rejected += 1;
                }
            }
        }

        self.epoch_id += 1;
        trace
    }
}

/// Canonical [`Emitter`] — wraps the candidate's rule with
/// `content_hash` via [`Artifact::seal`] and `parent_hashes` derived
/// from symbol references in `rule.rhs`. Domain-specific emitters
/// can compose on top by adding validation; this impl suffices for
/// any Generator that already produces complete `RewriteRule`s.
#[derive(Debug, Default, Clone, Copy)]
pub struct RuleEmitter;

impl Emitter for RuleEmitter {
    fn emit(
        &self,
        candidate: &Candidate,
        cert: &AcceptanceCertificate,
        epoch_id: u64,
        library: &[Artifact],
    ) -> Option<Artifact> {
        let rule = candidate.rule.clone();
        let parent_hashes = Artifact::parents_from_rhs(&rule, library);
        Some(Artifact::seal(rule, epoch_id, cert.clone(), parent_hashes))
    }
}

/// In-memory [`Registry`] — Vec-backed, good for tests and the current
/// in-memory REPL. Persistent backends live in `mathscape-store`.
#[derive(Debug, Default, Clone)]
pub struct InMemoryRegistry {
    entries: Vec<Artifact>,
    status_overlay: std::collections::HashMap<TermRef, ProofStatus>,
}

impl InMemoryRegistry {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Registry for InMemoryRegistry {
    fn insert(&mut self, artifact: Artifact) {
        self.entries.push(artifact);
    }
    fn all(&self) -> &[Artifact] {
        &self.entries
    }
    fn mark_status(&mut self, artifact_hash: TermRef, status: ProofStatus) {
        self.status_overlay.insert(artifact_hash, status);
    }
    fn status_of(&self, artifact_hash: TermRef) -> Option<ProofStatus> {
        // Overlay wins; fall back to embedded certificate status.
        if let Some(s) = self.status_overlay.get(&artifact_hash) {
            return Some(s.clone());
        }
        self.entries
            .iter()
            .find(|a| a.content_hash == artifact_hash)
            .map(|a| a.certificate.status.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{apply, nat, var};
    use crate::value::Value;

    /// Trivial generator that emits a fixed list of candidates for the
    /// first epoch, then nothing.
    struct FixedGen {
        first: Vec<Candidate>,
    }
    impl Generator for FixedGen {
        fn propose(
            &mut self,
            epoch_id: u64,
            _corpus: &[Term],
            _library: &[Artifact],
        ) -> Vec<Candidate> {
            if epoch_id == 0 {
                std::mem::take(&mut self.first)
            } else {
                vec![]
            }
        }
    }

    /// Always-accept prover with a fixed certificate.
    struct AlwaysAccept;
    impl Prover for AlwaysAccept {
        fn prove(&self, _c: &Candidate, _corpus: &[Term], _lib: &[Artifact]) -> Verdict {
            Verdict::Accept(sample_cert(1.0))
        }
    }

    fn sample_cert(score: f64) -> AcceptanceCertificate {
        AcceptanceCertificate {
            score,
            compression_ratio: 0.5,
            condensation_ratio: 0.0,
            coverage_delta: 0,
            novelty: 0.5,
            meta_compression: 0.0,
            delta_dl: score,
            status: ProofStatus::Conjectured,
            equivalence_hash: None,
        }
    }

    /// Always-reject prover.
    struct AlwaysReject;
    impl Prover for AlwaysReject {
        fn prove(&self, _c: &Candidate, _corpus: &[Term], _lib: &[Artifact]) -> Verdict {
            Verdict::Reject(vec![Rejection {
                reason: "test".into(),
                threshold: 1.0,
                actual: 0.0,
            }])
        }
    }

    fn dummy_rule(id: SymbolId, rhs: Term) -> RewriteRule {
        RewriteRule {
            name: format!("S_{id:03}"),
            lhs: Term::Symbol(id, vec![]),
            rhs,
        }
    }

    fn sample_cand(id: SymbolId, origin: &str) -> Candidate {
        Candidate {
            rule: dummy_rule(id, apply(var(2), vec![nat(1), nat(1)])),
            origin: origin.into(),
        }
    }

    #[test]
    fn empty_epoch_produces_empty_trace() {
        let mut epoch = Epoch::new(
            FixedGen { first: vec![] },
            AlwaysAccept,
            RuleEmitter,
            InMemoryRegistry::new(),
        );
        let trace = epoch.step(&[]);
        assert_eq!(trace.proposals, 0);
        assert_eq!(trace.accepted, 0);
        assert_eq!(trace.rejected, 0);
    }

    #[test]
    fn accepting_prover_lands_artifact() {
        let mut epoch = Epoch::new(
            FixedGen {
                first: vec![sample_cand(1, "test")],
            },
            AlwaysAccept,
            RuleEmitter,
            InMemoryRegistry::new(),
        );
        let trace = epoch.step(&[]);
        assert_eq!(trace.proposals, 1);
        assert_eq!(trace.accepted, 1);
        assert_eq!(trace.rejected, 0);
        assert_eq!(epoch.registry.len(), 1);
    }

    #[test]
    fn rejecting_prover_drops_candidate() {
        let mut epoch = Epoch::new(
            FixedGen {
                first: vec![sample_cand(1, "test")],
            },
            AlwaysReject,
            RuleEmitter,
            InMemoryRegistry::new(),
        );
        let trace = epoch.step(&[]);
        assert_eq!(trace.rejected, 1);
        assert!(epoch.registry.is_empty());
    }

    #[test]
    fn content_hash_is_deterministic() {
        let rule = RewriteRule {
            name: "S_001".into(),
            lhs: Term::Symbol(1, vec![]),
            rhs: apply(var(2), vec![nat(1), nat(1)]),
        };
        let cert = sample_cert(1.0);
        let h1 = Artifact::canonical_hash(&rule, 0, &cert);
        let h2 = Artifact::canonical_hash(&rule, 0, &cert);
        assert_eq!(h1, h2);
    }

    #[test]
    fn distinct_artifacts_distinct_hashes() {
        let cert = sample_cert(1.0);
        let r1 = RewriteRule {
            name: "S_001".into(),
            lhs: Term::Symbol(1, vec![]),
            rhs: Term::Number(Value::Nat(1)),
        };
        let r2 = RewriteRule {
            name: "S_002".into(),
            lhs: Term::Symbol(2, vec![]),
            rhs: Term::Number(Value::Nat(2)),
        };
        assert_ne!(
            Artifact::canonical_hash(&r1, 0, &cert),
            Artifact::canonical_hash(&r2, 0, &cert)
        );
    }

    #[test]
    fn parent_hashes_chain_through_symbols() {
        // First epoch mints S_001 with no parents.
        // Second epoch mints S_002 whose rhs references S_001.
        let mut registry = InMemoryRegistry::new();
        let emitter = RuleEmitter;

        let c1 = sample_cert(1.0);
        let first = emitter
            .emit(
                &Candidate {
                    rule: dummy_rule(1, Term::Number(Value::Nat(1))),
                    origin: "t".into(),
                },
                &c1,
                0,
                registry.all(),
            )
            .unwrap();
        assert!(first.parent_hashes.is_empty());
        let first_hash = first.content_hash;
        registry.insert(first);

        // Second: rhs references S_001 via Term::Symbol(1, ...)
        let second = emitter
            .emit(
                &Candidate {
                    rule: dummy_rule(2, Term::Symbol(1, vec![])),
                    origin: "t".into(),
                },
                &c1,
                1,
                registry.all(),
            )
            .unwrap();
        assert_eq!(second.parent_hashes, vec![first_hash]);
    }

    #[test]
    fn epoch_id_increments() {
        let mut epoch = Epoch::new(
            FixedGen { first: vec![] },
            AlwaysAccept,
            RuleEmitter,
            InMemoryRegistry::new(),
        );
        assert_eq!(epoch.epoch_id, 0);
        epoch.step(&[]);
        assert_eq!(epoch.epoch_id, 1);
        epoch.step(&[]);
        assert_eq!(epoch.epoch_id, 2);
    }

    #[test]
    fn in_memory_registry_append_only() {
        let mut reg = InMemoryRegistry::new();
        let cert = sample_cert(1.0);
        let rule = RewriteRule {
            name: "S_001".into(),
            lhs: Term::Symbol(1, vec![]),
            rhs: Term::Number(Value::Nat(1)),
        };
        let a = Artifact::seal(rule, 0, cert, vec![]);
        reg.insert(a.clone());
        reg.insert(a);
        assert_eq!(reg.len(), 2);
    }
}
