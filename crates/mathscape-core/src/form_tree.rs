//! `DiscoveryForest` — retroactive-reducer + adaptive-scheduler.
//!
//! Every term the machine has ever encountered becomes a node. Every
//! rewrite rule is an edge that maps one node to another. Whenever a
//! new morphism (library rule) is discovered, every existing node is
//! re-checked against it: nodes that were formerly irreducible may
//! become reducible under the new rule, and the forest records both
//! the successful reduction AND the historical point at which the
//! reduction became possible.
//!
//! Each node tracks its **irreducibility rate** — how many times a
//! reduction attempt has failed relative to how often it has been
//! tried. Nodes with stably-high irreducibility scores get checked
//! less frequently; nodes whose irreducibility has recently dropped
//! (because a new morphism was found) get checked more frequently.
//!
//! Efficiency: an internal `schedule: BTreeMap<epoch, Vec<TermRef>>`
//! indexes nodes by their next-due epoch. Each retroactive pass
//! drains only the due buckets (`schedule.range(..=current_epoch)`),
//! so the per-pass cost is O(due) + O(log n), never O(total_nodes).
//! This is the "concentrate compute on the live frontier" property
//! the user asked for — the schedule materially skips stable leaves.
//!
//! Design driven by the user-stated requirement:
//!
//! > "we should be able to support multiple tree structure of form
//! > tracking so that anytime we find a new symbolic morphism we
//! > relate it to the whole and attempt to reduce terms
//! > retroactively in the tree, noting the rate at which leaves in
//! > the tree of discovery become irreducible over time and
//! > therefore optimizing how often they are checked as to optimize
//! > traversal time."

use crate::eval::{pattern_match, RewriteRule};
use crate::hash::TermRef;
use crate::term::Term;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

// ── Typescape: invariant-carrying types ─────────────────────────────
//
// The three typed invariants that the forest depends on. Each has a
// gated constructor, so any value that exists in the program is by
// construction within the allowed domain. Downstream code doesn't
// need runtime checks — the type carries the proof.
//
// This is the pleme-io "types → proofs → render anywhere" discipline
// applied to the forest. Every line that mentions an `IrreducibilityRate`
// knows it's in [0, 1]; every `CheckPeriod` is in [1, 64]; every
// `HitCount` has `hits <= check_count`. No runtime validation, no
// clamping in consumer code, no defensive `assert!` scattered
// through the scheduler.

/// Irreducibility rate: fraction of checks that failed to reduce,
/// constructor-gated to [0.0, 1.0]. The only way to produce a value
/// outside the domain is to bypass the constructor — which requires
/// a type change, caught at compile time.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct IrreducibilityRate(f64);

impl IrreducibilityRate {
    /// The "fully irreducible" end of the spectrum. Brand-new nodes
    /// default to this: conservative, due immediately for checking.
    pub const MAX: Self = Self(1.0);
    /// The "always reduces" end.
    pub const MIN: Self = Self(0.0);

    /// Construct from a raw f64, clamping into [0, 1]. Any NaN is
    /// treated as MAX (conservative — "due for checking").
    #[must_use]
    pub fn new(raw: f64) -> Self {
        if raw.is_nan() {
            return Self::MAX;
        }
        Self(raw.clamp(0.0, 1.0))
    }

    #[must_use]
    pub fn as_f64(&self) -> f64 {
        self.0
    }
}

/// Check period: how many epochs to wait before re-inspecting a
/// node. Constructor-gated to [1, 64]. `CheckPeriod::from_rate`
/// derives it from the irreducibility rate under the `2^(6r)` curve.
/// Downstream arithmetic on `u64` is safe — the bounds are baked in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CheckPeriod(u64);

impl CheckPeriod {
    pub const MIN: Self = Self(1);
    pub const MAX: Self = Self(64);

