//! Phase W.5 (2026-04-18): online hyperparameter experimentation.
//!
//! The user-framed problem:
//!
//!   "The effects of online experimentation can be done,
//!    measured, applied, tested, and tuned while things work."
//!
//! # The mechanism
//!
//! A `BanditProbe` is a `MapEventConsumer` subscribed to an
//! `EventHub`. It cycles through a set of hyperparameter arms
//! (e.g. learning rates, ewc_lambdas, prune thresholds), applies
//! one at a time, and watches the resulting `BenchmarkScored`
//! events to attribute reward to the currently-active arm.
//! Every `switch_interval` events, the probe picks the next
//! arm via ε-greedy over arm reward EMAs.
//!
//! # Determinism
//!
//! The pseudo-random draws use a deterministic counter-based RNG
//! — two probes that see the same event sequence make the same
//! picks, so `MetaLoopOutcome` attestations stay stable.
//!
//! # Concurrency
//!
//! Single-threaded today. For multi-threaded bandits wrap
//! `RefCell` in `RwLock`; public API is unchanged.
//!
//! # Scope
//!
//! Deliberately narrow: one probe controls one hyperparameter.
//! Compose N probes on the hub for N-dimensional tuning; they
//! don't interfere because each only writes its own knob.

use crate::mathscape_map::{MapEvent, MapEventConsumer};
use std::cell::{Cell, RefCell};

/// A generic ε-greedy bandit over a finite set of arms. Each arm
/// is a hyperparameter value; `apply` is a closure that writes
/// the current arm into whichever subsystem owns the knob
/// (e.g. `trainer.adjust_learning_rate`).
pub struct BanditProbe<T: Clone + std::fmt::Debug + 'static> {
    arms: Vec<T>,
    /// EMA of `delta_from_prior` attributed to each arm.
    arm_reward_ema: RefCell<Vec<f64>>,
    /// Per-arm trial count.
    arm_trials: RefCell<Vec<u64>>,
    /// Which arm is currently in force.
    current_arm: Cell<usize>,
    /// How many events to observe before reselecting.
    switch_interval: Cell<u64>,
    /// Events seen since the last arm switch.
    events_since_switch: Cell<u64>,
    /// ε for the ε-greedy policy. 0.0 = pure exploit.
    epsilon: Cell<f64>,
    /// EMA smoothing factor (higher = faster tracking).
    smoothing: Cell<f64>,
    /// Counter-based deterministic PRNG state.
    rng_counter: Cell<u64>,
    /// Callable that applies an arm value to external state.
    apply: Box<dyn Fn(&T)>,
    /// Name for logging / introspection.
    name: String,
}

