//! Phase V.spin (2026-04-18): the map of mathscape as a Merkle
//! tree being mutated by the motor.
//!
//! # The reframing
//!
//! Every discovered library has a canonical Merkle root
//! (`library_merkle_root`). Each motor phase that mutates the
//! library produces a new root. The trajectory of roots IS the
//! path through mathscape the machine traced; the set of rules
//! at each root IS the territory mapped.
//!
//! Multiple runs (different seeds, different scenarios) produce
//! different trajectories — different paths through the same
//! space. The UNION of their snapshots is the cross-run map of
//! mathscape discovered; the CORE (rules present in every run)
//! is the invariant mathematics the motor finds no matter how
//! it starts.
//!
//! # Why this is the right data structure
//!
//! Hash-identified. Content-addressable. Insertion-order
//! independent (sorted leaves). Permutation-comparable: two maps
//! from different seeds either share roots (same library reached
//! by different paths) or diverge (different mathematics found).
//!
//! # What this enables
//!
//! - Save/restore maps across sessions (bincode-serializable)
//! - Analyze permutations: which rules are universal? which are
//!   seed-specific? which roots recur?
//! - Visualize the mutation tree: nodes are library states, edges
//!   are motor phases
//! - Detect when the motor has explored a new region (root not
//!   in prior map) vs retraced old territory (root already known)
//!
//! The map becomes the operational target: every session's
//! purpose is to extend the map — add snapshots, discover new
//! roots, expand the union.

use crate::bootstrap::{library_merkle_root, LearningObservation};
use crate::eval::RewriteRule;
use crate::hash::TermRef;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// A single snapshot of the library's state at a moment.
/// Hash-identified by `library_root`. Two snapshots with the same
/// root are the same library (possibly reached by different paths).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MapSnapshot {
    /// The motor run this snapshot came from (seed).
    pub seed: u64,
    /// Phase index within that run.
    pub phase_index: usize,
    /// Merkle root of the library at this moment.
    pub library_root: TermRef,
    /// The library content itself.
    pub library: Vec<RewriteRule>,
    /// The observation that produced this snapshot (for audit +
    /// analysis: staleness level, policy delta, etc.).
    pub observation: Option<LearningObservation>,
}

impl MapSnapshot {
    #[must_use]
    pub fn new(
        seed: u64,
        phase_index: usize,
        library: Vec<RewriteRule>,
        observation: Option<LearningObservation>,
    ) -> Self {
        let library_root = library_merkle_root(&library);
        Self {
            seed,
            phase_index,
            library_root,
            library,
            observation,
        }
    }

    /// Number of rules at this snapshot.
    #[must_use]
    pub fn size(&self) -> usize {
        self.library.len()
    }
}

/// The map of mathscape — a collection of snapshots across one or
/// more motor runs. Saveable, mergeable, analyzable.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct MathscapeMap {
    pub snapshots: Vec<MapSnapshot>,
}

