//! Phase V.certification (2026-04-18): the rule-status state
//! machine, validation → certification pipeline, feedback loop.
//!
//! # The state machine
//!
//! Every rule the motor produces lives at a `CertificationLevel`.
//! The levels are ordered — higher means stronger evidence:
//!
//! ```text
//!   Candidate      — just extracted by the law generator.
//!      │            No evidence beyond "it showed up in some AU."
//!      ▼
//!   Validated      — Phase J (K=8 Nat bindings) evaluated to
//!      │            LHS == RHS across all samples.
//!      ▼
//!   ProvisionalCore— the rule is in the CORE of the MathscapeMap
//!      │            across N ≥ 2 seeds. Cross-trajectory invariant.
//!      ▼
//!   Certified      — Phase J re-run at stricter strength (K=32,
//!      │            optionally cross-domain, optionally with
//!      ▼            random seed library contexts). Strong evidence.
//!   Canonical      — promoted: seeds all future runs, becomes
//!                    part of the permanent substrate.
//! ```
//!
//! # The pipeline
//!
//! Validation is FAST (8 samples, ~microseconds per rule). It
//! produces CANDIDATES for certification. Certification is
//! STRICTER (32+ samples, cross-context). It produces candidates
//! for canonicalization. The pipeline is:
//!
//! ```text
//!   extractor ──► validated-stream ──► certification-worker ──►
//!     canonical-ready-stream ──► promotion-worker ──► canonical-library
//! ```
//!
//! Each stage is SYNCHRONOUS in the current implementation —
//! under load the same shape becomes async workers consuming
//! from a shared event stream. The abstraction is designed so
//! that transition is a crate-local refactor, not a re-architecture.
//!
//! # Why levels matter
//!
//! The primary algorithm (motor / proposer) can condition its
//! behavior on levels. Example: when seeding a new run, prefer
//! Canonical over Certified over ProvisionalCore — the stronger
//! the evidence, the safer the seed. When sampling bindings
//! during Phase J, a Certified library rule can be assumed valid
//! (skip re-check); a ProvisionalCore one cannot.
//!
//! Feedback: as the certifier elevates rules, the system
//! accumulates a monotonically-certified substrate. Future
//! motor runs start from this substrate and reach further, still
//! generating candidates but now compounding on KNOWN
//! mathematics rather than statistical noise.

use crate::eval::RewriteRule;
use crate::hash::TermRef;
use serde::{Deserialize, Serialize};

/// The position of a rule in the certification state machine.
/// Higher = stronger evidence.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize,
    Deserialize,
)]
pub enum CertificationLevel {
    Candidate,
    Validated,
    ProvisionalCore,
    Certified,
    Canonical,
}

impl CertificationLevel {
    /// Stable name for logging / events.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            CertificationLevel::Candidate => "candidate",
            CertificationLevel::Validated => "validated",
            CertificationLevel::ProvisionalCore => "provisional-core",
            CertificationLevel::Certified => "certified",
            CertificationLevel::Canonical => "canonical",
        }
    }
}

/// A rule with its current certification state + attestation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CertifiedRule {
    pub rule: RewriteRule,
    pub level: CertificationLevel,
    /// How many evidence samples have passed (Phase J K for
    /// Validated/Certified).
    pub evidence_samples: usize,
    /// BLAKE3 over the rule content — stable identity regardless
    /// of level transitions.
    pub content_hash: TermRef,
}

impl CertifiedRule {
    #[must_use]
    pub fn new(rule: RewriteRule, level: CertificationLevel) -> Self {
        let content_hash = rule_content_hash(&rule);
        Self {
            rule,
            level,
            evidence_samples: 0,
            content_hash,
        }
    }

    /// Elevate to a higher level if the new level is strictly
    /// greater. No-op if it's equal or lower (rules never
    /// regress).
    pub fn elevate(
        &mut self,
        new_level: CertificationLevel,
        samples: usize,
    ) -> bool {
        if new_level > self.level {
            self.level = new_level;
            self.evidence_samples = self.evidence_samples.max(samples);
            true
        } else {
            false
        }
    }
}

fn rule_content_hash(rule: &RewriteRule) -> TermRef {
    let bytes =
        bincode::serialize(&(&rule.lhs, &rule.rhs)).expect("serializable");
    TermRef::from_bytes(&bytes)
}