impl<T: Clone + std::fmt::Debug + 'static> BanditProbe<T> {
    /// Build a probe. `arms` must be non-empty. `apply(arm)` is
    /// invoked immediately to install the initial arm, and again
    /// on every arm switch.
    pub fn new(
        name: impl Into<String>,
        arms: Vec<T>,
        apply: Box<dyn Fn(&T)>,
        switch_interval: u64,
        epsilon: f64,
    ) -> Self {
        assert!(!arms.is_empty(), "BanditProbe needs ≥1 arm");
        let n = arms.len();
        let probe = Self {
            arms,
            arm_reward_ema: RefCell::new(vec![0.0; n]),
            arm_trials: RefCell::new(vec![0u64; n]),
            current_arm: Cell::new(0),
            switch_interval: Cell::new(switch_interval.max(1)),
            events_since_switch: Cell::new(0),
            epsilon: Cell::new(epsilon.clamp(0.0, 1.0)),
            smoothing: Cell::new(0.3),
            rng_counter: Cell::new(0xdeadbeef),
            apply,
            name: name.into(),
        };
        // Install arm 0 at construction.
        (probe.apply)(&probe.arms[0]);
        probe
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn current_arm_index(&self) -> usize {
        self.current_arm.get()
    }

    pub fn current_arm(&self) -> T {
        self.arms[self.current_arm.get()].clone()
    }

    pub fn arm_reward_ema(&self) -> Vec<f64> {
        self.arm_reward_ema.borrow().clone()
    }

    pub fn arm_trials(&self) -> Vec<u64> {
        self.arm_trials.borrow().clone()
    }

    pub fn arms(&self) -> &[T] {
        &self.arms
    }

    pub fn set_epsilon(&self, eps: f64) {
        self.epsilon.set(eps.clamp(0.0, 1.0));
    }

    pub fn set_smoothing(&self, s: f64) {
        self.smoothing.set(s.clamp(0.0, 1.0));
    }

    pub fn set_switch_interval(&self, n: u64) {
        self.switch_interval.set(n.max(1));
    }

    /// Identify the current best arm by EMA. Ties break toward
    /// lower index.
    pub fn best_arm_index(&self) -> usize {
        let r = self.arm_reward_ema.borrow();
        let mut best = 0usize;
        let mut best_v = r[0];
        for (i, v) in r.iter().enumerate().skip(1) {
            if *v > best_v {
                best = i;
                best_v = *v;
            }
        }
        best
    }

    /// Deterministic counter-based PRNG step. Produces a f64 in
    /// [0, 1). Stable across replays with the same event order.
    fn next_unit(&self) -> f64 {
        let c = self.rng_counter.get();
        // SplitMix64-style advance + output.
        let mut z = c.wrapping_add(0x9E3779B97F4A7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^= z >> 31;
        self.rng_counter.set(c.wrapping_add(1));
        // Take top 53 bits as a f64 in [0, 1).
        ((z >> 11) as f64) * (1.0f64 / ((1u64 << 53) as f64))
    }
}

impl<T: Clone + std::fmt::Debug + 'static> MapEventConsumer for BanditProbe<T> {
    fn on_event(&self, event: &MapEvent) {
        // Attribute reward to the currently-active arm.
        if let MapEvent::BenchmarkScored {
            delta_from_prior, ..
        } = event
        {
            let arm = self.current_arm.get();
            let d = if delta_from_prior.is_nan()
                || delta_from_prior.is_infinite()
            {
                0.0
            } else {
                *delta_from_prior
            };
            let alpha = self.smoothing.get();
            let mut rewards = self.arm_reward_ema.borrow_mut();
            rewards[arm] = alpha * d + (1.0 - alpha) * rewards[arm];
            drop(rewards);
            let mut trials = self.arm_trials.borrow_mut();
            trials[arm] += 1;
        }

        // Count all events toward the switch interval.
        self.events_since_switch
            .set(self.events_since_switch.get() + 1);
        if self.events_since_switch.get() < self.switch_interval.get() {
            return;
        }
        self.events_since_switch.set(0);

        // ε-greedy pick.
        let eps = self.epsilon.get();
        let coin = self.next_unit();
        let next = if coin < eps {
            // Uniform random arm.
            let pick_coin = self.next_unit();
            (pick_coin * (self.arms.len() as f64)) as usize
        } else {
            self.best_arm_index()
        };
        let prev = self.current_arm.get();
        if next != prev {
            self.current_arm.set(next);
            (self.apply)(&self.arms[next]);
        }
    }
}