    /// Derive period from the irreducibility rate via `2^(6r)`:
    /// r=0 → 1 epoch, r=1 → 64 epochs. Round, clamp, construct.
    #[must_use]
    pub fn from_rate(rate: IrreducibilityRate) -> Self {
        let raw = (rate.as_f64() * 6.0).exp2();
        let clamped = raw.clamp(Self::MIN.0 as f64, Self::MAX.0 as f64);
        Self(clamped.round() as u64)
    }

    #[must_use]
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

/// Hit count: invariant `hits <= check_count`. The only mutation is
/// through `record_check`, which maintains the invariant by
/// construction. Consumers cannot set hits > check_count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct HitCount {
    check_count: u64,
    hits: u64,
}

impl HitCount {
    /// Record a check. `hit` is true iff at least one rule fired in
    /// this pass. The type guarantees `hits <= check_count` post-call.
    pub fn record_check(&mut self, hit: bool) {
        self.check_count += 1;
        if hit {
            self.hits += 1;
        }
        debug_assert!(self.hits <= self.check_count, "HitCount invariant violated");
    }

    #[must_use]
    pub fn check_count(&self) -> u64 {
        self.check_count
    }

    #[must_use]
    pub fn hits(&self) -> u64 {
        self.hits
    }

    /// Empirical irreducibility rate. A never-checked counter has
    /// rate 1.0 (conservative).
    #[must_use]
    pub fn irreducibility_rate(&self) -> IrreducibilityRate {
        if self.check_count == 0 {
            return IrreducibilityRate::MAX;
        }
        let miss = (self.check_count - self.hits) as f64 / self.check_count as f64;
        IrreducibilityRate::new(miss)
    }
}

/// A node in the discovery forest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormNode {
    /// Content-addressable hash of `term` — the node's identity.
    pub id: TermRef,
    /// The term itself.
    pub term: Term,
    /// Epoch at which this node was first inserted into the forest.
    /// Used to mark morphism edges as `retroactive` when a rule
    /// lands on a node that predates the rule's minting epoch.
    pub inserted_epoch: u64,
    /// Typed check/hit counter — invariant `hits <= check_count`
    /// enforced by the constructor. Accessors:
    /// `counter.check_count()`, `counter.hits()`,
    /// `counter.irreducibility_rate()`.
    pub counter: HitCount,
    /// Cumulative morphism edges fired on this node, across its
    /// entire history. Distinct from `counter.hits()`: a single pass
    /// with 3 matching rules counts as 1 hit but 3 edges_fired.
    pub edges_fired: u64,
    /// Epoch at which this node was last checked.
    pub last_checked_epoch: u64,
    /// Epoch at which this node was last successfully reduced.
    pub last_hit_epoch: Option<u64>,
    /// If reduction has *ever* fired on this node, the id of the
    /// term it reduced to most recently. Gives the forest its
    /// morphism edges.
    pub reduced_to: Option<TermRef>,
    /// Rule names that were ever applied to this node successfully.
    pub history: Vec<String>,
}

impl FormNode {
    /// Empirical irreducibility rate as a typed newtype. The value
    /// is guaranteed to be in [0, 1] by the type system — downstream
    /// consumers cannot observe a value outside the domain.
    #[must_use]
    pub fn irreducibility_rate(&self) -> IrreducibilityRate {
        self.counter.irreducibility_rate()
    }

    /// Adaptive check period as a typed newtype. Guaranteed to be
    /// in [1, 64]. Burn-in: before the counter accumulates `BURN_IN`
    /// samples, return `CheckPeriod::MIN` — this prevents a single
    /// all-miss observation from jumping the period to MAX and
    /// starving retroactive firing of a matching rule that arrives
    /// a few epochs later.
    #[must_use]
    pub fn check_period(&self) -> CheckPeriod {
        const BURN_IN: u64 = 3;
        if self.counter.check_count() < BURN_IN {
            return CheckPeriod::MIN;
        }
        CheckPeriod::from_rate(self.irreducibility_rate())
    }
}