/// Seam for the certification stage. Given a validated rule and
/// a context (e.g. the current canonical substrate), decide
/// whether to elevate to Certified.
pub trait Certifier {
    fn try_certify(
        &self,
        rule: &RewriteRule,
        context: &[RewriteRule],
    ) -> CertificationVerdict;
}

/// Certifier's output for one rule.
#[derive(Debug, Clone, PartialEq)]
pub enum CertificationVerdict {
    /// Rule passes stricter certification. The number of samples
    /// run is reported so the `evidence_samples` field on the
    /// certified rule reflects actual work done.
    Certified {
        samples_passed: usize,
    },
    /// Rule fails certification. Reason gives the first
    /// disagreement observed.
    Rejected {
        reason: String,
    },
    /// Not enough evidence to decide — the rule should stay at
    /// its current level. E.g., the certifier requires context
    /// we don't yet have.
    Inconclusive,
}

/// Default certifier: re-runs Phase J at stricter K, optionally
/// with random seeds to cover more of the binding space.
///
/// This version is a self-contained Nat-only certifier. Richer
/// certifiers (cross-domain, cross-context) live downstream —
/// the trait is the extension point.
#[derive(Debug, Clone)]
pub struct DefaultCertifier {
    /// Samples to run. Default 32 (4x the Validated standard).
    pub k_samples: usize,
    /// Number of distinct seeds to try. Different seeds produce
    /// different sample sets, so `k_samples × seed_count` is the
    /// total samples attempted.
    pub seed_count: usize,
    /// Max eval steps per side.
    pub step_limit: usize,
}

impl Default for DefaultCertifier {
    fn default() -> Self {
        Self {
            k_samples: 32,
            seed_count: 3,
            step_limit: 300,
        }
    }
}

