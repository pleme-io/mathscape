//! Phase L5 — the edge-riding discovery session.
//!
//! State + invariants + lifecycle for the perpetual discovery
//! loop. The correctness criterion — "any halt is a bug" — is
//! enforced by the `DiscoverySession` through novelty-rate tracking
//! and stall detection.
//!
//! # Architecture overview
//!
//! The L5 loop runs forever. Each cycle:
//!
//!   1. An outer orchestrator runs a SUB-CAMPAIGN over apparatus
//!      × corpus × scale space, collecting candidate rules the
//!      machine wants to believe.
//!   2. Each candidate is tested by phase J's semantic validator
//!      against the primitive Peano evaluator. Passing candidates
//!      are THEOREMS.
//!   3. Theorems that strictly REDUCE (RHS size < LHS size) enter
//!      the SUBSTRATE — they will pre-reduce the corpus for
//!      subsequent cycles, eating one layer of structure.
//!   4. ALL validated theorems enter the LEDGER — they contribute
//!      their RHS shapes as candidate templates for future cycles.
//!      Equivalences (like commutativity) go here rather than
//!      the substrate, because applying them during reduction
//!      would oscillate.
//!   5. The adaptive-corpus generator uses the new substrate to
//!      produce next-cycle corpora whose residue surfaces next-
//!      layer patterns.
//!   6. A new apparatus pool is evolved from cycle winners.
//!   7. Repeat.
//!
//! The `DiscoverySession` owns steps 3-4's state and the
//! correctness invariant. The orchestration (1, 2, 5, 6, 7) lives
//! in the caller — typically an integration test or a long-running
//! daemon.
//!
//! # Halt-is-a-bug correctness
//!
//! By Gödel's incompleteness, any sufficiently-rich formal system
//! has true statements unprovable from inside. Our substrate is
//! always a finite axiom set; there are always unprovable-from-
//! it statements. Therefore: if the machine stops finding new
//! theorems, it has stopped looking correctly, not finished.
//! Every zero-novelty cycle is a specific mechanism gap.
//!
//! `DiscoverySession::has_stalled` reports whether any post-
//! bootstrap cycle produced zero new theorems. The test harness
//! treats this as a failure — the loop is not "done," it's
//! diagnosed.

use mathscape_core::eval::RewriteRule;
use mathscape_core::term::Term;
use std::collections::HashSet;

// ── Utility: term size ───────────────────────────────────────────

/// Count nodes in a term. Used by `is_reducing` to determine whether
/// a rule strictly reduces (eligible for substrate) or only
/// reshapes / equates (ledger only).
#[must_use]
pub fn term_size(t: &Term) -> usize {
    match t {
        Term::Point(_) | Term::Number(_) | Term::Var(_) => 1,
        Term::Apply(f, args) => {
            1 + term_size(f) + args.iter().map(term_size).sum::<usize>()
        }
        Term::Symbol(_, args) => 1 + args.iter().map(term_size).sum::<usize>(),
        Term::Fn(params, body) => 1 + params.len() + term_size(body),
    }
}

// ── Theorem identity ─────────────────────────────────────────────

fn format_term(t: &Term) -> String {
    use mathscape_core::value::Value;
    match t {
        Term::Var(v) => format!("?v{v}"),
        Term::Number(Value::Nat(n)) => n.to_string(),
        // R7: Int prints with an 'i' suffix to distinguish in
        // theorem keys — e.g. 3i for Int(3), 3 for Nat(3). Keeps
        // cross-domain theorems structurally distinct in the
        // theorem_key.
        Term::Number(Value::Int(n)) => format!("{n}i"),
        // R13: Tensor rendered with shape tag for theorem keying.
        Term::Number(Value::Tensor { shape, data }) => {
            let s = shape
                .iter()
                .map(|x| x.to_string())
                .collect::<Vec<_>>()
                .join("x");
            let d = data
                .iter()
                .map(|x| x.to_string())
                .collect::<Vec<_>>()
                .join(",");
            format!("T{s}[{d}]")
        }
        Term::Apply(f, args) => {
            let fs = format_term(f);
            let ass: Vec<String> = args.iter().map(format_term).collect();
            format!("({} {})", fs, ass.join(" "))
        }
        Term::Symbol(id, args) => {
            let ass: Vec<String> = args.iter().map(format_term).collect();
            if ass.is_empty() {
                format!("S_{id}")
            } else {
                format!("(S_{id} {})", ass.join(" "))
            }
        }
        Term::Point(p) => format!("P_{p:?}"),
        Term::Fn(params, body) => format!("(fn {:?} → {})", params, format_term(body)),
    }
}