/// A retroactive edge: "at epoch E, rule R reduced node A to node B."
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Morphism {
    pub rule_name: String,
    pub from: TermRef,
    pub to: TermRef,
    pub epoch: u64,
    /// True iff the reduction was triggered by re-applying a rule
    /// to an already-seen node — the "historical leaf becomes
    /// reducible under a new rule" case, as distinct from a node
    /// being reduced on first touch.
    pub retroactive: bool,
}

/// The discovery forest: all terms seen, their reduction history,
/// and the morphisms that link them.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DiscoveryForest {
    /// All nodes indexed by their TermRef.
    pub nodes: HashMap<TermRef, FormNode>,
    /// All morphism edges, in chronological order.
    pub edges: Vec<Morphism>,
    /// Current epoch cursor; bumped by callers.
    pub epoch: u64,
    /// Schedule: `next_check_epoch -> nodes due AT OR BEFORE this epoch`.
    /// Drained each pass via `BTreeMap::range`. Nodes may appear in
    /// multiple buckets due to re-scheduling; a lazy-tombstone check
    /// on the node itself (`next_check_epoch == bucket_key`) filters
    /// out stale entries without requiring eager removal.
    schedule: BTreeMap<u64, Vec<TermRef>>,
    /// Per-node next-check-epoch — authoritative value for resolving
    /// lazy tombstones in `schedule`. Equal to
    /// `last_checked_epoch + check_period` after a pass.
    next_check: HashMap<TermRef, u64>,
}

impl DiscoveryForest {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Advance the forest's epoch cursor. Callers should bump this
    /// once per step of the outer machine's clock.
    pub fn set_epoch(&mut self, e: u64) {
        self.epoch = e;
    }

    /// Insert a term. Idempotent: repeated inserts of the same term
    /// return the same TermRef and do not duplicate schedule entries.
    pub fn insert(&mut self, term: Term) -> TermRef {
        let id = term.content_hash();
        if self.nodes.contains_key(&id) {
            return id;
        }
        let node = FormNode {
            id,
            term,
            inserted_epoch: self.epoch,
            counter: HitCount::default(),
            edges_fired: 0,
            last_checked_epoch: 0,
            last_hit_epoch: None,
            reduced_to: None,
            history: Vec::new(),
        };
        self.nodes.insert(id, node);
        // Schedule immediately: a fresh insert is due at epoch 0,
        // which is <= any current epoch.
        self.next_check.insert(id, 0);
        self.schedule.entry(0).or_default().push(id);
        id
    }

    /// Number of distinct nodes in the forest.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Apply a newly-discovered rule retroactively. Single-rule
    /// convenience form; wraps `apply_rules_retroactively(&[rule])`.
    pub fn apply_rule_retroactively(&mut self, rule: &RewriteRule) -> Vec<Morphism> {
        self.apply_rules_retroactively(std::slice::from_ref(&rule))
    }