/// Mirrors Phase J.2 domain detection. Inspects the rule's LHS
/// head op id to pick the binding pool; falls back to Nat when
/// no recognizable head op appears.
fn detect_domain(rule: &RewriteRule) -> CertifierDomain {
    fn detect_in(t: &crate::term::Term) -> Option<CertifierDomain> {
        use crate::term::Term;
        match t {
            Term::Apply(f, args) => {
                if let Term::Var(op) = &**f {
                    if let Some(d) = CertifierDomain::from_op_id(*op) {
                        return Some(d);
                    }
                }
                for a in args {
                    if let Some(d) = detect_in(a) {
                        return Some(d);
                    }
                }
                None
            }
            Term::Symbol(_, args) => {
                for a in args {
                    if let Some(d) = detect_in(a) {
                        return Some(d);
                    }
                }
                None
            }
            _ => None,
        }
    }
    detect_in(&rule.lhs).unwrap_or(CertifierDomain::Nat)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CertifierDomain {
    Nat,
    Int,
    Tensor,
    Float,
    FloatTensor,
}

impl CertifierDomain {
    fn from_op_id(id: u32) -> Option<Self> {
        match id {
            0..=9 => Some(Self::Nat),
            10..=19 => Some(Self::Int),
            20..=29 => Some(Self::Tensor),
            30..=39 => Some(Self::Float),
            40..=99 => Some(Self::FloatTensor),
            _ => None,
        }
    }

    fn sample(&self, x: u64) -> crate::term::Term {
        use crate::term::Term;
        use crate::value::Value;
        match self {
            Self::Nat => {
                let pool = [0u64, 1, 2, 5, 17, 3, 8];
                Term::Number(Value::Nat(pool[(x as usize) % pool.len()]))
            }
            Self::Int => {
                let pool = [0i64, 1, -1, 5, -3, 42, -7];
                Term::Number(Value::Int(pool[(x as usize) % pool.len()]))
            }
            Self::Float => {
                let pool = [0.0f64, 1.0, 2.5, -1.5, 0.5, 3.14];
                let f = pool[(x as usize) % pool.len()];
                Term::Number(
                    Value::from_f64(f)
                        .unwrap_or(Value::Float(f.to_bits())),
                )
            }
            Self::Tensor => {
                let pool: [(Vec<usize>, Vec<i64>); 4] = [
                    (vec![2], vec![0, 0]),
                    (vec![2], vec![1, 1]),
                    (vec![2], vec![2, 3]),
                    (vec![2], vec![5, 7]),
                ];
                let (shape, data) = pool[(x as usize) % pool.len()].clone();
                Term::Number(
                    Value::tensor(shape, data).unwrap_or(Value::Nat(0)),
                )
            }
            Self::FloatTensor => {
                let pool: [(Vec<usize>, Vec<f64>); 4] = [
                    (vec![2], vec![0.0, 0.0]),
                    (vec![2], vec![1.0, 1.0]),
                    (vec![2], vec![0.5, 1.5]),
                    (vec![2], vec![2.0, 3.0]),
                ];
                let (shape, data) = pool[(x as usize) % pool.len()].clone();
                Term::Number(
                    Value::float_tensor(shape, data)
                        .unwrap_or(Value::Float(0.0f64.to_bits())),
                )
            }
        }
    }
}

impl Certifier for DefaultCertifier {
    fn try_certify(
        &self,
        rule: &RewriteRule,
        context: &[RewriteRule],
    ) -> CertificationVerdict {
        // Domain-aware sampling (mirrors Phase J.2). Detect the
        // rule's algebraic domain from its LHS head op; sample
        // from the matching pool.
        let domain = detect_domain(rule);
        let mut total_samples = 0;
        for seed in 0..self.seed_count as u64 {
            for sample_index in 0..self.k_samples {
                total_samples += 1;
                let vars = collect_pattern_vars(&rule.lhs);
                let mut lhs = rule.lhs.clone();
                let mut rhs = rule.rhs.clone();
                for (var_pos, &var_id) in vars.iter().enumerate() {
                    let x = seed
                        .wrapping_add(sample_index as u64)
                        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                        .wrapping_add(var_pos as u64);
                    let val = domain.sample(x);
                    lhs = lhs.substitute(var_id, &val);
                    rhs = rhs.substitute(var_id, &val);
                }
                let lhs_nf = crate::eval::eval(&lhs, context, self.step_limit);
                let rhs_nf = crate::eval::eval(&rhs, context, self.step_limit);
                match (lhs_nf, rhs_nf) {
                    (Ok(l), Ok(r)) if l == r => continue,
                    (Ok(_), Ok(_)) => {
                        return CertificationVerdict::Rejected {
                            reason: format!(
                                "disagreement at sample {} seed {} (domain {:?})",
                                sample_index, seed, domain
                            ),
                        };
                    }
                    _ => {
                        return CertificationVerdict::Rejected {
                            reason: format!(
                                "eval error at sample {} seed {} (domain {:?})",
                                sample_index, seed, domain
                            ),
                        };
                    }
                }
            }
        }
        CertificationVerdict::Certified {
            samples_passed: total_samples,
        }
    }
}

fn collect_pattern_vars(t: &crate::term::Term) -> Vec<u32> {
    fn inner(t: &crate::term::Term, out: &mut Vec<u32>) {
        use crate::term::Term;
        match t {
            Term::Var(v) if *v >= 100 => out.push(*v),
            Term::Var(_) | Term::Point(_) | Term::Number(_) => {}
            Term::Apply(head, args) => {
                inner(head, out);
                for a in args {
                    inner(a, out);
                }
            }
            Term::Fn(_, body) => inner(body, out),
            Term::Symbol(_, args) => {
                for a in args {
                    inner(a, out);
                }
            }
        }
    }
    let mut out = Vec::new();
    inner(t, &mut out);
    out.sort_unstable();
    out.dedup();
    out
}

/// A certification step's output — what happened to each rule.
#[derive(Debug, Clone, PartialEq)]
pub struct CertificationStepReport {
    pub elevated: Vec<CertifiedRule>,
    pub rejected: Vec<(RewriteRule, String)>,
    pub skipped: usize,
}

/// Run the certifier over a batch of provisional-core rules.
/// Rules already at or above `Certified` are passed through
/// unchanged. Rules at `ProvisionalCore` get the certifier's
/// verdict applied; Certified means elevation, Rejected means
/// they drop (not included in elevated; caller can decide what
/// to do with them).
pub fn run_certification_step<C: Certifier>(
    certifier: &C,
    input: Vec<CertifiedRule>,
    context: &[RewriteRule],
) -> CertificationStepReport {
    let mut elevated = Vec::new();
    let mut rejected = Vec::new();
    let mut skipped = 0;
    for mut cr in input {
        match cr.level {
            CertificationLevel::Certified
            | CertificationLevel::Canonical => {
                skipped += 1;
                elevated.push(cr);
            }
            _ => match certifier.try_certify(&cr.rule, context) {
                CertificationVerdict::Certified { samples_passed } => {
                    cr.elevate(CertificationLevel::Certified, samples_passed);
                    elevated.push(cr);
                }
                CertificationVerdict::Rejected { reason } => {
                    rejected.push((cr.rule, reason));
                }
                CertificationVerdict::Inconclusive => {
                    skipped += 1;
                    elevated.push(cr);
                }
            },
        }
    }
    CertificationStepReport {
        elevated,
        rejected,
        skipped,
    }
}

/// Reactive certification consumer. Subscribes to `CoreGrew`
/// events in the map stream; when a new rule becomes invariant
/// across seeds, try to certify it at the stricter level and
/// emit a downstream event.
///
/// Chainable: each consumer has a `downstream` that receives
/// every event, plus the new `RuleCertified` /
/// `RuleRejectedAtCertification` events the certifier emits.
/// Pipelines are composed by nesting consumers.
///
/// This is the event-driven version of the state machine — the
/// certification step runs REACTIVELY as the map mutates, not
/// as a separate batch phase. Stream-shape semantics even though
/// implementation is synchronous.
#[derive(Debug)]
pub struct CertifyingConsumer<D, C>
where
    D: Certifier,
    C: crate::mathscape_map::MapEventConsumer,
{
    pub certifier: D,
    pub downstream: C,
    /// Rules that have reached Certified. Caller can read this
    /// at any point to get the current certified library.
    pub certified: std::cell::RefCell<Vec<CertifiedRule>>,
    /// Per-run context passed to the certifier (e.g. a snapshot
    /// of the stable library). Empty is fine; domain detection
    /// handles the rest.
    pub context: Vec<RewriteRule>,
}

impl<D, C> CertifyingConsumer<D, C>
where
    D: Certifier,
    C: crate::mathscape_map::MapEventConsumer,
{
    #[must_use]
    pub fn new(certifier: D, downstream: C) -> Self {
        Self {
            certifier,
            downstream,
            certified: std::cell::RefCell::new(Vec::new()),
            context: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_context(mut self, context: Vec<RewriteRule>) -> Self {
        self.context = context;
        self
    }

    /// Current count of certified rules.
    pub fn certified_count(&self) -> usize {
        self.certified.borrow().len()
    }

    /// Snapshot the certified rules (clone).
    pub fn certified_rules(&self) -> Vec<RewriteRule> {
        self.certified
            .borrow()
            .iter()
            .map(|cr| cr.rule.clone())
            .collect()
    }
}

impl<D, C> crate::mathscape_map::MapEventConsumer for CertifyingConsumer<D, C>
where
    D: Certifier,
    C: crate::mathscape_map::MapEventConsumer,
{
    fn on_event(&self, event: &crate::mathscape_map::MapEvent) {
        // Forward every event downstream first — consumers below
        // see the original map events.
        self.downstream.on_event(event);

        // React to CoreGrew: try to certify the newly-invariant rule.
        if let crate::mathscape_map::MapEvent::CoreGrew {
            added_rule, ..
        } = event
        {
            // Skip if we've already certified this rule (content-
            // hash dedup).
            let already_certified = {
                let hash = rule_content_hash(added_rule);
                self.certified
                    .borrow()
                    .iter()
                    .any(|cr| cr.content_hash == hash)
            };
            if already_certified {
                return;
            }
            let verdict = self.certifier.try_certify(added_rule, &self.context);
            match verdict {
                CertificationVerdict::Certified { samples_passed } => {
                    let mut cr = CertifiedRule::new(
                        added_rule.clone(),
                        CertificationLevel::Certified,
                    );
                    cr.evidence_samples = samples_passed;
                    self.certified.borrow_mut().push(cr);
                    self.downstream.on_event(
                        &crate::mathscape_map::MapEvent::RuleCertified {
                            rule: added_rule.clone(),
                            evidence_samples: samples_passed,
                        },
                    );
                }
                CertificationVerdict::Rejected { reason } => {
                    self.downstream.on_event(
                        &crate::mathscape_map::MapEvent::RuleRejectedAtCertification {
                            rule: added_rule.clone(),
                            reason,
                        },
                    );
                }
                CertificationVerdict::Inconclusive => {
                    // No event emitted; rule stays at its current
                    // level. The next CoreGrew observation for the
                    // same rule won't retry (already_certified
                    // check above is content-hash dedup; we could
                    // track inconclusive separately if needed).
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::term::Term;
    use crate::value::Value;

    fn var(id: u32) -> Term {
        Term::Var(id)
    }
    fn apply(h: Term, args: Vec<Term>) -> Term {
        Term::Apply(Box::new(h), args)
    }
    fn nat(n: u64) -> Term {
        Term::Number(Value::Nat(n))
    }

    fn add_id() -> RewriteRule {
        RewriteRule {
            name: "add-id".into(),
            lhs: apply(var(2), vec![nat(0), var(100)]),
            rhs: var(100),
        }
    }
    fn bogus() -> RewriteRule {
        // bogus: add(?x, ?y) => ?y — semantically wrong
        RewriteRule {
            name: "bogus".into(),
            lhs: apply(var(2), vec![var(100), var(101)]),
            rhs: var(101),
        }
    }

    #[test]
    fn levels_order_correctly() {
        assert!(CertificationLevel::Candidate < CertificationLevel::Validated);
        assert!(
            CertificationLevel::Validated < CertificationLevel::ProvisionalCore
        );
        assert!(
            CertificationLevel::ProvisionalCore < CertificationLevel::Certified
        );
        assert!(
            CertificationLevel::Certified < CertificationLevel::Canonical
        );
    }

    #[test]
    fn elevate_is_monotone() {
        let mut cr = CertifiedRule::new(
            add_id(),
            CertificationLevel::Validated,
        );
        assert!(cr.elevate(CertificationLevel::Certified, 32));
        // Can't regress.
        assert!(!cr.elevate(CertificationLevel::Validated, 8));
        assert_eq!(cr.level, CertificationLevel::Certified);
        assert_eq!(cr.evidence_samples, 32);
    }

    #[test]
    fn default_certifier_certifies_add_identity() {
        let cert = DefaultCertifier::default();
        let verdict = cert.try_certify(&add_id(), &[]);
        assert!(
            matches!(verdict, CertificationVerdict::Certified { .. }),
            "add-identity must certify, got {:?}",
            verdict
        );
    }

    #[test]
    fn default_certifier_rejects_semantically_wrong_rule() {
        let cert = DefaultCertifier::default();
        let verdict = cert.try_certify(&bogus(), &[]);
        assert!(
            matches!(verdict, CertificationVerdict::Rejected { .. }),
            "bogus rule must be rejected, got {:?}",
            verdict
        );
    }

    #[test]
    fn certification_step_elevates_passes_and_drops_failures() {
        let cert = DefaultCertifier::default();
        let input = vec![
            CertifiedRule::new(add_id(), CertificationLevel::ProvisionalCore),
            CertifiedRule::new(bogus(), CertificationLevel::ProvisionalCore),
        ];
        let report = run_certification_step(&cert, input, &[]);
        assert_eq!(report.elevated.len(), 1);
        assert_eq!(report.rejected.len(), 1);
        assert_eq!(report.elevated[0].level, CertificationLevel::Certified);
    }

    #[test]
    fn certified_rule_bincode_roundtrip() {
        let cr = CertifiedRule::new(add_id(), CertificationLevel::Certified);
        let bytes = bincode::serialize(&cr).unwrap();
        let back: CertifiedRule = bincode::deserialize(&bytes).unwrap();
        assert_eq!(cr, back);
    }

    // ── Reactive CertifyingConsumer tests ────────────────────────

    #[test]
    fn certifying_consumer_elevates_on_core_grew() {
        use crate::mathscape_map::{
            BufferedConsumer, MapEvent, MapEventConsumer, MathscapeMap,
            MapSnapshot,
        };
        // Stack: CertifyingConsumer → BufferedConsumer.
        // Certifier = default (K=32 × 3 = 96 samples).
        let buffered = BufferedConsumer::new();
        let consumer = CertifyingConsumer::new(
            DefaultCertifier::default(),
            buffered,
        );
        let mut map = MathscapeMap::new();
        // Seed 1 has both rules as final. Seed 2 has both too →
        // both become core → CoreGrew fires for each → reactive
        // certification tries each.
        map.push_with_events(
            MapSnapshot::new(1, 0, vec![add_id()], None),
            &consumer,
            0.6,
        );
        map.push_with_events(
            MapSnapshot::new(2, 0, vec![add_id()], None),
            &consumer,
            0.6,
        );
        // After these pushes, add_id should be in core → CoreGrew
        // emitted → CertifyingConsumer tried to certify → add_id
        // passes (it's a valid identity law) → RuleCertified fired.
        let downstream_events = consumer.downstream.drain();
        let certified_events: Vec<_> = downstream_events
            .iter()
            .filter(|e| matches!(e, MapEvent::RuleCertified { .. }))
            .collect();
        assert_eq!(
            certified_events.len(),
            1,
            "add-identity should reactively certify after CoreGrew"
        );
        assert_eq!(consumer.certified_count(), 1);
    }

    #[test]
    fn certifying_consumer_emits_rejection_for_bogus_rule() {
        use crate::mathscape_map::{
            BufferedConsumer, MapEvent, MapEventConsumer, MathscapeMap,
            MapSnapshot,
        };
        let buffered = BufferedConsumer::new();
        let consumer = CertifyingConsumer::new(
            DefaultCertifier::default(),
            buffered,
        );
        let mut map = MathscapeMap::new();
        // Push bogus rule into 2 seeds' finals → becomes core →
        // certification rejects.
        map.push_with_events(
            MapSnapshot::new(1, 0, vec![bogus()], None),
            &consumer,
            0.6,
        );
        map.push_with_events(
            MapSnapshot::new(2, 0, vec![bogus()], None),
            &consumer,
            0.6,
        );
        let events = consumer.downstream.drain();
        let rejected_events: Vec<_> = events
            .iter()
            .filter(|e| {
                matches!(e, MapEvent::RuleRejectedAtCertification { .. })
            })
            .collect();
        assert_eq!(
            rejected_events.len(),
            1,
            "bogus rule must reactively emit rejection"
        );
        assert_eq!(consumer.certified_count(), 0);
    }

    #[test]
    fn certifying_consumer_does_not_re_certify_same_rule() {
        use crate::mathscape_map::{
            BufferedConsumer, MathscapeMap, MapSnapshot,
        };
        let buffered = BufferedConsumer::new();
        let consumer = CertifyingConsumer::new(
            DefaultCertifier::default(),
            buffered,
        );
        let mut map = MathscapeMap::new();
        // Certify once.
        map.push_with_events(
            MapSnapshot::new(1, 0, vec![add_id()], None),
            &consumer,
            0.6,
        );
        map.push_with_events(
            MapSnapshot::new(2, 0, vec![add_id()], None),
            &consumer,
            0.6,
        );
        let first_count = consumer.certified_count();
        // Push a third snapshot. The rule is already certified;
        // we should not re-certify it.
        map.push_with_events(
            MapSnapshot::new(3, 0, vec![add_id()], None),
            &consumer,
            0.6,
        );
        assert_eq!(consumer.certified_count(), first_count);
    }

    // ── MathscapeMap persistence tests ────────────────────────────

    #[test]
    fn map_save_and_load_roundtrip() {
        use crate::mathscape_map::{MapSnapshot, MathscapeMap};
        let mut m = MathscapeMap::new();
        m.push(MapSnapshot::new(1, 0, vec![add_id()], None));
        m.push(MapSnapshot::new(2, 0, vec![bogus()], None));
        // Write to a temp file, read back.
        let dir = std::env::temp_dir();
        let path = dir.join("phase_v_pipeline_roundtrip.bin");
        m.save_to_path(&path).expect("save succeeds");
        let loaded = MathscapeMap::load_from_path(&path).expect("load succeeds");
        assert_eq!(m, loaded);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn already_certified_rules_skipped() {
        let cert = DefaultCertifier::default();
        let input = vec![
            CertifiedRule::new(add_id(), CertificationLevel::Certified),
        ];
        let report = run_certification_step(&cert, input, &[]);
        assert_eq!(report.elevated.len(), 1);
        assert_eq!(report.skipped, 1);
    }
}
