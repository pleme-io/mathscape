//! Multi-layer orchestrator — step 7 of `collapse-and-surprise.md`.
//!
//! The orchestrator composes the three phases per layer:
//!
//!   1. **Run until reduced** — epochs fire under the allocator
//!      until `check_reduction(..., policy)` returns `Reduced` or
//!      a per-layer `max_epochs` ceiling hits
//!   2. **Promote** — if the caller has a `PromotionSignal` +
//!      `AxiomIdentity` from the external bridge (mathscape-axiom-
//!      bridge → axiom-forge → rustc), they hand it to the
//!      orchestrator
//!   3. **Migrate** — `migrate_library` rewrites the library in the
//!      new primitive's substrate; layer counter increments
//!
//! The bridge invocation in step 2 is external because it requires
//! axiom-forge as a dependency (which mathscape-core deliberately
//! does not take). The orchestrator exposes the hook; the caller
//! (CLI, service) wires it.
//!
//! See `docs/arch/self-hosting-horizon.md` for why this is the
//! mechanical path to layer-deep discovery, and
//! `docs/arch/forced-realization.md` for the gate lattice the
//! orchestrator traverses on each layer.

use crate::control::{Allocator, EpochAction};
use crate::epoch::{Epoch, EpochTrace, Registry};
use crate::hash::TermRef;
use crate::lifecycle::AxiomIdentity;
use crate::migration::migrate_library;
use crate::promotion::{MigrationReport, PromotionSignal};
use crate::reduction::{check_reduction, reduction_pressure, ReductionPolicy, ReductionVerdict};
use crate::term::Term;
use serde::{Deserialize, Serialize};

/// Per-epoch summary collected during a layer's progression.
/// Compact enough to stream + store; rich enough to reconstruct
/// the layer's pressure / library / event-category trajectory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerEpochSnapshot {
    pub epoch_id: u64,
    pub action: String,
    pub library_size: usize,
    pub pressure: f64,
    pub delta_dl_discovery: f64,
    pub delta_dl_reinforce: f64,
    pub events_count: usize,
    pub registry_root: TermRef,
}

/// Terminal state + telemetry of a single layer run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerTrajectory {
    /// Layer index (0 for base, 1+ for post-migration layers).
    pub layer_id: u32,
    /// Epoch snapshots, in order.
    pub epochs: Vec<LayerEpochSnapshot>,
    /// Final reduction verdict under the layer's policy.
    pub terminal_verdict: ReductionVerdict,
    /// Final registry root — the layer's identity.
    pub terminal_root: TermRef,
    /// Whether the layer hit max_epochs before reducing.
    pub hit_epoch_cap: bool,
}

impl LayerTrajectory {
    /// Number of epochs actually run in this layer.
    #[must_use]
    pub fn epoch_count(&self) -> usize {
        self.epochs.len()
    }

    /// Sum of ΔDL contributed by Discovery events across the layer.
    #[must_use]
    pub fn total_discovery_delta(&self) -> f64 {
        self.epochs.iter().map(|s| s.delta_dl_discovery).sum()
    }

    /// Sum of ΔDL contributed by Reinforce events across the layer.
    #[must_use]
    pub fn total_reinforce_delta(&self) -> f64 {
        self.epochs.iter().map(|s| s.delta_dl_reinforce).sum()
    }
}

/// Build a snapshot from a trace + the post-epoch registry.
fn snapshot_from<R: Registry + ?Sized>(
    trace: &EpochTrace,
    registry: &R,
    pressure_before: f64,
) -> LayerEpochSnapshot {
    use crate::event::EventCategory;
    let mut delta_dl_discovery = 0.0f64;
    let mut delta_dl_reinforce = 0.0f64;
    for ev in &trace.events {
        match ev.category() {
            EventCategory::Discovery => delta_dl_discovery += ev.delta_dl(),
            EventCategory::Reinforce => delta_dl_reinforce += ev.delta_dl(),
            _ => {}
        }
    }
    LayerEpochSnapshot {
        epoch_id: trace.epoch_id,
        action: action_str(&trace.action),
        library_size: registry.len(),
        pressure: pressure_before,
        delta_dl_discovery,
        delta_dl_reinforce,
        events_count: trace.events.len(),
        registry_root: registry.root(),
    }
}

fn action_str(action: &Option<EpochAction>) -> String {
    match action {
        Some(EpochAction::Discover) => "Discover".into(),
        Some(EpochAction::Reinforce) => "Reinforce".into(),
        Some(EpochAction::Promote(_)) => "Promote".into(),
        Some(EpochAction::Migrate(_)) => "Migrate".into(),
        None => "None".into(),
    }
}