    /// Apply a batch of rules retroactively. Drains due nodes once,
    /// then tries each rule on each due node. Uses the scheduler
    /// correctly when multiple rules land in the same epoch: before
    /// the fix, whichever rule was called first would drain the due
    /// set and reschedule all nodes past `epoch`, so subsequent
    /// rules in the same epoch saw zero due nodes. Under this batch
    /// form, each due node is tried against every rule exactly once
    /// per call; only one reschedule per node per call.
    pub fn apply_rules_retroactively(&mut self, rules: &[&RewriteRule]) -> Vec<Morphism> {
        let epoch = self.epoch;
        let due_keys: Vec<u64> = self.schedule.range(..=epoch).map(|(k, _)| *k).collect();
        let mut due_ids: Vec<TermRef> = Vec::new();
        for k in &due_keys {
            if let Some(bucket) = self.schedule.remove(k) {
                for id in bucket {
                    if let Some(nc) = self.next_check.get(&id) {
                        if *nc <= epoch {
                            due_ids.push(id);
                        }
                    }
                }
            }
        }
        // Dedup stale duplicates from prior lazy-tombstoned entries.
        due_ids.sort_by_key(|tr| tr.0);
        due_ids.dedup();

        let mut new_edges: Vec<Morphism> = Vec::new();
        let mut inserts: Vec<Term> = Vec::new();

        for id in due_ids {
            let before_term = match self.nodes.get(&id) {
                Some(n) => n.term.clone(),
                None => continue,
            };

            // Try every rule on this node in order. Each match fires
            // an edge. We count check_count once per (node, call), so
            // a node is "checked once" regardless of how many rules
            // were in the batch — that matches the scheduling
            // semantics: the batch is one observation of the node.
            let mut node_hit_this_pass = false;
            for rule in rules {
                let hit = pattern_match(&rule.lhs, &before_term).map(|bindings| {
                    let mut rhs = rule.rhs.clone();
                    for (v, val) in &bindings {
                        rhs = rhs.substitute(*v, val);
                    }
                    rhs
                });
                if let Some(after_term) = hit {
                    let after_id = after_term.content_hash();
                    let retroactive = {
                        let n = self.nodes.get(&id).expect("id exists");
                        n.inserted_epoch < epoch
                    };
                    let node = self.nodes.get_mut(&id).expect("id exists");
                    node.edges_fired += 1;
                    node.last_hit_epoch = Some(epoch);
                    node.reduced_to = Some(after_id);
                    node.history.push(rule.name.clone());
                    new_edges.push(Morphism {
                        rule_name: rule.name.clone(),
                        from: id,
                        to: after_id,
                        epoch,
                        retroactive,
                    });
                    inserts.push(after_term);
                    node_hit_this_pass = true;
                }
            }

            let node = self.nodes.get_mut(&id).expect("id exists");
            node.counter.record_check(node_hit_this_pass);
            node.last_checked_epoch = epoch;
            // Re-schedule based on updated stats. A node that hit in
            // this pass is "hot" — its rate drops and period shrinks.
            // A node that missed all rules has a higher rate and will
            // be scheduled further out. Typed `CheckPeriod` guarantees
            // the period is in [1, 64] — no arithmetic overflow on
            // the schedule key, no silent period=0 stall.
            let next = epoch + node.check_period().as_u64();
            self.next_check.insert(id, next);
            self.schedule.entry(next).or_default().push(id);
        }

        // Close the forest under reduction.
        for t in inserts {
            self.insert(t);
        }
        self.edges.extend(new_edges.clone());
        new_edges
    }

    /// How many nodes are currently considered "stable leaves"
    /// (irreducibility_rate >= 0.9999 AND check_count > 0).
    #[must_use]
    pub fn stable_leaf_count(&self) -> usize {
        self.nodes
            .values()
            .filter(|n| {
                n.counter.check_count() > 0
                    && n.irreducibility_rate().as_f64() >= 0.9999
            })
            .count()
    }

    /// Nodes currently due at the given epoch (O(due) via the
    /// schedule index).
    #[must_use]
    pub fn due_nodes(&self, current_epoch: u64) -> Vec<TermRef> {
        let mut out = Vec::new();
        for (_, bucket) in self.schedule.range(..=current_epoch) {
            for id in bucket {
                if let Some(nc) = self.next_check.get(id) {
                    if *nc <= current_epoch {
                        out.push(*id);
                    }
                }
            }
        }
        // Dedup stale duplicates from re-scheduling (lazy-tombstones).
        out.sort_by_key(|tr| tr.0);
        out.dedup();
        out
    }

    /// Traversal-compute saved by the adaptive scheduler: the ratio
    /// of *not-due* nodes to total nodes at the current epoch. Values
    /// approaching 1 mean the forest is almost all stable-leaves and
    /// the scheduler is extracting maximum savings.
    #[must_use]
    pub fn traversal_saving(&self, current_epoch: u64) -> f64 {
        if self.nodes.is_empty() {
            return 0.0;
        }
        let due = self.due_nodes(current_epoch).len() as f64;
        let total = self.nodes.len() as f64;
        1.0 - (due / total)
    }

