//! The authored acquisition spec — the data the interpreter walks.
//!
//! An `AcquisitionSpec` is a genesis complexity plus an ordered list of
//! `AcquisitionOp`s (seal a layer, absorb a layer, fold back through a
//! primitive, advance the frontier). It is authored declaratively as a
//! `(defacquisition …)` tatara-lisp form (see `spec_lisp.rs`) and driven
//! by `interp::apply`.

use crate::types::Complexity;
use mathscape_core::eval::RewriteRule;
use serde::{Deserialize, Serialize};

/// One authored acquisition step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AcquisitionOp {
    /// Confirm a candidate rule and, iff it seals, commit it as a layer
    /// (INV-1). `k` = sample count for the shipped floor seal.
    SealLayer { rule: RewriteRule, k: usize },
    /// Confirm a candidate and absorb it **iff** folding it back lowers
    /// total DL (§1.5 significance-as-a-type). `k` = sample count.
    Absorb { rule: RewriteRule, k: usize },
    /// Fold the earlier layers back through the layer at
    /// `primitive_index` in the current head node (INV-2, trusted-
    /// primitive floor).
    FoldBack { primitive_index: usize },
    /// Advance the frontier's ceiling to `to` (mint a primitive that
    /// raises `K`). Chaitin: never closes the frontier, always opens a
    /// higher one (§1.5, §3.5.4).
    AdvanceFrontier { to: Complexity },
}

/// A full authored acquisition run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcquisitionSpec {
    /// Human-readable name, echoed into the `(defacquisition …)` form.
    pub name: String,
    /// The genesis frontier ceiling — the substrate's starting `K`.
    pub genesis_ceiling: Complexity,
    pub ops: Vec<AcquisitionOp>,
}

impl AcquisitionSpec {
    #[must_use]
    pub fn new(name: impl Into<String>, genesis_ceiling: Complexity) -> Self {
        AcquisitionSpec {
            name: name.into(),
            genesis_ceiling,
            ops: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_op(mut self, op: AcquisitionOp) -> Self {
        self.ops.push(op);
        self
    }

    /// Push a seal-a-layer op.
    #[must_use]
    pub fn seal(self, rule: RewriteRule, k: usize) -> Self {
        self.with_op(AcquisitionOp::SealLayer { rule, k })
    }

    /// Push an absorb op.
    #[must_use]
    pub fn absorb(self, rule: RewriteRule, k: usize) -> Self {
        self.with_op(AcquisitionOp::Absorb { rule, k })
    }

    /// Push a fold-back op.
    #[must_use]
    pub fn fold_back(self, primitive_index: usize) -> Self {
        self.with_op(AcquisitionOp::FoldBack { primitive_index })
    }

    /// Push a frontier-advance op.
    #[must_use]
    pub fn advance(self, to: Complexity) -> Self {
        self.with_op(AcquisitionOp::AdvanceFrontier { to })
    }
}