/// Key a theorem by its literal `LHS=RHS` form. Anonymization is
/// NOT applied: anonymizing LHS and RHS independently would collapse
/// commutativity (`add(a,b)→add(b,a)`) to identity (`add(a,b)→
/// add(a,b)`) because each side renames variables in isolation.
/// Using the literal form preserves the cross-side variable identity.
#[must_use]
pub fn theorem_key(rule: &RewriteRule) -> String {
    format!("{}={}", format_term(&rule.lhs), format_term(&rule.rhs))
}

/// A rule is "substrate-eligible" iff its RHS tree is strictly
/// smaller than its LHS tree. Equivalences (RHS size == LHS size,
/// like commutativity or associativity-rewrites) do NOT qualify —
/// they would oscillate in the rewriter (`add(a,b) → add(b,a) →
/// add(a,b) → …`). Equivalences belong in the ledger (for
/// candidate generation) and in a future e-graph (phase K) but
/// never in the term-rewriting substrate.
#[must_use]
pub fn is_reducing(rule: &RewriteRule) -> bool {
    term_size(&rule.rhs) < term_size(&rule.lhs)
}

// ── Substrate ────────────────────────────────────────────────────

/// The set of REDUCING theorems. Applied during adaptive-corpus
/// generation and pre-probe fixed-point reduction. Strictly
/// contracting under `rewrite_fixed_point` — never oscillates.
#[derive(Clone, Debug, Default)]
pub struct Substrate {
    rules: Vec<RewriteRule>,
}

impl Substrate {
    #[must_use]
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Add a rule. Caller must have verified `is_reducing(&rule)` —
    /// this is a cheap append, not a validation pass. Rules that
    /// aren't reducing will cause the substrate's use of
    /// `rewrite_fixed_point` to oscillate until step-limit.
    pub fn push(&mut self, rule: RewriteRule) {
        self.rules.push(rule);
    }

    #[must_use]
    pub fn rules(&self) -> &[RewriteRule] {
        &self.rules
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }
}

// ── Ledger ───────────────────────────────────────────────────────

/// The append-only record of EVERY validated theorem — reducing
/// and equivalence rules both. Used:
///   - as the source of compositional candidates (every RHS shape
///     is a template for future rules)
///   - for dedup (via `contains`)
///   - for audit / reporting of what the machine has discovered
#[derive(Clone, Debug, Default)]
pub struct Ledger {
    rules: Vec<RewriteRule>,
    keys: HashSet<String>,
}

impl Ledger {
    #[must_use]
    pub fn new() -> Self {
        Self {
            rules: Vec::new(),
            keys: HashSet::new(),
        }
    }

    /// Insert a theorem. Returns true iff newly added; false if it
    /// was already present (by `theorem_key` identity).
    pub fn insert(&mut self, rule: RewriteRule) -> bool {
        let k = theorem_key(&rule);
        if self.keys.contains(&k) {
            return false;
        }
        self.keys.insert(k);
        self.rules.push(rule);
        true
    }

    #[must_use]
    pub fn contains(&self, rule: &RewriteRule) -> bool {
        self.keys.contains(&theorem_key(rule))
    }

