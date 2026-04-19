//! Phase W.9.1 — Sexp bridges for Phase V/W types.
//!
//! # What this enables
//!
//! With the hub (`EventHub`), streaming trainer
//! (`StreamingPolicyTrainer`), bandit probe (`BanditProbe`), and
//! plasticity controller (`PlasticityController`) all in place,
//! the final piece for in-memory Lisp morphing is a set of
//! Sexp bridges so a Lisp program can:
//!
//! 1. Observe events flowing through the hub (by converting
//!    `MapEvent` to Sexp).
//! 2. Read the trainer's full state (weights, Fisher,
//!    phantom gradients, benchmark history) as a single Sexp.
//! 3. Read a `PlasticityReport` summary of one controller tick.
//!
//! Writing back (from_sexp) is deferred until the full Lisp
//! adapter crate lands (Phase W.9 proper) — these bridges are
//! the read half, which is the larger share of the work
//! historically. A Lisp script that can OBSERVE the running
//! machine can already do most of what "in-memory morphing"
//! means: watch events, compute a new policy, and write it
//! back via the existing `policy_to_sexp` → `policy_from_sexp`
//! → `trainer.inject()` path.
//!
//! # Sexp forms
//!
//! ```text
//! (map-event :kind novel-root :seed 1 :phase-index 0 :library-size 5)
//! (map-event :kind core-grew :prev 0 :new 1 :rule-name "add-id")
//! (map-event :kind root-mutated :seed 1 :from 0 :to 1 :size-delta 2)
//! (map-event :kind staleness-crossed :seed 1 :phase-index 0 :threshold 0.6 :observed 0.9)
//! (map-event :kind rule-certified :rule-name "add-id" :evidence 96)
//! (map-event :kind rule-rejected :rule-name "bad-rule" :reason "low-support")
//! (map-event :kind benchmark-scored :solved 8 :total 10 :fraction 0.8 :delta 0.1)
//!
//! (trainer-snapshot
//!   :trained-steps    N
//!   :bias             0.0
//!   :weights          (w0 w1 ...)
//!   :fisher           (f0 f1 ...)
//!   :phantom          (p0 p1 ...)
//!   :pruned-count     K
//!   :events-seen      E
//!   :updates-applied  U
//!   :has-anchor       BOOL
//!   :ewc-lambda       0.5
//!   :benchmark-history (s0 s1 ...))
//!
//! (plasticity-report
//!   :total-phased-out N
//!   :total-reinforced M
//!   :ticks
//!     ((component :name "name"
//!                 :active-before A1 :active-after A2
//!                 :phased-out-before P1 :phased-out-after P2
//!                 :phased-out-this-tick D :reinforced-this-tick R
//!                 :utilization-after U)
//!      ...))
//! ```

use mathscape_core::mathscape_map::MapEvent;
use mathscape_core::plasticity::PlasticityReport;
use mathscape_core::streaming_policy::StreamingPolicyTrainer;
use tatara_lisp::ast::Sexp;