/// Run epochs until the library is maximally reduced under
/// `policy`, or `max_epochs` is reached. The allocator drives
/// dispatch; the corpus is held constant across all epochs in the
/// layer (per-layer corpus rotation is the caller's concern, above
/// this function).
///
/// Returns a `LayerTrajectory` with per-epoch snapshots + the
/// terminal verdict.
pub fn run_until_reduced<G, P, E, R>(
    epoch: &mut Epoch<G, P, E, R>,
    allocator: &mut Allocator,
    corpus: &[Term],
    policy: &ReductionPolicy,
    max_epochs: usize,
    layer_id: u32,
) -> LayerTrajectory
where
    G: crate::epoch::Generator,
    P: crate::epoch::Prover,
    E: crate::epoch::Emitter,
    R: Registry,
{
    let mut epochs = Vec::with_capacity(max_epochs);
    let mut hit_epoch_cap = false;

    for i in 0..max_epochs {
        // Early-exit check: if already reduced at entry, don't run
        // another epoch — the layer has converged. Exception: if
        // library is empty, we need at least one Discover epoch.
        if i > 0 && epoch.registry.len() > 0 {
            let verdict = check_reduction(&epoch.registry, policy);
            if matches!(verdict, ReductionVerdict::Reduced) {
                break;
            }
        }
        let pressure_before = reduction_pressure(&epoch.registry);
        let trace = epoch.step_auto(corpus, allocator);
        let snap = snapshot_from(&trace, &epoch.registry, pressure_before);
        epochs.push(snap);
        if i == max_epochs - 1 {
            let verdict = check_reduction(&epoch.registry, policy);
            if !matches!(verdict, ReductionVerdict::Reduced) {
                hit_epoch_cap = true;
            }
        }
    }

    let terminal_verdict = check_reduction(&epoch.registry, policy);
    let terminal_root = epoch.registry.root();
    LayerTrajectory {
        layer_id,
        epochs,
        terminal_verdict,
        terminal_root,
        hit_epoch_cap,
    }
}

/// A full multi-layer run's report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiLayerReport {
    pub layers: Vec<LayerTrajectory>,
    pub migrations: Vec<MigrationReport>,
    pub final_root: TermRef,
    pub deepest_reduced_layer: u32,
}

impl MultiLayerReport {
    /// How many layers actually reached `Reduced` before the
    /// trajectory ended.
    #[must_use]
    pub fn reduced_layer_count(&self) -> usize {
        self.layers
            .iter()
            .filter(|l| matches!(l.terminal_verdict, ReductionVerdict::Reduced))
            .count()
    }
}

/// The outcome a `PromotionHook` returns. The hook is the caller-
/// provided bridge integration — in production it invokes
/// mathscape-axiom-bridge, which calls axiom-forge, emits Rust,
/// and returns the AxiomIdentity if gates 6+7 pass.
#[derive(Debug, Clone)]
pub enum PromotionOutcome {
    /// Gates 6+7 passed; orchestrator should run migrate_library.
    Approved {
        identity: AxiomIdentity,
    },
    /// No candidate was found, or bridge rejected. Orchestrator
    /// stops (no more layers).
    Declined,
}

/// Callback type for plugging the bridge into the orchestrator.
/// Takes the current registry + pressure metric and decides
/// whether to fire a promotion.
pub type PromotionHook<'a, R> =
    Box<dyn FnMut(&R) -> Option<(PromotionSignal, PromotionOutcome)> + 'a>;

/// Multi-layer orchestrator state. Owns an `Epoch` + `Allocator`
/// plus per-layer policy; drives the layer-by-layer cycle.
pub struct MultiLayerRunner<G, P, E, R> {
    pub epoch: Epoch<G, P, E, R>,
    pub allocator: Allocator,
    pub per_layer_max_epochs: usize,
    pub max_layers: u32,
    pub policy: ReductionPolicy,
}

impl<G, P, E, R> MultiLayerRunner<G, P, E, R>
where
    G: crate::epoch::Generator,
    P: crate::epoch::Prover,
    E: crate::epoch::Emitter,
    R: Registry,
{
    /// Drive the multi-layer loop: run layer K until reduced, invoke
    /// the promotion hook, migrate on Approved outcome, advance to
    /// layer K+1, repeat until max_layers or hook returns Declined.
    pub fn run(
        &mut self,
        corpus: &[Term],
        mut promotion_hook: PromotionHook<'_, R>,
    ) -> MultiLayerReport {
        let mut layers = Vec::new();
        let mut migrations = Vec::new();
        let mut deepest_reduced: u32 = 0;

        for layer_id in 0..self.max_layers {
            let traj = run_until_reduced(
                &mut self.epoch,
                &mut self.allocator,
                corpus,
                &self.policy,
                self.per_layer_max_epochs,
                layer_id,
            );
            if matches!(traj.terminal_verdict, ReductionVerdict::Reduced) {
                deepest_reduced = layer_id;
            }
            layers.push(traj);

            // Consult the bridge hook. Caller decides if there's a
            // promotion to fire and what its AxiomIdentity is.
            let (signal, outcome) = match promotion_hook(&self.epoch.registry) {
                Some(pair) => pair,
                None => break, // no candidate — stop
            };
            match outcome {
                PromotionOutcome::Approved { identity } => {
                    let report = migrate_library(
                        &mut self.epoch.registry,
                        &signal,
                        identity,
                        self.epoch.epoch_id,
                    );
                    migrations.push(report);
                }
                PromotionOutcome::Declined => break,
            }
        }

        MultiLayerReport {
            final_root: self.epoch.registry.root(),
            deepest_reduced_layer: deepest_reduced,
            layers,
            migrations,
        }
    }
}

// Tests for the orchestrator live in tests/orchestrator.rs —
// moved out of this module because the tests need mathscape-compress
// + mathscape-reward as dev-deps, which causes a dependency-graph
// conflict with the lib test build (two views of mathscape-core).