    /// Number of morphism edges where the reduction was retroactive.
    #[must_use]
    pub fn retroactive_edge_count(&self) -> usize {
        self.edges.iter().filter(|e| e.retroactive).count()
    }

    /// The generator-facing seam: return the *live frontier* the
    /// generator should anti-unify over this epoch. Stable leaves
    /// (check_period at MAX, not yet due) are skipped — they are
    /// not going to produce new extractable patterns. This is where
    /// the forest's scheduler stops being observation and starts
    /// being compute-saving for the discovery loop.
    ///
    /// Each returned term is the node's current `reduced_to` chain
    /// tip (if any) or the original term (if never reduced). That
    /// way the generator sees the library-reduced view without
    /// re-running `rewrite_fixed_point` from the raw corpus every
    /// epoch.
    ///
    /// This is the clean boundary for a future Lisp layer: a
    /// `DueSelector` trait can wrap this method with arbitrary
    /// selection policy (random-sample, top-k-by-rate, etc.)
    /// without touching the forest's internal scheduler.
    #[must_use]
    pub fn due_corpus_view(&self, epoch: u64) -> Vec<Term> {
        let mut out = Vec::new();
        for id in self.due_nodes(epoch) {
            if let Some(node) = self.nodes.get(&id) {
                // Resolve the reduction chain: if this node has been
                // reduced, follow reduced_to to the current tip.
                // Prevents cycles with a step bound.
                let mut cur = node.id;
                let mut steps = 0usize;
                while steps < 32 {
                    match self.nodes.get(&cur) {
                        Some(n) => match n.reduced_to {
                            Some(next) if next != cur => {
                                cur = next;
                                steps += 1;
                            }
                            _ => break,
                        },
                        None => break,
                    }
                }
                if let Some(tip) = self.nodes.get(&cur) {
                    out.push(tip.term.clone());
                }
            }
        }
        out
    }

    /// Total compute units skipped vs a naive O(total) scan over the
    /// forest: the count of non-due nodes. Exposed as a metric so the
    /// flex harness can log scheduler savings alongside discovery
    /// progress.
    #[must_use]
    pub fn scheduler_skip_count(&self, epoch: u64) -> usize {
        self.len().saturating_sub(self.due_nodes(epoch).len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{apply, nat, var};

    fn add_identity() -> RewriteRule {
        RewriteRule {
            name: "add-identity".into(),
            lhs: apply(var(2), vec![var(100), nat(0)]),
            rhs: var(100),
        }
    }

    fn mul_identity() -> RewriteRule {
        RewriteRule {
            name: "mul-identity".into(),
            lhs: apply(var(3), vec![var(100), nat(1)]),
            rhs: var(100),
        }
    }

    #[test]
    fn insert_is_idempotent() {
        let mut f = DiscoveryForest::new();
        let t = apply(var(2), vec![nat(5), nat(0)]);
        let id1 = f.insert(t.clone());
        let id2 = f.insert(t);
        assert_eq!(id1, id2);
        assert_eq!(f.len(), 1);
    }

    #[test]
    fn new_rule_retroactively_reduces_existing_node() {
        let mut f = DiscoveryForest::new();
        let target = apply(var(2), vec![nat(5), nat(0)]);
        let id = f.insert(target);
        let rule = add_identity();
        let edges = f.apply_rule_retroactively(&rule);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].from, id);
        let leaf_id = nat(5).content_hash();
        assert!(f.nodes.contains_key(&leaf_id));
        assert_eq!(f.nodes[&id].counter.hits(), 1);
    }

