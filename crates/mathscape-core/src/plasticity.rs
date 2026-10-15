//! Phase W.8 (2026-04-19): universal neuroplasticity.
//!
//! # The user-framed invariant
//!
//!   "Everything has an algorithm to phase out and choose what to
//!    phase out and choose what to reinforce, which is obviously
//!    neuroplasticity."
//!
//! # The shape
//!
//! Every adaptive subsystem of the loop implements `Plastic`.
//! The trait is intentionally narrow: one name, one capacity
//! count, one phase-out call, one reinforce call. Component-
//! specific logic (thresholds, arm-selection policies,
//! promotion gates) lives inside the component; the trait
//! exposes only the shed/reinforce interface.
//!
//! `PlasticityController` holds a collection of `Rc<dyn Plastic>`
//! components and runs one tick across all of them. A tick
//! produces a `PlasticityReport` with per-component before/after
//! counts.
//!
//! This is the outer loop (Loop 0 in the perpetual-improvement
//! architecture). Where the streaming trainer runs at event
//! granularity, the controller runs at *administrative*
//! granularity — called periodically by an orchestrator (motor
//! every K phases, or a timer, or a Lisp script) to enforce
//! the global phase-out/reinforce invariant.
//!
//! # Why it matters
//!
//! Without this trait, each component's neuroplasticity is a
//! collection of mutually-unaware knobs: the trainer has prune
//! + rejuvenate, the probe has arm-selection, the certifier has
//! promote/demote. They all do the same THING (shed weak,
//! amplify strong) in different VOCABULARIES. `Plastic` names
//! the common shape so an outer controller can drive all of
//! them uniformly — and so future components automatically
//! compose into the perpetual-improvement loop when they
//! implement the trait.

use std::cell::RefCell;
use std::rc::Rc;

/// Universal neuroplasticity interface. Every adaptive component
/// implements this so an outer controller can drive shed +
/// reinforce uniformly.
///
/// Single-threaded today (`&self` + interior mutability); when
/// Phase W.7 lands the async hub, the trait will be Send + Sync
/// and implementations will use atomics or RwLocks inside.
pub trait Plastic {
    /// Human-readable component name for reports.
    fn component_name(&self) -> &str;

    /// Count of currently-active items (unpruned weights,
    /// well-performing arms, certified rules, etc.).
    fn active_count(&self) -> usize;

    /// Count of currently-phased-out items (pruned weights,
    /// discarded arms, demoted rules).
    fn phased_out_count(&self) -> usize;

    /// Total capacity = active + phased_out. Components with
    /// unbounded capacity (e.g. rule library) can return
    /// `active_count + phased_out_count`; for components with
    /// bounded width (e.g. trainer) this is a fixed constant.
    fn capacity(&self) -> usize {
        self.active_count() + self.phased_out_count()
    }

    /// Utilization fraction: active / capacity.
    fn utilization(&self) -> f64 {
        let c = self.capacity();
        if c == 0 {
            0.0
        } else {
            self.active_count() as f64 / c as f64
        }
    }

    /// Run one phase-out pass using the component's internal
    /// criterion. Returns the count of items shed in this call.
    fn phase_out_stale(&self) -> usize;

    /// Run one reinforcement pass. Returns the count of items
    /// strengthened (rejuvenated weights, selected arms, promoted
    /// rules) in this call.
    fn reinforce_strong(&self) -> usize;
}

/// One per-component outcome from a controller tick.
#[derive(Clone, Debug)]
pub struct ComponentTick {
    pub name: String,
    pub active_before: usize,
    pub active_after: usize,
    pub phased_out_before: usize,
    pub phased_out_after: usize,
    pub phased_out_this_tick: usize,
    pub reinforced_this_tick: usize,
    pub utilization_after: f64,
}

/// Aggregated report across all components for one tick.
#[derive(Clone, Debug, Default)]
pub struct PlasticityReport {
    pub ticks: Vec<ComponentTick>,
    pub total_phased_out: usize,
    pub total_reinforced: usize,
}

impl PlasticityReport {
    /// Format as a short summary string.
    pub fn summary(&self) -> String {
        let mut s = format!(
            "plasticity: phased_out={} reinforced={} across {} components",
            self.total_phased_out,
            self.total_reinforced,
            self.ticks.len()
        );
        for t in &self.ticks {
            s.push_str(&format!(
                "\n  {}: active {} → {} (shed {}, reinforced {}, util {:.0}%)",
                t.name,
                t.active_before,
                t.active_after,
                t.phased_out_this_tick,
                t.reinforced_this_tick,
                t.utilization_after * 100.0
            ));
        }
        s
    }
}

/// The outer controller — holds a set of `Plastic` components and
/// runs uniform shed + reinforce ticks across all of them.
#[derive(Default)]
pub struct PlasticityController {
    components: RefCell<Vec<Rc<dyn Plastic>>>,
    tick_count: std::cell::Cell<u64>,
}

impl std::fmt::Debug for PlasticityController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlasticityController")
            .field("components", &self.components.borrow().len())
            .field("tick_count", &self.tick_count.get())
            .finish()
    }
}