    /// Has `key` (pre-computed) been seen? Equivalent to
    /// `contains(rule)` when `key = theorem_key(rule)`.
    #[must_use]
    pub fn contains_key(&self, key: &str) -> bool {
        self.keys.contains(key)
    }

    #[must_use]
    pub fn rules(&self) -> &[RewriteRule] {
        &self.rules
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }
}

// ── Discovery session ────────────────────────────────────────────

/// One cycle's outcome. The full trajectory is the sequence of
/// these; novelty-rate analysis operates on the sequence.
#[derive(Clone, Debug)]
pub struct CycleRecord {
    pub cycle: usize,
    /// Structural rules surfaced by the sub-campaign's anti-unifier,
    /// before semantic validation.
    pub structural_rules: usize,
    /// New theorems added to the ledger this cycle (after dedup).
    pub new_theorems: usize,
    /// Substrate size AFTER this cycle.
    pub substrate_size: usize,
    /// Ledger size AFTER this cycle.
    pub ledger_size: usize,
    /// Wallclock time for the cycle, seconds.
    pub elapsed_secs: f64,
}

/// The state of a long-running discovery loop. Owns substrate +
/// ledger + trajectory. Enforces the halt-is-a-bug correctness
/// criterion via `has_stalled` / `stalled_cycles`.
#[derive(Clone, Debug, Default)]
pub struct DiscoverySession {
    pub substrate: Substrate,
    pub ledger: Ledger,
    pub trajectory: Vec<CycleRecord>,
}

impl DiscoverySession {
    #[must_use]
    pub fn new() -> Self {
        Self {
            substrate: Substrate::new(),
            ledger: Ledger::new(),
            trajectory: Vec::new(),
        }
    }

    /// Promote a newly-validated theorem. Always updates the ledger
    /// (if not already present); updates the substrate iff reducing.
    /// Returns `(added_to_ledger, added_to_substrate)`.
    pub fn promote(&mut self, rule: RewriteRule) -> (bool, bool) {
        let added_ledger = self.ledger.insert(rule.clone());
        let added_substrate = if added_ledger && is_reducing(&rule) {
            self.substrate.push(rule);
            true
        } else {
            false
        };
        (added_ledger, added_substrate)
    }

    /// Record a cycle's stats. Caller is responsible for counting
    /// structural rules + new theorems; this simply appends.
    pub fn record_cycle(
        &mut self,
        cycle: usize,
        structural_rules: usize,
        new_theorems: usize,
        elapsed_secs: f64,
    ) {
        self.trajectory.push(CycleRecord {
            cycle,
            structural_rules,
            new_theorems,
            substrate_size: self.substrate.len(),
            ledger_size: self.ledger.len(),
            elapsed_secs,
        });
    }

    /// Post-bootstrap cycles where `new_theorems == 0`. If any
    /// exist, the correctness criterion has been violated and the
    /// loop is broken.
    ///
    /// We skip the first two cycles — bootstrap cycles where the
    /// substrate is still forming are allowed to produce reduced
    /// novelty without signaling a bug. Only persistent zero-novelty
    /// after cycle 2 counts.
    #[must_use]
    pub fn stalled_cycles(&self) -> Vec<usize> {
        self.trajectory
            .iter()
            .skip(2)
            .filter(|r| r.new_theorems == 0)
            .map(|r| r.cycle)
            .collect()
    }

    /// Convenience: did the session exhibit a post-bootstrap stall?
    #[must_use]
    pub fn has_stalled(&self) -> bool {
        !self.stalled_cycles().is_empty()
    }

    /// Mean theorems per cycle across the full trajectory.
    #[must_use]
    pub fn mean_theorems_per_cycle(&self) -> f64 {
        if self.trajectory.is_empty() {
            return 0.0;
        }
        let total: usize = self.trajectory.iter().map(|r| r.new_theorems).sum();
        total as f64 / self.trajectory.len() as f64
    }