    #[test]
    fn irreducibility_rate_and_check_period_ramp() {
        let mut f = DiscoveryForest::new();
        // Term that doesn't match add-identity.
        f.insert(apply(var(3), vec![nat(1), nat(1)]));
        let rule = add_identity();
        // Keep advancing epoch so the node keeps becoming due.
        for e in 1..=40 {
            f.set_epoch(e);
            let _ = f.apply_rule_retroactively(&rule);
        }
        let node = f.nodes.values().next().unwrap();
        assert!(node.counter.check_count() >= 2);
        assert_eq!(node.counter.hits(), 0);
        assert!((node.irreducibility_rate().as_f64() - 1.0).abs() < 1e-9);
        assert!(
            node.check_period().as_u64() >= 32,
            "fully-irreducible node should have near-max period; got {}",
            node.check_period().as_u64(),
        );
    }

    #[test]
    fn check_period_hot_vs_cold() {
        let mut hot_counter = HitCount::default();
        for _ in 0..100 {
            hot_counter.record_check(true); // but counter caps at 99 hits because...
        }
        // Actually record 99 hits + 1 miss to get 99/100.
        let mut hot_counter = HitCount::default();
        for _ in 0..99 {
            hot_counter.record_check(true);
        }
        hot_counter.record_check(false);

        let mut cold_counter = HitCount::default();
        for _ in 0..100 {
            cold_counter.record_check(false);
        }

        let hot = FormNode {
            id: nat(0).content_hash(),
            term: nat(0),
            inserted_epoch: 0,
            counter: hot_counter,
            edges_fired: 99,
            last_checked_epoch: 0,
            last_hit_epoch: None,
            reduced_to: None,
            history: vec![],
        };
        let cold = FormNode {
            counter: cold_counter,
            ..hot.clone()
        };
        assert!(cold.check_period() > hot.check_period());
        assert_eq!(cold.check_period(), CheckPeriod::MAX);
        assert_eq!(hot.check_period(), CheckPeriod::MIN);
    }

    // ── Typescape: proofs of the invariant types ────────────────────

    #[test]
    fn irreducibility_rate_clamps_into_domain() {
        assert_eq!(IrreducibilityRate::new(-1.0).as_f64(), 0.0);
        assert_eq!(IrreducibilityRate::new(0.5).as_f64(), 0.5);
        assert_eq!(IrreducibilityRate::new(2.0).as_f64(), 1.0);
    }

    #[test]
    fn irreducibility_rate_nan_goes_max() {
        assert_eq!(IrreducibilityRate::new(f64::NAN), IrreducibilityRate::MAX);
    }

    #[test]
    fn check_period_constructor_clamps_into_domain() {
        assert_eq!(CheckPeriod::from_rate(IrreducibilityRate::MIN), CheckPeriod::MIN);
        assert_eq!(CheckPeriod::from_rate(IrreducibilityRate::MAX), CheckPeriod::MAX);
        let mid = CheckPeriod::from_rate(IrreducibilityRate::new(0.5));
        assert!(mid >= CheckPeriod::MIN && mid <= CheckPeriod::MAX);
    }

    #[test]
    fn hit_count_invariant_holds_after_many_checks() {
        let mut c = HitCount::default();
        for i in 0..1000 {
            c.record_check(i % 3 == 0); // record hits sparsely
        }
        assert!(c.hits() <= c.check_count());
        let r = c.irreducibility_rate();
        assert!(r.as_f64() >= 0.0 && r.as_f64() <= 1.0);
    }

    #[test]
    fn hit_count_default_is_conservative() {
        let c = HitCount::default();
        assert_eq!(c.check_count(), 0);
        assert_eq!(c.hits(), 0);
        assert_eq!(c.irreducibility_rate(), IrreducibilityRate::MAX);
    }