impl PlasticityController {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a plastic component. The controller keeps an Rc;
    /// callers typically retain their own Rc to the same object
    /// so they can still query component-specific APIs.
    pub fn register(&self, component: Rc<dyn Plastic>) {
        self.components.borrow_mut().push(component);
    }

    pub fn component_count(&self) -> usize {
        self.components.borrow().len()
    }

    pub fn tick_count(&self) -> u64 {
        self.tick_count.get()
    }

    /// Run one tick: phase out stale + reinforce strong across
    /// every registered component. Returns a per-component report.
    pub fn tick(&self) -> PlasticityReport {
        let components: Vec<_> = self.components.borrow().clone();
        let mut ticks = Vec::with_capacity(components.len());
        let mut total_phased = 0usize;
        let mut total_reinforced = 0usize;
        for c in &components {
            let active_before = c.active_count();
            let phased_before = c.phased_out_count();
            let phased = c.phase_out_stale();
            let reinforced = c.reinforce_strong();
            let active_after = c.active_count();
            let phased_after = c.phased_out_count();
            let util = c.utilization();
            total_phased += phased;
            total_reinforced += reinforced;
            ticks.push(ComponentTick {
                name: c.component_name().to_string(),
                active_before,
                active_after,
                phased_out_before: phased_before,
                phased_out_after: phased_after,
                phased_out_this_tick: phased,
                reinforced_this_tick: reinforced,
                utilization_after: util,
            });
        }
        self.tick_count.set(self.tick_count.get() + 1);
        PlasticityReport {
            ticks,
            total_phased_out: total_phased,
            total_reinforced: total_reinforced,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// A fake plastic component for testing the controller without
    /// depending on concrete implementations.
    struct FakePlastic {
        name: String,
        active: Cell<usize>,
        phased: Cell<usize>,
        phase_out_per_call: usize,
        reinforce_per_call: usize,
    }

    impl Plastic for FakePlastic {
        fn component_name(&self) -> &str {
            &self.name
        }
        fn active_count(&self) -> usize {
            self.active.get()
        }
        fn phased_out_count(&self) -> usize {
            self.phased.get()
        }
        fn phase_out_stale(&self) -> usize {
            let take = self.phase_out_per_call.min(self.active.get());
            self.active.set(self.active.get() - take);
            self.phased.set(self.phased.get() + take);
            take
        }
        fn reinforce_strong(&self) -> usize {
            let take = self.reinforce_per_call.min(self.phased.get());
            self.phased.set(self.phased.get() - take);
            self.active.set(self.active.get() + take);
            take
        }
    }

    #[test]
    fn controller_ticks_across_all_registered_components() {
        let controller = PlasticityController::new();
        let a = Rc::new(FakePlastic {
            name: "a".into(),
            active: Cell::new(10),
            phased: Cell::new(0),
            phase_out_per_call: 2,
            reinforce_per_call: 0,
        });
        let b = Rc::new(FakePlastic {
            name: "b".into(),
            active: Cell::new(5),
            phased: Cell::new(3),
            phase_out_per_call: 0,
            reinforce_per_call: 1,
        });
        controller.register(a.clone());
        controller.register(b.clone());
        assert_eq!(controller.component_count(), 2);

        let report = controller.tick();
        assert_eq!(report.total_phased_out, 2);
        assert_eq!(report.total_reinforced, 1);
        assert_eq!(report.ticks.len(), 2);
        assert_eq!(a.active_count(), 8);
        assert_eq!(b.active_count(), 6);
        assert_eq!(controller.tick_count(), 1);
    }

    #[test]
    fn controller_tick_count_is_monotonic() {
        let controller = PlasticityController::new();
        controller.register(Rc::new(FakePlastic {
            name: "x".into(),
            active: Cell::new(5),
            phased: Cell::new(0),
            phase_out_per_call: 0,
            reinforce_per_call: 0,
        }));
        for i in 1..=5 {
            controller.tick();
            assert_eq!(controller.tick_count(), i);
        }
    }

    #[test]
    fn plasticity_report_summary_formats_correctly() {
        let controller = PlasticityController::new();
        controller.register(Rc::new(FakePlastic {
            name: "trainer".into(),
            active: Cell::new(9),
            phased: Cell::new(0),
            phase_out_per_call: 1,
            reinforce_per_call: 0,
        }));
        let report = controller.tick();
        let s = report.summary();
        assert!(s.contains("plasticity:"));
        assert!(s.contains("trainer"));
        assert!(s.contains("shed 1"));
    }

    #[test]
    fn utilization_handles_zero_capacity() {
        struct Empty;
        impl Plastic for Empty {
            fn component_name(&self) -> &str {
                "empty"
            }
            fn active_count(&self) -> usize {
                0
            }
            fn phased_out_count(&self) -> usize {
                0
            }
            fn phase_out_stale(&self) -> usize {
                0
            }
            fn reinforce_strong(&self) -> usize {
                0
            }
        }
        let e = Empty;
        assert_eq!(e.utilization(), 0.0);
        assert_eq!(e.capacity(), 0);
    }
}
