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

use mathscape_core::eval::RewriteRule;
use mathscape_core::hash::TermRef;
use mathscape_core::mathscape_map::MapEvent;
use mathscape_core::plasticity::PlasticityReport;
use mathscape_core::streaming_policy::StreamingPolicyTrainer;
use mathscape_core::term::Term;
use tatara_lisp::ast::{Atom, Sexp};

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

/// Parse a `MapEvent` from its Sexp form. Returns `None` if the
/// form is malformed. Lisp publishers use this to inject events
/// into the hub:
///
/// ```text
/// (let ((e (map-event :kind core-grew :prev 0 :new 1
///                     :rule-name "add-id")))
///   (hub-publish hub e))
/// ```
///
/// Round-trip: `map_event_from_sexp(map_event_to_sexp(e)) ≈ e`
/// for all variants except those carrying `TermRef` (NovelRoot,
/// RootMutated) and full `RewriteRule` content (CoreGrew,
/// RuleCertified, RuleRejectedAtCertification) — these round-trip
/// by NAME/identity only. For full round-trip, use bincode (every
/// MapEvent derives `Serialize`/`Deserialize`).
#[must_use]
pub fn map_event_from_sexp(sexp: &Sexp) -> Option<MapEvent> {
    let items = match sexp {
        Sexp::List(items) if !items.is_empty() => items,
        _ => return None,
    };
    match &items[0] {
        Sexp::Atom(Atom::Symbol(s)) if s == "map-event" => {}
        _ => return None,
    }
    let fields = parse_keyword_args(&items[1..])?;
    let kind = fields.get("kind").and_then(symbol_val)?;

    match kind.as_str() {
        "novel-root" => {
            let seed = int_of(&fields, "seed")?;
            let phase_index = int_of(&fields, "phase-index")?;
            let library_size = int_of(&fields, "library-size")?;
            Some(MapEvent::NovelRoot {
                seed: seed as u64,
                phase_index: phase_index as usize,
                // Round-trip approximation: NovelRoot's real
                // `root: TermRef` is content-hashed; reconstructing
                // it requires the original bytes. We use a
                // placeholder hash derived from the seed+phase
                // tuple for Lisp-round-trip uses; full-fidelity
                // persistence should use bincode.
                root: TermRef::from_bytes(
                    format!("lisp-{seed}-{phase_index}").as_bytes(),
                ),
                library_size: library_size as usize,
            })
        }
        "root-mutated" => {
            let seed = int_of(&fields, "seed")?;
            let from_phase = int_of(&fields, "from")?;
            let to_phase = int_of(&fields, "to")?;
            let size_delta = int_of(&fields, "size-delta")?;
            Some(MapEvent::RootMutated {
                seed: seed as u64,
                from_phase: from_phase as usize,
                to_phase: to_phase as usize,
                prev_root: TermRef::from_bytes(
                    format!("lisp-prev-{seed}").as_bytes(),
                ),
                next_root: TermRef::from_bytes(
                    format!("lisp-next-{seed}").as_bytes(),
                ),
                size_delta: size_delta as i64,
            })
        }
        "core-grew" => {
            let prev = int_of(&fields, "prev")?;
            let new = int_of(&fields, "new")?;
            let rule_name = string_of(&fields, "rule-name")?;
            Some(MapEvent::CoreGrew {
                prev_core_size: prev as usize,
                new_core_size: new as usize,
                // Named placeholder rule. Lisp-reconstructed events
                // carry the name but not the full LHS/RHS term
                // structure — that's reconstructed via the rule
                // registry elsewhere.
                added_rule: RewriteRule {
                    name: rule_name,
                    lhs: Term::Var(0),
                    rhs: Term::Var(0),
                },
            })
        }
        "staleness-crossed" => {
            let seed = int_of(&fields, "seed")?;
            let phase_index = int_of(&fields, "phase-index")?;
            let threshold = float_of(&fields, "threshold")?;
            let observed = float_of(&fields, "observed")?;
            Some(MapEvent::StalenessCrossed {
                seed: seed as u64,
                phase_index: phase_index as usize,
                threshold,
                observed,
            })
        }
        "rule-certified" => {
            let rule_name = string_of(&fields, "rule-name")?;
            let evidence = int_of(&fields, "evidence")?;
            Some(MapEvent::RuleCertified {
                rule: RewriteRule {
                    name: rule_name,
                    lhs: Term::Var(0),
                    rhs: Term::Var(0),
                },
                evidence_samples: evidence as usize,
            })
        }
        "rule-rejected-at-certification" => {
            let rule_name = string_of(&fields, "rule-name")?;
            let reason = string_of(&fields, "reason")?;
            Some(MapEvent::RuleRejectedAtCertification {
                rule: RewriteRule {
                    name: rule_name,
                    lhs: Term::Var(0),
                    rhs: Term::Var(0),
                },
                reason,
            })
        }
        "benchmark-scored" => {
            let solved = int_of(&fields, "solved")?;
            let total = int_of(&fields, "total")?;
            let fraction = float_of(&fields, "fraction")?;
            // Delta can be :delta NaN-symbol OR a float.
            let delta = fields.get("delta").and_then(|v| match v {
                Sexp::Atom(Atom::Symbol(s)) if s == "nan" => Some(f64::NAN),
                Sexp::Atom(Atom::Float(f)) => Some(*f),
                Sexp::Atom(Atom::Int(n)) => Some(*n as f64),
                _ => None,
            })?;
            Some(MapEvent::BenchmarkScored {
                solved_count: solved as usize,
                total: total as usize,
                solved_fraction: fraction,
                delta_from_prior: delta,
            })
        }
        _ => None,
    }
}

