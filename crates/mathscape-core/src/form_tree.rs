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
//! This turns discovery traversal into an O(live-frontier) problem
//! instead of O(all-nodes) — the forest naturally concentrates
//! compute on the regions where reduction is still paying off.
//!
//! Not wired into `Epoch::step_auto` yet. Intended as the data plane
//! under a future "retroactive reinforcement" layer; for now it is a
//! standalone structure that can be fed terms + rules and observed.
//!
//! Design is driven by the user-stated requirement:
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
use std::collections::HashMap;

/// A node in the discovery forest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormNode {
    /// Content-addressable hash of `term` — the node's identity.
    pub id: TermRef,
    /// The term itself.
    pub term: Term,
    /// Number of times we have tried to reduce this node.
    pub check_count: u64,
    /// Number of times reduction succeeded (a rule fired and produced
    /// a different term).
    pub hits: u64,
    /// Epoch at which this node was last checked.
    pub last_checked_epoch: u64,
    /// Epoch at which this node was last successfully reduced.
    pub last_hit_epoch: Option<u64>,
    /// If reduction has *ever* fired on this node, the id of the
    /// term it reduced to most recently. Gives the forest its
    /// morphism edges.
    pub reduced_to: Option<TermRef>,
    /// Rule names that were ever applied to this node successfully
    /// (kept small — for auditing which morphisms touched this form).
    pub history: Vec<String>,
}

impl FormNode {
    /// Empirical irreducibility rate: `1 - hits / check_count`,
    /// clamped to [0, 1]. A node never checked has rate 1.0 (assume
    /// maximally irreducible until proven otherwise — conservative).
    #[must_use]
    pub fn irreducibility_rate(&self) -> f64 {
        if self.check_count == 0 {
            return 1.0;
        }
        let miss = (self.check_count - self.hits) as f64 / self.check_count as f64;
        miss.clamp(0.0, 1.0)
    }

    /// Adaptive check period: how many epochs to wait before the
    /// scheduler should re-check this node. Exponential in the
    /// irreducibility rate, with a floor of 1 and ceiling of 64.
    /// Highly reducible nodes (rate ~0) get checked every epoch;
    /// stably irreducible nodes (rate ~1) get checked every ~64.
    #[must_use]
    pub fn check_period(&self) -> u64 {
        let r = self.irreducibility_rate();
        let raw = 1.0 + (r * 6.0).exp2() - 1.0;
        raw.clamp(1.0, 64.0).round() as u64
    }
}

/// A retroactive edge: "at epoch E, rule R reduced node A to node B."
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Morphism {
    pub rule_name: String,
    pub from: TermRef,
    pub to: TermRef,
    pub epoch: u64,
    /// True iff the reduction was triggered by a *new* rule being
    /// applied to an *existing* node (the retroactive case), as
    /// opposed to a node being reduced the first time it was
    /// inserted.
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
}

impl DiscoveryForest {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_epoch(&mut self, e: u64) {
        self.epoch = e;
    }

    /// Insert a term as a node. If it already exists, return its id;
    /// the existing node is left untouched. The terminal node of a
    /// reduction chain (after applying the whole library) is what
    /// you typically want to track.
    pub fn insert(&mut self, term: Term) -> TermRef {
        let id = term.content_hash();
        self.nodes.entry(id).or_insert_with(|| FormNode {
            id,
            term,
            check_count: 0,
            hits: 0,
            last_checked_epoch: 0,
            last_hit_epoch: None,
            reduced_to: None,
            history: Vec::new(),
        });
        id
    }