impl MathscapeMap {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, snapshot: MapSnapshot) {
        self.snapshots.push(snapshot);
    }

    /// Merge another map's snapshots into this one. Duplicate
    /// (seed, phase_index) pairs are NOT deduplicated — the caller
    /// is responsible for avoiding double-adds. Use this for
    /// cross-run unioning.
    pub fn merge(&mut self, other: MathscapeMap) {
        self.snapshots.extend(other.snapshots);
    }

    /// All distinct library-root hashes seen across every snapshot.
    /// The count is an upper bound on how many distinct library
    /// states the motor has visited — a measure of map breadth.
    #[must_use]
    pub fn unique_roots(&self) -> BTreeSet<TermRef> {
        self.snapshots
            .iter()
            .map(|s| s.library_root)
            .collect()
    }

    /// All distinct seeds represented in the map.
    #[must_use]
    pub fn seeds(&self) -> BTreeSet<u64> {
        self.snapshots.iter().map(|s| s.seed).collect()
    }

    /// All rules in the union — every rule discovered in any
    /// snapshot. Deduplicated by (lhs, rhs) structural equality.
    #[must_use]
    pub fn union_rules(&self) -> Vec<RewriteRule> {
        let mut seen: BTreeMap<(String, String), RewriteRule> =
            BTreeMap::new();
        for snap in &self.snapshots {
            for rule in &snap.library {
                let key = (format!("{:?}", rule.lhs), format!("{:?}", rule.rhs));
                seen.entry(key).or_insert_with(|| rule.clone());
            }
        }
        seen.into_values().collect()
    }

    /// Core rules — rules present in EVERY seed's final (largest-
    /// phase) snapshot. The invariant mathematics of the current
    /// motor configuration: what every run discovers regardless
    /// of starting point.
    #[must_use]
    pub fn core_rules(&self) -> Vec<RewriteRule> {
        let seeds: BTreeSet<u64> = self.seeds();
        if seeds.is_empty() {
            return Vec::new();
        }
        // Per-seed, pick the snapshot with the largest phase index
        // (the final state).
        let mut per_seed_final: BTreeMap<u64, &MapSnapshot> =
            BTreeMap::new();
        for snap in &self.snapshots {
            let entry = per_seed_final.entry(snap.seed).or_insert(snap);
            if snap.phase_index > entry.phase_index {
                *entry = snap;
            }
        }
        // Count rule occurrences across the per-seed finals.
        let mut counts: BTreeMap<(String, String), (RewriteRule, usize)> =
            BTreeMap::new();
        for snap in per_seed_final.values() {
            // Dedup within a single snapshot first (in case the
            // library has alpha-variants).
            let mut seen_this_snap: BTreeSet<(String, String)> =
                BTreeSet::new();
            for rule in &snap.library {
                let key = (format!("{:?}", rule.lhs), format!("{:?}", rule.rhs));
                if seen_this_snap.insert(key.clone()) {
                    let entry = counts
                        .entry(key)
                        .or_insert_with(|| (rule.clone(), 0));
                    entry.1 += 1;
                }
            }
        }
        let n = per_seed_final.len();
        counts
            .into_values()
            .filter(|(_, c)| *c == n)
            .map(|(r, _)| r)
            .collect()
    }

    /// Mutation edges — ordered (prev_root, next_root) pairs
    /// per-seed in phase order. The motor's trajectory through
    /// the root-space.
    #[must_use]
    pub fn mutation_edges(&self) -> Vec<(u64, TermRef, TermRef)> {
        let mut by_seed: BTreeMap<u64, Vec<&MapSnapshot>> =
            BTreeMap::new();
        for snap in &self.snapshots {
            by_seed.entry(snap.seed).or_default().push(snap);
        }
        let mut edges = Vec::new();
        for (seed, mut snaps) in by_seed {
            snaps.sort_by_key(|s| s.phase_index);
            for pair in snaps.windows(2) {
                if pair[0].library_root != pair[1].library_root {
                    edges.push((
                        seed,
                        pair[0].library_root,
                        pair[1].library_root,
                    ));
                }
            }
        }
        edges
    }

    /// Summary report — counts of snapshots / unique roots /
    /// union-size / core-size / edge count.
    #[must_use]
    pub fn summary(&self) -> MapSummary {
        MapSummary {
            total_snapshots: self.snapshots.len(),
            unique_roots: self.unique_roots().len(),
            seeds: self.seeds().len(),
            union_rule_count: self.union_rules().len(),
            core_rule_count: self.core_rules().len(),
            mutation_edges: self.mutation_edges().len(),
        }
    }
}