fn parse_keyword_args(
    items: &[Sexp],
) -> Option<std::collections::HashMap<String, Sexp>> {
    let mut map = std::collections::HashMap::new();
    let mut i = 0;
    while i + 1 < items.len() {
        let key = match &items[i] {
            Sexp::Atom(Atom::Keyword(k)) => k.clone(),
            _ => return None,
        };
        map.insert(key, items[i + 1].clone());
        i += 2;
    }
    Some(map)
}

fn symbol_val(sexp: &Sexp) -> Option<String> {
    match sexp {
        Sexp::Atom(Atom::Symbol(s)) => Some(s.clone()),
        _ => None,
    }
}

fn int_of(
    m: &std::collections::HashMap<String, Sexp>,
    key: &str,
) -> Option<i64> {
    match m.get(key)? {
        Sexp::Atom(Atom::Int(n)) => Some(*n),
        _ => None,
    }
}

fn float_of(
    m: &std::collections::HashMap<String, Sexp>,
    key: &str,
) -> Option<f64> {
    match m.get(key)? {
        Sexp::Atom(Atom::Float(f)) => Some(*f),
        Sexp::Atom(Atom::Int(n)) => Some(*n as f64),
        _ => None,
    }
}

fn string_of(
    m: &std::collections::HashMap<String, Sexp>,
    key: &str,
) -> Option<String> {
    match m.get(key)? {
        Sexp::Atom(Atom::Str(s)) => Some(s.clone()),
        _ => None,
    }
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

    // ── Round-trip tests (map_event_from_sexp) ──────────────

    #[test]
    fn core_grew_round_trips_by_name() {
        let original = MapEvent::CoreGrew {
            prev_core_size: 2,
            new_core_size: 3,
            added_rule: add_rule(),
        };
        let s = map_event_to_sexp(&original);
        let back = map_event_from_sexp(&s).expect("parse succeeds");
        match back {
            MapEvent::CoreGrew {
                prev_core_size,
                new_core_size,
                added_rule,
            } => {
                assert_eq!(prev_core_size, 2);
                assert_eq!(new_core_size, 3);
                assert_eq!(added_rule.name, "add-id");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn benchmark_scored_round_trips_with_all_fields() {
        let original = MapEvent::BenchmarkScored {
            solved_count: 8,
            total: 10,
            solved_fraction: 0.8,
            delta_from_prior: 0.1,
        };
        let s = map_event_to_sexp(&original);
        let back = map_event_from_sexp(&s).expect("parse succeeds");
        match back {
            MapEvent::BenchmarkScored {
                solved_count,
                total,
                solved_fraction,
                delta_from_prior,
            } => {
                assert_eq!(solved_count, 8);
                assert_eq!(total, 10);
                assert_eq!(solved_fraction, 0.8);
                assert_eq!(delta_from_prior, 0.1);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn benchmark_scored_round_trips_with_nan_delta() {
        let original = MapEvent::BenchmarkScored {
            solved_count: 5,
            total: 10,
            solved_fraction: 0.5,
            delta_from_prior: f64::NAN,
        };
        let s = map_event_to_sexp(&original);
        let back = map_event_from_sexp(&s).expect("parse succeeds");
        match back {
            MapEvent::BenchmarkScored {
                delta_from_prior, ..
            } => {
                assert!(delta_from_prior.is_nan());
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn staleness_crossed_round_trips_full_fidelity() {
        let original = MapEvent::StalenessCrossed {
            seed: 42,
            phase_index: 7,
            threshold: 0.6,
            observed: 0.95,
        };
        let s = map_event_to_sexp(&original);
        let back = map_event_from_sexp(&s).expect("parse succeeds");
        match back {
            MapEvent::StalenessCrossed {
                seed,
                phase_index,
                threshold,
                observed,
            } => {
                assert_eq!(seed, 42);
                assert_eq!(phase_index, 7);
                assert_eq!(threshold, 0.6);
                assert_eq!(observed, 0.95);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn rule_certified_and_rejected_round_trip_by_name() {
        let cert = MapEvent::RuleCertified {
            rule: add_rule(),
            evidence_samples: 100,
        };
        let s = map_event_to_sexp(&cert);
        let back = map_event_from_sexp(&s).expect("parse succeeds");
        match back {
            MapEvent::RuleCertified {
                rule,
                evidence_samples,
            } => {
                assert_eq!(rule.name, "add-id");
                assert_eq!(evidence_samples, 100);
            }
            _ => panic!("wrong variant"),
        }

        let rej = MapEvent::RuleRejectedAtCertification {
            rule: add_rule(),
            reason: "not enough support".into(),
        };
        let s2 = map_event_to_sexp(&rej);
        let back2 = map_event_from_sexp(&s2).expect("parse succeeds");
        match back2 {
            MapEvent::RuleRejectedAtCertification { rule, reason } => {
                assert_eq!(rule.name, "add-id");
                assert_eq!(reason, "not enough support");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn malformed_sexp_rejected() {
        // Not a list.
        let not_list = Sexp::symbol("foo");
        assert!(map_event_from_sexp(&not_list).is_none());

        // Wrong head symbol.
        let wrong_head = Sexp::List(vec![
            Sexp::symbol("not-map-event"),
            Sexp::keyword("kind"),
            Sexp::symbol("core-grew"),
        ]);
        assert!(map_event_from_sexp(&wrong_head).is_none());

        // Unknown kind.
        let unknown = Sexp::List(vec![
            Sexp::symbol("map-event"),
            Sexp::keyword("kind"),
            Sexp::symbol("unknown-kind"),
        ]);
        assert!(map_event_from_sexp(&unknown).is_none());

        // Missing required field (core-grew without :rule-name).
        let partial = Sexp::List(vec![
            Sexp::symbol("map-event"),
            Sexp::keyword("kind"),
            Sexp::symbol("core-grew"),
            Sexp::keyword("prev"),
            Sexp::int(0),
            Sexp::keyword("new"),
            Sexp::int(1),
        ]);
        assert!(map_event_from_sexp(&partial).is_none());
    }

    #[test]
    fn lisp_publisher_injects_events_into_hub() {
        // End-to-end demo: a "Lisp publisher" constructs a Sexp,
        // parses it back to MapEvent, and publishes to the hub.
        // The trainer (subscribed to the hub) sees the event and
        // updates. This proves the write-side round-trip works
        // for the full pipeline.
        use mathscape_core::mathscape_map::EventHub;
        use mathscape_core::streaming_policy::StreamingPolicyTrainer;

        let hub = EventHub::new();
        let trainer = Rc::new(StreamingPolicyTrainer::new(0.1));
        hub.subscribe(trainer.clone());

        // "Lisp-authored" event as Sexp.
        let sexp = Sexp::List(vec![
            Sexp::symbol("map-event"),
            Sexp::keyword("kind"),
            Sexp::symbol("core-grew"),
            Sexp::keyword("prev"),
            Sexp::int(0),
            Sexp::keyword("new"),
            Sexp::int(1),
            Sexp::keyword("rule-name"),
            Sexp::string("lisp-authored-rule"),
        ]);
        let event = map_event_from_sexp(&sexp).expect("parses");
        hub.publish(&event);
        // Trainer saw the event.
        assert_eq!(trainer.events_seen(), 1);
        assert!(trainer.updates_applied() >= 1);
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