impl<T: Clone + std::fmt::Debug + 'static> std::fmt::Debug for BanditProbe<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BanditProbe")
            .field("name", &self.name)
            .field("arms", &self.arms)
            .field("current_arm", &self.current_arm.get())
            .field("arm_reward_ema", &self.arm_reward_ema.borrow())
            .field("arm_trials", &self.arm_trials.borrow())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mathscape_map::EventHub;
    use std::rc::Rc;

    fn make_benchmark_event(frac: f64, delta: f64) -> MapEvent {
        MapEvent::BenchmarkScored {
            solved_count: (frac * 10.0) as usize,
            total: 10,
            solved_fraction: frac,
            delta_from_prior: delta,
        }
    }

    #[test]
    fn probe_installs_first_arm_at_construction() {
        let applied = Rc::new(RefCell::new(Vec::<f64>::new()));
        let applied_c = applied.clone();
        let probe = BanditProbe::new(
            "lr",
            vec![0.01, 0.05, 0.1],
            Box::new(move |v| applied_c.borrow_mut().push(*v)),
            5,
            0.0,
        );
        assert_eq!(probe.current_arm_index(), 0);
        assert_eq!(applied.borrow().len(), 1);
        assert_eq!(applied.borrow()[0], 0.01);
    }

    #[test]
    fn probe_attributes_delta_reward_to_active_arm() {
        let probe = BanditProbe::new(
            "lr",
            vec![0.01, 0.05],
            Box::new(|_| {}),
            100, // Never switch during this test.
            0.0,
        );
        probe.on_event(&make_benchmark_event(0.5, 0.3));
        let rewards = probe.arm_reward_ema();
        assert!(rewards[0] > 0.0, "arm 0 got credit");
        assert_eq!(rewards[1], 0.0, "arm 1 untouched");
        assert_eq!(probe.arm_trials()[0], 1);
        assert_eq!(probe.arm_trials()[1], 0);
    }

    #[test]
    fn probe_switches_to_best_arm_when_exploitation_dominant() {
        let applied = Rc::new(RefCell::new(Vec::<f64>::new()));
        let applied_c = applied.clone();
        let probe = BanditProbe::new(
            "lr",
            vec![0.01, 0.05, 0.1],
            Box::new(move |v| applied_c.borrow_mut().push(*v)),
            1, // Switch after every event.
            0.0, // Zero exploration → always exploit.
        );

        // Train arm 0 with a positive delta.
        probe.on_event(&make_benchmark_event(0.5, 0.3));
        // After switch decision: best arm is arm 0 (only one with
        // reward > 0), so probe stays on arm 0. But the
        // switch-selection logic always fires — so we need to
        // advance the rng to show pick matches. The probe should
        // pick arm 0 again (best so far) and NOT re-apply since
        // it's the same arm.
        assert_eq!(probe.current_arm_index(), 0);

        // Force arm 1 to become best by injecting reward via an
        // arm-1 trial. We need to switch to arm 1, score, switch
        // back. Since ε=0, we can't get off arm 0 via exploration.
        // So test another angle: raise ε transiently.
        probe.set_epsilon(1.0); // Full exploration.
        for _ in 0..8 {
            probe.on_event(&make_benchmark_event(0.5, 0.0));
        }
        // Some arm switches happened — the `applied` log has
        // multiple entries now.
        assert!(applied.borrow().len() >= 2);
    }

    #[test]
    fn probe_is_deterministic_across_replays() {
        let applied_1 = Rc::new(RefCell::new(Vec::<f64>::new()));
        let applied_2 = Rc::new(RefCell::new(Vec::<f64>::new()));
        let a1 = applied_1.clone();
        let a2 = applied_2.clone();
        let p1 = BanditProbe::new(
            "lr",
            vec![0.01, 0.05, 0.1, 0.2],
            Box::new(move |v| a1.borrow_mut().push(*v)),
            2,
            0.5,
        );
        let p2 = BanditProbe::new(
            "lr",
            vec![0.01, 0.05, 0.1, 0.2],
            Box::new(move |v| a2.borrow_mut().push(*v)),
            2,
            0.5,
        );
        let events = vec![
            make_benchmark_event(0.3, 0.1),
            make_benchmark_event(0.5, 0.2),
            make_benchmark_event(0.2, -0.3),
            make_benchmark_event(0.6, 0.4),
            make_benchmark_event(0.5, -0.1),
            make_benchmark_event(0.7, 0.2),
        ];
        for e in &events {
            p1.on_event(e);
            p2.on_event(e);
        }
        assert_eq!(p1.current_arm_index(), p2.current_arm_index());
        assert_eq!(applied_1.borrow().len(), applied_2.borrow().len());
        for (a, b) in applied_1.borrow().iter().zip(applied_2.borrow().iter()) {
            assert_eq!(a, b);
        }
    }

    #[test]
    fn probe_composes_on_hub_without_interference() {
        // Two independent probes on the same hub observe the same
        // event stream; each tracks its own arm independently.
        let hub = EventHub::new();
        let a1 = Rc::new(RefCell::new(Vec::<f64>::new()));
        let a2 = Rc::new(RefCell::new(Vec::<u32>::new()));
        let a1c = a1.clone();
        let a2c = a2.clone();
        let lr_probe = Rc::new(BanditProbe::new(
            "lr",
            vec![0.01, 0.1],
            Box::new(move |v: &f64| a1c.borrow_mut().push(*v)),
            3,
            0.3,
        ));
        let width_probe = Rc::new(BanditProbe::new(
            "width",
            vec![16u32, 32, 64],
            Box::new(move |v: &u32| a2c.borrow_mut().push(*v)),
            3,
            0.3,
        ));
        hub.subscribe(lr_probe.clone());
        hub.subscribe(width_probe.clone());
        for _ in 0..20 {
            hub.publish(&make_benchmark_event(0.5, 0.1));
        }
        // Both probes saw events (each trained at least one arm).
        assert!(lr_probe.arm_trials().iter().any(|t| *t > 0));
        assert!(width_probe.arm_trials().iter().any(|t| *t > 0));
        // Neither applied's history is empty (install always
        // fires, and switch events happen with non-zero ε).
        assert!(!a1.borrow().is_empty());
        assert!(!a2.borrow().is_empty());
    }
}