/// Convert one `MapEvent` to its canonical Sexp form.
#[must_use]
pub fn map_event_to_sexp(event: &MapEvent) -> Sexp {
    let mut items: Vec<Sexp> = vec![Sexp::symbol("map-event")];
    items.push(Sexp::keyword("kind"));
    items.push(Sexp::symbol(event.category()));
    match event {
        MapEvent::NovelRoot {
            seed,
            phase_index,
            root: _,
            library_size,
        } => {
            items.push(Sexp::keyword("seed"));
            items.push(Sexp::int(*seed as i64));
            items.push(Sexp::keyword("phase-index"));
            items.push(Sexp::int(*phase_index as i64));
            items.push(Sexp::keyword("library-size"));
            items.push(Sexp::int(*library_size as i64));
        }
        MapEvent::RootMutated {
            seed,
            from_phase,
            to_phase,
            prev_root: _,
            next_root: _,
            size_delta,
        } => {
            items.push(Sexp::keyword("seed"));
            items.push(Sexp::int(*seed as i64));
            items.push(Sexp::keyword("from"));
            items.push(Sexp::int(*from_phase as i64));
            items.push(Sexp::keyword("to"));
            items.push(Sexp::int(*to_phase as i64));
            items.push(Sexp::keyword("size-delta"));
            items.push(Sexp::int(*size_delta as i64));
        }
        MapEvent::CoreGrew {
            prev_core_size,
            new_core_size,
            added_rule,
        } => {
            items.push(Sexp::keyword("prev"));
            items.push(Sexp::int(*prev_core_size as i64));
            items.push(Sexp::keyword("new"));
            items.push(Sexp::int(*new_core_size as i64));
            items.push(Sexp::keyword("rule-name"));
            items.push(Sexp::string(&added_rule.name));
        }
        MapEvent::StalenessCrossed {
            seed,
            phase_index,
            threshold,
            observed,
        } => {
            items.push(Sexp::keyword("seed"));
            items.push(Sexp::int(*seed as i64));
            items.push(Sexp::keyword("phase-index"));
            items.push(Sexp::int(*phase_index as i64));
            items.push(Sexp::keyword("threshold"));
            items.push(Sexp::float(*threshold));
            items.push(Sexp::keyword("observed"));
            items.push(Sexp::float(*observed));
        }
        MapEvent::RuleCertified {
            rule,
            evidence_samples,
        } => {
            items.push(Sexp::keyword("rule-name"));
            items.push(Sexp::string(&rule.name));
            items.push(Sexp::keyword("evidence"));
            items.push(Sexp::int(*evidence_samples as i64));
        }
        MapEvent::RuleRejectedAtCertification { rule, reason } => {
            items.push(Sexp::keyword("rule-name"));
            items.push(Sexp::string(&rule.name));
            items.push(Sexp::keyword("reason"));
            items.push(Sexp::string(reason));
        }
        MapEvent::BenchmarkScored {
            solved_count,
            total,
            solved_fraction,
            delta_from_prior,
        } => {
            items.push(Sexp::keyword("solved"));
            items.push(Sexp::int(*solved_count as i64));
            items.push(Sexp::keyword("total"));
            items.push(Sexp::int(*total as i64));
            items.push(Sexp::keyword("fraction"));
            items.push(Sexp::float(*solved_fraction));
            items.push(Sexp::keyword("delta"));
            // NaN becomes a sentinel symbol so the Sexp stays
            // readable; writers typically check is_nan before
            // publishing.
            if delta_from_prior.is_nan() {
                items.push(Sexp::symbol("nan"));
            } else {
                items.push(Sexp::float(*delta_from_prior));
            }
        }
    }
    Sexp::List(items)
}

/// Convert a streaming-trainer full-state snapshot to Sexp.
#[must_use]
pub fn trainer_snapshot_to_sexp(trainer: &StreamingPolicyTrainer) -> Sexp {
    let snap = trainer.snapshot();
    let (counts, contribs, pruned) = trainer.weight_stats();
    let fisher = trainer.fisher_snapshot();
    let phantom = trainer.phantom_gradients();
    let history = trainer.benchmark_history();

    let mut items: Vec<Sexp> = vec![Sexp::symbol("trainer-snapshot")];
    items.push(Sexp::keyword("trained-steps"));
    items.push(Sexp::int(snap.trained_steps as i64));
    items.push(Sexp::keyword("bias"));
    items.push(Sexp::float(snap.bias));

    items.push(Sexp::keyword("weights"));
    items.push(Sexp::List(
        snap.weights.iter().map(|w| Sexp::float(*w)).collect(),
    ));

    items.push(Sexp::keyword("fisher"));
    items.push(Sexp::List(
        fisher.iter().map(|f| Sexp::float(*f)).collect(),
    ));

    items.push(Sexp::keyword("phantom"));
    items.push(Sexp::List(
        phantom.iter().map(|p| Sexp::float(*p)).collect(),
    ));

    items.push(Sexp::keyword("activation-counts"));
    items.push(Sexp::List(
        counts.iter().map(|c| Sexp::int(*c as i64)).collect(),
    ));

    items.push(Sexp::keyword("cumulative-contributions"));
    items.push(Sexp::List(
        contribs.iter().map(|c| Sexp::float(*c)).collect(),
    ));

    items.push(Sexp::keyword("pruned"));
    items.push(Sexp::List(
        pruned
            .iter()
            .map(|b| Sexp::symbol(if *b { "t" } else { "nil" }))
            .collect(),
    ));

    items.push(Sexp::keyword("pruned-count"));
    items.push(Sexp::int(trainer.pruned_count() as i64));
    items.push(Sexp::keyword("events-seen"));
    items.push(Sexp::int(trainer.events_seen() as i64));
    items.push(Sexp::keyword("updates-applied"));
    items.push(Sexp::int(trainer.updates_applied() as i64));
    items.push(Sexp::keyword("learning-rate"));
    items.push(Sexp::float(trainer.learning_rate()));
    items.push(Sexp::keyword("has-anchor"));
    items.push(Sexp::symbol(if trainer.has_anchor() { "t" } else { "nil" }));
    items.push(Sexp::keyword("ewc-lambda"));
    items.push(Sexp::float(trainer.ewc_lambda()));
    items.push(Sexp::keyword("learning-progress-window"));
    items.push(Sexp::int(trainer.learning_progress_window() as i64));

    items.push(Sexp::keyword("benchmark-history"));
    items.push(Sexp::List(
        history.iter().map(|h| Sexp::float(*h)).collect(),
    ));

    Sexp::List(items)
}