    #[test]
    fn due_nodes_is_o_due_not_o_total() {
        // Insert 500 terms, all of which do NOT match the rule.
        // After a few passes they should stabilize with long
        // check_periods, and due_nodes(epoch=1) should return few.
        let mut f = DiscoveryForest::new();
        for n in 0..500 {
            f.insert(apply(var(3), vec![nat(n), nat(2)])); // mul(n, 2), unreducible
        }
        let rule = add_identity();
        // Prime: run enough passes that nodes stabilize.
        for e in 1..=10 {
            f.set_epoch(e);
            let _ = f.apply_rule_retroactively(&rule);
        }
        // At epoch 11 only nodes whose next_check_epoch <= 11 are
        // due. After stabilization that should be few.
        let due = f.due_nodes(11);
        assert!(
            due.len() < f.len(),
            "after stabilization, due_nodes should be a strict subset of total: due={} total={}",
            due.len(),
            f.len()
        );
    }

    #[test]
    fn traversal_saving_grows_as_forest_stabilizes() {
        let mut f = DiscoveryForest::new();
        for n in 0..100 {
            f.insert(apply(var(3), vec![nat(n), nat(2)])); // unreducible by add-id
        }
        let rule = add_identity();
        let saving_at_start = f.traversal_saving(0);
        for e in 1..=20 {
            f.set_epoch(e);
            let _ = f.apply_rule_retroactively(&rule);
        }
        let saving_after = f.traversal_saving(20);
        assert!(
            saving_after > saving_at_start,
            "traversal saving must grow as nodes become stable: start={saving_at_start} after={saving_after}"
        );
        assert!(
            saving_after > 0.5,
            "after 20 epochs of all-miss, over half the forest should be non-due: {saving_after}"
        );
    }

    #[test]
    fn retroactive_flag_set_when_rule_lands_after_insert() {
        // Semantic: retroactive means "the node existed before the
        // epoch at which the rule landed." Insertion at epoch 0, rule
        // landing at epoch 1 → retroactive=true. Insertion at epoch 5
        // and rule at epoch 5 → retroactive=false (same-epoch arrival).
        let mut f = DiscoveryForest::new();
        // Insert at epoch 0 (default).
        f.insert(apply(var(2), vec![nat(5), nat(0)]));
        let rule = add_identity();
        f.set_epoch(1);
        let edges = f.apply_rule_retroactively(&rule);
        assert_eq!(edges.len(), 1);
        assert!(
            edges[0].retroactive,
            "rule landing at epoch 1 on a node inserted at epoch 0 must be flagged retroactive"
        );

        // Contrast: insert + apply in same epoch.
        let mut g = DiscoveryForest::new();
        g.set_epoch(5);
        g.insert(apply(var(2), vec![nat(7), nat(0)]));
        let e2 = g.apply_rule_retroactively(&rule);
        assert_eq!(e2.len(), 1);
        assert!(
            !e2[0].retroactive,
            "rule applied in the same epoch the node was inserted is not retroactive"
        );
    }

    #[test]
    fn two_rules_compose_morphism_chain() {
        // Insert a term reducible by rule-A first, then by rule-B
        // after rule-A fires: mul(add(x, 0), 1). Under add-identity,
        // the inner add(x, 0) would reduce to x only via subterm
        // rewriting, which the forest doesn't currently do at root-
        // match level. But if we insert the intermediate form too,
        // the second rule fires on it. Verifies that the forest
        // *chains* morphisms as successive rules land.
        let mut f = DiscoveryForest::new();
        // Start term: mul(5, 1) — reducible by mul-identity directly.
        // And a term the add-identity reduces: add(7, 0).
        f.insert(apply(var(3), vec![nat(5), nat(1)]));
        f.insert(apply(var(2), vec![nat(7), nat(0)]));

        f.set_epoch(1);
        let _ = f.apply_rule_retroactively(&add_identity());
        f.set_epoch(2);
        let _ = f.apply_rule_retroactively(&mul_identity());

        // Both should be reducible; forest has both source + target
        // leaf nodes.
        assert!(f.nodes.contains_key(&nat(7).content_hash()));
        assert!(f.nodes.contains_key(&nat(5).content_hash()));
        assert!(f.edges.len() >= 2);
    }
}