/// Summary statistics for a `MathscapeMap` — the shape of the
/// discovered territory at a glance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapSummary {
    pub total_snapshots: usize,
    /// Distinct library-root hashes — upper bound on the count
    /// of distinct library states visited.
    pub unique_roots: usize,
    pub seeds: usize,
    /// Size of the union library (any rule from any seed).
    pub union_rule_count: usize,
    /// Size of the core library (rules in every seed's final).
    pub core_rule_count: usize,
    /// Number of phase-transitions where the root CHANGED.
    /// A mutation that added/removed a rule. Transitions where
    /// the root stayed the same (e.g. when a phase discovered
    /// nothing) don't count.
    pub mutation_edges: usize,
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
    fn mul_id() -> RewriteRule {
        RewriteRule {
            name: "mul-id".into(),
            lhs: apply(var(3), vec![nat(1), var(100)]),
            rhs: var(100),
        }
    }

    #[test]
    fn empty_map_has_empty_summary() {
        let m = MathscapeMap::new();
        let s = m.summary();
        assert_eq!(s.total_snapshots, 0);
        assert_eq!(s.seeds, 0);
        assert_eq!(s.union_rule_count, 0);
        assert_eq!(s.core_rule_count, 0);
    }

    #[test]
    fn snapshot_computes_library_root() {
        let snap =
            MapSnapshot::new(1, 0, vec![add_id()], None);
        // Root is a 32-byte hash — not the zero hash.
        let zero = TermRef::from_bytes(&[]);
        // Empty library's root is empty-bytes hash; ours is not
        // empty-bytes.
        assert_ne!(snap.library_root, zero);
    }

    #[test]
    fn two_snapshots_same_content_same_root() {
        let s1 = MapSnapshot::new(1, 0, vec![add_id(), mul_id()], None);
        let s2 = MapSnapshot::new(7, 3, vec![mul_id(), add_id()], None);
        // Same rules, different order → same root.
        assert_eq!(s1.library_root, s2.library_root);
    }

    #[test]
    fn map_union_collects_all_rules() {
        let mut m = MathscapeMap::new();
        m.push(MapSnapshot::new(1, 0, vec![add_id()], None));
        m.push(MapSnapshot::new(2, 0, vec![mul_id()], None));
        let union = m.union_rules();
        assert_eq!(union.len(), 2);
    }

    #[test]
    fn map_core_is_intersection_of_seed_finals() {
        let mut m = MathscapeMap::new();
        m.push(MapSnapshot::new(1, 0, vec![add_id()], None));
        m.push(MapSnapshot::new(1, 1, vec![add_id(), mul_id()], None));
        m.push(MapSnapshot::new(2, 0, vec![mul_id()], None));
        m.push(MapSnapshot::new(2, 1, vec![add_id(), mul_id()], None));
        // Each seed's final has both rules → core is both rules.
        let core = m.core_rules();
        assert_eq!(core.len(), 2);
    }

    #[test]
    fn map_core_respects_seed_finals_only() {
        let mut m = MathscapeMap::new();
        // Seed 1 final has add_id + mul_id.
        m.push(MapSnapshot::new(1, 0, vec![add_id(), mul_id()], None));
        // Seed 2 final has only add_id — no mul_id.
        m.push(MapSnapshot::new(2, 0, vec![add_id()], None));
        // Core is only add_id (present in both finals).
        let core = m.core_rules();
        assert_eq!(core.len(), 1);
    }

    #[test]
    fn mutation_edges_record_root_transitions() {
        let mut m = MathscapeMap::new();
        // Seed 1: empty → {add_id} → {add_id, mul_id}
        m.push(MapSnapshot::new(1, 0, vec![], None));
        m.push(MapSnapshot::new(1, 1, vec![add_id()], None));
        m.push(MapSnapshot::new(1, 2, vec![add_id(), mul_id()], None));
        let edges = m.mutation_edges();
        assert_eq!(edges.len(), 2);
        assert_eq!(edges[0].0, 1);
        assert_ne!(edges[0].1, edges[0].2);
    }

    #[test]
    fn mutation_edges_skip_no_op_transitions() {
        // When root stays the same phase-to-phase, no edge.
        let mut m = MathscapeMap::new();
        m.push(MapSnapshot::new(1, 0, vec![add_id()], None));
        m.push(MapSnapshot::new(1, 1, vec![add_id()], None));
        m.push(MapSnapshot::new(1, 2, vec![add_id(), mul_id()], None));
        let edges = m.mutation_edges();
        assert_eq!(edges.len(), 1, "only the real mutation counted");
    }

    #[test]
    fn map_bincode_roundtrip() {
        let mut m = MathscapeMap::new();
        m.push(MapSnapshot::new(42, 3, vec![add_id(), mul_id()], None));
        let bytes = bincode::serialize(&m).unwrap();
        let back: MathscapeMap = bincode::deserialize(&bytes).unwrap();
        assert_eq!(m, back);
    }
}