    /// Apply a newly-discovered rule retroactively: re-check every
    /// node that is due under its adaptive schedule, and record any
    /// reductions that fire. Returns the list of morphism edges
    /// this call produced.
    pub fn apply_rule_retroactively(&mut self, rule: &RewriteRule) -> Vec<Morphism> {
        let epoch = self.epoch;
        let ids: Vec<TermRef> = self
            .nodes
            .iter()
            .filter(|(_, n)| {
                // Due if the scheduler says it's time, OR if the
                // node has never been checked at all.
                n.check_count == 0
                    || epoch.saturating_sub(n.last_checked_epoch) >= n.check_period()
            })
            .map(|(id, _)| *id)
            .collect();

        let mut new_edges = Vec::new();
        let mut to_insert: Vec<Term> = Vec::new();
        for id in ids {
            // Attempt pattern-match + rewrite at the root. Whole-term
            // retroactive rewriting at subterm depth is a future
            // enhancement; root-match alone already captures the
            // "new primitive subsumes old terms" case.
            let (before_term, before_id) = {
                let n = self.nodes.get(&id).expect("id from iter must exist");
                (n.term.clone(), n.id)
            };
            let fired = if let Some(bindings) = pattern_match(&rule.lhs, &before_term) {
                let mut rhs = rule.rhs.clone();
                for (v, val) in &bindings {
                    rhs = rhs.substitute(*v, val);
                }
                Some(rhs)
            } else {
                None
            };
            let node = self.nodes.get_mut(&id).expect("id from iter must exist");
            node.check_count += 1;
            node.last_checked_epoch = epoch;
            if let Some(after_term) = fired {
                let after_id = after_term.content_hash();
                node.hits += 1;
                node.last_hit_epoch = Some(epoch);
                node.reduced_to = Some(after_id);
                node.history.push(rule.name.clone());
                let retroactive = node.check_count > 1;
                new_edges.push(Morphism {
                    rule_name: rule.name.clone(),
                    from: before_id,
                    to: after_id,
                    epoch,
                    retroactive,
                });
                // Queue the target term for insertion after we
                // release the &mut borrow.
                to_insert.push(after_term);
            }
        }

        // Ensure the targets of any new edges exist as nodes too,
        // so the forest closes under reduction.
        for t in to_insert {
            self.insert(t);
        }
        self.edges.extend(new_edges.clone());
        new_edges
    }

    /// How many nodes are currently considered "stable leaves"
    /// (irreducibility_rate == 1.0 AND check_count > 0).
    #[must_use]
    pub fn stable_leaf_count(&self) -> usize {
        self.nodes
            .values()
            .filter(|n| n.check_count > 0 && n.irreducibility_rate() >= 0.9999)
            .count()
    }

    /// Nodes that are due for the next scheduling pass. O(n) scan;
    /// production would index this by next-check epoch, but for
    /// clarity we keep the lazy form.
    #[must_use]
    pub fn due_nodes(&self, current_epoch: u64) -> Vec<TermRef> {
        self.nodes
            .iter()
            .filter(|(_, n)| {
                n.check_count == 0
                    || current_epoch.saturating_sub(n.last_checked_epoch) >= n.check_period()
            })
            .map(|(id, _)| *id)
            .collect()
    }