/// Convert a plasticity report to Sexp.
#[must_use]
pub fn plasticity_report_to_sexp(report: &PlasticityReport) -> Sexp {
    let mut items: Vec<Sexp> = vec![Sexp::symbol("plasticity-report")];
    items.push(Sexp::keyword("total-phased-out"));
    items.push(Sexp::int(report.total_phased_out as i64));
    items.push(Sexp::keyword("total-reinforced"));
    items.push(Sexp::int(report.total_reinforced as i64));

    items.push(Sexp::keyword("ticks"));
    let ticks = report
        .ticks
        .iter()
        .map(|t| {
            let mut xs: Vec<Sexp> = vec![Sexp::symbol("component")];
            xs.push(Sexp::keyword("name"));
            xs.push(Sexp::string(&t.name));
            xs.push(Sexp::keyword("active-before"));
            xs.push(Sexp::int(t.active_before as i64));
            xs.push(Sexp::keyword("active-after"));
            xs.push(Sexp::int(t.active_after as i64));
            xs.push(Sexp::keyword("phased-out-before"));
            xs.push(Sexp::int(t.phased_out_before as i64));
            xs.push(Sexp::keyword("phased-out-after"));
            xs.push(Sexp::int(t.phased_out_after as i64));
            xs.push(Sexp::keyword("phased-out-this-tick"));
            xs.push(Sexp::int(t.phased_out_this_tick as i64));
            xs.push(Sexp::keyword("reinforced-this-tick"));
            xs.push(Sexp::int(t.reinforced_this_tick as i64));
            xs.push(Sexp::keyword("utilization-after"));
            xs.push(Sexp::float(t.utilization_after));
            Sexp::List(xs)
        })
        .collect();
    items.push(Sexp::List(ticks));

    Sexp::List(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mathscape_core::eval::RewriteRule;
    use mathscape_core::hash::TermRef;
    use mathscape_core::mathscape_map::{EventHub, MapEventConsumer};
    use mathscape_core::plasticity::{Plastic, PlasticityController};
    use mathscape_core::term::Term;
    use mathscape_core::value::Value;
    use std::rc::Rc;

    fn add_rule() -> RewriteRule {
        RewriteRule {
            name: "add-id".into(),
            lhs: Term::Apply(
                Box::new(Term::Var(2)),
                vec![Term::Number(Value::Nat(0)), Term::Var(100)],
            ),
            rhs: Term::Var(100),
        }
    }

    #[test]
    fn novel_root_event_serializes() {
        let e = MapEvent::NovelRoot {
            seed: 1,
            phase_index: 0,
            root: TermRef::from_bytes(b"r"),
            library_size: 5,
        };
        let s = map_event_to_sexp(&e);
        let rendered = format!("{}", s);
        assert!(rendered.contains("map-event"));
        assert!(rendered.contains(":kind"));
        assert!(rendered.contains("novel-root"));
    }

    #[test]
    fn core_grew_event_includes_rule_name() {
        let e = MapEvent::CoreGrew {
            prev_core_size: 0,
            new_core_size: 1,
            added_rule: add_rule(),
        };
        let s = map_event_to_sexp(&e);
        let rendered = format!("{}", s);
        assert!(rendered.contains("core-grew"));
        assert!(rendered.contains("add-id"));
    }

    #[test]
    fn benchmark_scored_nan_delta_renders_as_symbol() {
        let e = MapEvent::BenchmarkScored {
            solved_count: 5,
            total: 10,
            solved_fraction: 0.5,
            delta_from_prior: f64::NAN,
        };
        let s = map_event_to_sexp(&e);
        let rendered = format!("{}", s);
        assert!(rendered.contains("benchmark-scored"));
        assert!(rendered.contains("nan"));
    }

    #[test]
    fn trainer_snapshot_sexp_includes_all_fields() {
        let t = StreamingPolicyTrainer::new(0.1);
        t.on_event(&MapEvent::RuleCertified {
            rule: add_rule(),
            evidence_samples: 96,
        });
        let s = trainer_snapshot_to_sexp(&t);
        let rendered = format!("{}", s);
        assert!(rendered.contains("trainer-snapshot"));
        assert!(rendered.contains(":trained-steps"));
        assert!(rendered.contains(":weights"));
        assert!(rendered.contains(":fisher"));
        assert!(rendered.contains(":phantom"));
        assert!(rendered.contains(":has-anchor"));
        assert!(rendered.contains(":ewc-lambda"));
        assert!(rendered.contains(":benchmark-history"));
    }

    #[test]
    fn plasticity_report_sexp_captures_tick_structure() {
        let hub = EventHub::new();
        let trainer = Rc::new(StreamingPolicyTrainer::new(0.1));
        hub.subscribe(trainer.clone());

        // Publish a few events so the trainer has some state.
        for _ in 0..5 {
            hub.publish(&MapEvent::RuleCertified {
                rule: add_rule(),
                evidence_samples: 96,
            });
        }

        let controller = PlasticityController::new();
        controller.register(trainer.clone() as Rc<dyn Plastic>);
        let report = controller.tick();

        let s = plasticity_report_to_sexp(&report);
        let rendered = format!("{}", s);
        assert!(rendered.contains("plasticity-report"));
        assert!(rendered.contains(":ticks"));
        assert!(rendered.contains("component"));
        assert!(rendered.contains("streaming-policy-trainer"));
    }

    #[test]
    fn full_event_hub_pipeline_emits_lisp_observable_stream() {
        // A Lisp program observes the hub by converting every
        // event to Sexp as it flies by. This test proves the
        // pattern works end-to-end: events → Sexp list.
        let hub = EventHub::new();
        let events_seen: Rc<std::cell::RefCell<Vec<Sexp>>> =
            Rc::new(std::cell::RefCell::new(Vec::new()));

        struct LispObserver {
            out: Rc<std::cell::RefCell<Vec<Sexp>>>,
        }
        impl MapEventConsumer for LispObserver {
            fn on_event(&self, event: &MapEvent) {
                self.out.borrow_mut().push(map_event_to_sexp(event));
            }
        }

        hub.subscribe(Rc::new(LispObserver {
            out: events_seen.clone(),
        }));

        let events = [
            MapEvent::NovelRoot {
                seed: 1,
                phase_index: 0,
                root: TermRef::from_bytes(b"r"),
                library_size: 0,
            },
            MapEvent::CoreGrew {
                prev_core_size: 0,
                new_core_size: 1,
                added_rule: add_rule(),
            },
            MapEvent::BenchmarkScored {
                solved_count: 8,
                total: 10,
                solved_fraction: 0.8,
                delta_from_prior: 0.1,
            },
        ];
        for e in &events {
            hub.publish(e);
        }
        let captured = events_seen.borrow();
        assert_eq!(captured.len(), 3);
        // Each captured Sexp is a map-event form.
        for (i, s) in captured.iter().enumerate() {
            let rendered = format!("{}", s);
            assert!(
                rendered.starts_with("(map-event"),
                "event {i} starts with (map-event, got: {rendered}"
            );
        }
    }
}