    #[must_use]
    pub fn total_new_theorems(&self) -> usize {
        self.trajectory.iter().map(|r| r.new_theorems).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mathscape_core::term::Term;
    use mathscape_core::value::Value;

    fn nat(n: u64) -> Term {
        Term::Number(Value::Nat(n))
    }
    fn var(id: u32) -> Term {
        Term::Var(id)
    }
    fn apply(f: Term, args: Vec<Term>) -> Term {
        Term::Apply(Box::new(f), args)
    }

    fn reducing_rule() -> RewriteRule {
        RewriteRule {
            name: "add-left-zero".into(),
            lhs: apply(var(2), vec![nat(0), var(100)]),
            rhs: var(100),
        }
    }

    fn commutativity_rule() -> RewriteRule {
        RewriteRule {
            name: "add-commute".into(),
            lhs: apply(var(2), vec![var(100), var(101)]),
            rhs: apply(var(2), vec![var(101), var(100)]),
        }
    }

    #[test]
    fn is_reducing_flags_projections() {
        assert!(is_reducing(&reducing_rule()));
    }

    #[test]
    fn is_reducing_rejects_equivalences() {
        assert!(!is_reducing(&commutativity_rule()));
    }

    #[test]
    fn theorem_key_distinguishes_commute_from_identity() {
        let commute = commutativity_rule();
        let identity = RewriteRule {
            name: "identity-shape".into(),
            lhs: apply(var(2), vec![var(100), var(101)]),
            rhs: apply(var(2), vec![var(100), var(101)]),
        };
        assert_ne!(
            theorem_key(&commute),
            theorem_key(&identity),
            "literal form must distinguish commute (swaps args) from identity (same args)"
        );
    }

    #[test]
    fn ledger_dedups_by_theorem_key() {
        let mut l = Ledger::new();
        assert!(l.insert(reducing_rule()));
        assert!(!l.insert(reducing_rule()), "second insert of same theorem must be rejected");
        assert_eq!(l.len(), 1);
    }

    #[test]
    fn ledger_stores_all_validated() {
        let mut l = Ledger::new();
        l.insert(reducing_rule());
        l.insert(commutativity_rule());
        assert_eq!(l.len(), 2);
        assert!(l.contains(&reducing_rule()));
        assert!(l.contains(&commutativity_rule()));
    }

    #[test]
    fn substrate_takes_only_reducing_rules() {
        let mut sess = DiscoverySession::new();
        let (added_ledger, added_substrate) = sess.promote(reducing_rule());
        assert!(added_ledger && added_substrate);
        let (added_ledger, added_substrate) = sess.promote(commutativity_rule());
        assert!(added_ledger && !added_substrate, "equivalences land in ledger only");
        assert_eq!(sess.substrate.len(), 1);
        assert_eq!(sess.ledger.len(), 2);
    }

    #[test]
    fn promote_returns_false_false_on_duplicate() {
        let mut sess = DiscoverySession::new();
        sess.promote(reducing_rule());
        let (ledger, substrate) = sess.promote(reducing_rule());
        assert!(!ledger && !substrate, "duplicate promote must report no adds");
    }

    #[test]
    fn stalled_cycles_skips_bootstrap() {
        let mut sess = DiscoverySession::new();
        sess.record_cycle(0, 50, 3, 1.0);
        sess.record_cycle(1, 40, 0, 1.0); // bootstrap zero — allowed
        sess.record_cycle(2, 30, 5, 1.0);
        sess.record_cycle(3, 20, 0, 1.0); // NOT bootstrap — stalled
        assert_eq!(sess.stalled_cycles(), vec![3]);
        assert!(sess.has_stalled());
    }

    #[test]
    fn mean_theorems_per_cycle() {
        let mut sess = DiscoverySession::new();
        sess.record_cycle(0, 0, 10, 0.0);
        sess.record_cycle(1, 0, 20, 0.0);
        assert!((sess.mean_theorems_per_cycle() - 15.0).abs() < 1e-9);
    }
}