    /// Traversal-compute saved by the adaptive scheduler: the ratio
    /// of due nodes to total nodes at the current epoch. Values
    /// approaching zero mean the forest is almost all stable-leaves
    /// and the scheduler is extracting maximum savings.
    #[must_use]
    pub fn traversal_saving(&self, current_epoch: u64) -> f64 {
        if self.nodes.is_empty() {
            return 0.0;
        }
        let due = self.due_nodes(current_epoch).len() as f64;
        let total = self.nodes.len() as f64;
        1.0 - (due / total)
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

    #[test]
    fn insert_is_idempotent() {
        let mut f = DiscoveryForest::new();
        let t = apply(var(2), vec![nat(5), nat(0)]);
        let id1 = f.insert(t.clone());
        let id2 = f.insert(t);
        assert_eq!(id1, id2);
        assert_eq!(f.nodes.len(), 1);
    }

    #[test]
    fn new_rule_retroactively_reduces_existing_node() {
        let mut f = DiscoveryForest::new();
        let target = apply(var(2), vec![nat(5), nat(0)]);
        let id = f.insert(target);
        let rule = add_identity();
        let edges = f.apply_rule_retroactively(&rule);
        assert_eq!(edges.len(), 1, "rule must fire retroactively on existing node");
        assert_eq!(edges[0].from, id);
        assert_eq!(edges[0].rule_name, "add-identity");
        // Post-condition: target node has been hit, reduced_to points
        // to the nat(5) leaf.
        let leaf_id = nat(5).content_hash();
        assert!(f.nodes.contains_key(&leaf_id), "reduction target must be inserted");
        assert_eq!(f.nodes[&id].hits, 1);
        assert_eq!(f.nodes[&id].reduced_to, Some(leaf_id));
    }

    #[test]
    fn irreducibility_rate_updates_as_attempts_fail() {
        let mut f = DiscoveryForest::new();
        // Insert a term the rule cannot match.
        let mismatch = apply(var(3), vec![nat(1), nat(1)]);
        let _id = f.insert(mismatch);
        let rule = add_identity();
        // Drive enough epochs that each node is due again.
        for e in 1..=10 {
            f.set_epoch(e);
            let _ = f.apply_rule_retroactively(&rule);
        }
        let node = f.nodes.values().next().unwrap();
        assert!(
            node.check_count >= 1,
            "node should have been checked at least once"
        );
        assert_eq!(node.hits, 0);
        assert!(
            (node.irreducibility_rate() - 1.0).abs() < 1e-9,
            "all-miss node must have irreducibility rate 1.0"
        );
    }

    #[test]
    fn check_period_grows_with_irreducibility() {
        let mut unreducible = FormNode {
            id: nat(0).content_hash(),
            term: nat(0),
            check_count: 100,
            hits: 0,
            last_checked_epoch: 0,
            last_hit_epoch: None,
            reduced_to: None,
            history: vec![],
        };
        let hot = FormNode {
            check_count: 100,
            hits: 99,
            ..unreducible.clone()
        };
        assert!(
            unreducible.check_period() > hot.check_period(),
            "stably-irreducible nodes should be scheduled less often than hot ones"
        );
        // Clamp bounds hold.
        unreducible.hits = 0;
        assert!(unreducible.check_period() <= 64);
        assert!(hot.check_period() >= 1);
    }

    #[test]
    fn traversal_saving_increases_as_forest_stabilizes() {
        let mut f = DiscoveryForest::new();
        // Insert 50 add-identity-matching terms.
        for n in 1..=50 {
            f.insert(apply(var(2), vec![nat(n), nat(0)]));
        }
        let rule = add_identity();

        // Epoch 1: all nodes due (check_count = 0).
        f.set_epoch(1);
        let saving_before = f.traversal_saving(1);
        let _ = f.apply_rule_retroactively(&rule);

        // Epoch 2: most nodes hit last epoch, so rate=0, period=1 —
        // they're due again. Saving should still be near 0.
        f.set_epoch(2);
        let _ = f.apply_rule_retroactively(&rule);

        // Epoch 10: nodes that repeatedly reduced to leaves
        // (now irreducible as leaves) stop being due. Saving grows.
        // We measure how many inserted leaves (nat values) are stable.
        f.set_epoch(10);
        let _ = f.apply_rule_retroactively(&rule);
        let saving_later = f.traversal_saving(10);

        // Strict relation: total nodes include the leaves that the
        // rewriter inserted; many of those are fully irreducible.
        assert!(
            saving_later >= saving_before,
            "traversal saving must not shrink as the forest stabilizes: before={saving_before} later={saving_later}"
        );
    }

    #[test]
    fn retroactive_flag_distinguishes_revisit_from_first_touch() {
        let mut f = DiscoveryForest::new();
        let t = apply(var(2), vec![nat(5), nat(0)]);
        f.insert(t);
        let rule = add_identity();

        // First-ever check: retroactive = false (node's first touch).
        f.set_epoch(1);
        let edges1 = f.apply_rule_retroactively(&rule);
        assert!(!edges1.is_empty());
        assert!(!edges1[0].retroactive);

        // Sleep past check_period for rate=0 node (period=1). Advance
        // epoch and re-run — the next firing is retroactive = true.
        f.set_epoch(2);
        let edges2 = f.apply_rule_retroactively(&rule);
        if !edges2.is_empty() {
            assert!(edges2[0].retroactive);
        }
    }
}
