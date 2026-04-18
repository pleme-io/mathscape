//! R10.1 — Lisp bridge for the policy model.
//!
//! # The model in Lisp
//!
//! `LinearPolicy` is a Rust struct (trainable model, bincode-
//! serializable). This module lifts it to a `tatara_lisp::Sexp` —
//! the same substrate the mechanism config and mutations already
//! live in (see `mechanism.rs`'s M1/M2 forms).
//!
//! Once the policy is a Sexp, three things become possible:
//!
//! 1. **Persistence as Lisp source code.** A policy can be written
//!    to a `.lisp` file, inspected by humans, version-controlled
//!    alongside code, and re-loaded without ever touching Rust.
//!
//! 2. **Discovery-level mutation.** The policy is just another
//!    Sexp. ML5's mutation operators — the ones that mutate
//!    mechanism configs — can be extended to mutate policies
//!    too. A policy becomes a subject of the same evolutionary
//!    pressure as the rules it scores.
//!
//! 3. **Self-production.** A Lisp program can generate a policy
//!    Sexp. A generator that creates policy Sexps IS the "system
//!    that produces the model." Train → emit Sexp → load into
//!    next generation → train → emit. Fixed point: a policy that
//!    generates its own successor.
//!
//! # Sexp form
//!
//! ```text
//! (policy
//!   :generation        N
//!   :trained-steps     M
//!   :bias              0.0
//!   :weights           (w0 w1 w2 w3 w4 w5 w6 w7 w8))
//! ```
//!
//! The `weights` list has width `LibraryFeatures::WIDTH`. The
//! order matches `LibraryFeatures::as_vector()` exactly — changing
//! feature order requires updating the Sexp form AND a migration
//! for existing persisted policies.

use mathscape_core::policy::LinearPolicy;
use mathscape_core::trajectory::LibraryFeatures;
use tatara_lisp::ast::{Atom, Sexp};

/// Convert a `LinearPolicy` to its canonical Sexp form. Lossless
/// — a round-trip via `policy_from_sexp` reconstructs the same
/// model.
#[must_use]
pub fn policy_to_sexp(policy: &LinearPolicy) -> Sexp {
    let mut items: Vec<Sexp> = vec![Sexp::symbol("policy")];
    items.push(Sexp::keyword("generation"));
    items.push(Sexp::int(policy.generation as i64));
    items.push(Sexp::keyword("trained-steps"));
    items.push(Sexp::int(policy.trained_steps as i64));
    items.push(Sexp::keyword("bias"));
    items.push(Sexp::float(policy.bias));
    items.push(Sexp::keyword("weights"));
    items.push(Sexp::List(
        policy.weights.iter().map(|w| Sexp::float(*w)).collect(),
    ));
    Sexp::List(items)
}

/// Parse a `LinearPolicy` from Sexp form. Returns `None` if the
/// form is malformed or the weight list has the wrong width.
#[must_use]
pub fn policy_from_sexp(sexp: &Sexp) -> Option<LinearPolicy> {
    let items = match sexp {
        Sexp::List(items) if !items.is_empty() => items,
        _ => return None,
    };
    // Header: (policy ...)
    match &items[0] {
        Sexp::Atom(Atom::Symbol(s)) if s == "policy" => {}
        _ => return None,
    }

    let mut generation: Option<u64> = None;
    let mut trained_steps: Option<u64> = None;
    let mut bias: Option<f64> = None;
    let mut weights: Option<[f64; LibraryFeatures::WIDTH]> = None;

    let mut i = 1;
    while i + 1 < items.len() {
        let key = match &items[i] {
            Sexp::Atom(Atom::Keyword(k)) => k,
            _ => return None,
        };
        let val = &items[i + 1];
        match key.as_str() {
            "generation" => {
                let n = int_val(val)?;
                if n < 0 {
                    return None;
                }
                generation = Some(n as u64);
            }
            "trained-steps" => {
                let n = int_val(val)?;
                if n < 0 {
                    return None;
                }
                trained_steps = Some(n as u64);
            }
            "bias" => {
                bias = Some(float_val(val)?);
            }
            "weights" => {
                let list = match val {
                    Sexp::List(xs) => xs,
                    _ => return None,
                };
                if list.len() != LibraryFeatures::WIDTH {
                    return None;
                }
                let mut arr = [0.0; LibraryFeatures::WIDTH];
                for (j, item) in list.iter().enumerate() {
                    arr[j] = float_val(item)?;
                }
                weights = Some(arr);
            }
            // Unknown keyword: ignore (forward-compatibility with
            // policies that add new fields in future).
            _ => {}
        }
        i += 2;
    }

    Some(LinearPolicy {
        weights: weights?,
        bias: bias?,
        trained_steps: trained_steps?,
        generation: generation?,
    })
}

fn int_val(sexp: &Sexp) -> Option<i64> {
    match sexp {
        Sexp::Atom(Atom::Int(n)) => Some(*n),
        _ => None,
    }
}

fn float_val(sexp: &Sexp) -> Option<f64> {
    match sexp {
        Sexp::Atom(Atom::Float(f)) => Some(*f),
        // Tolerate Int where a Float was expected — common when a
        // bias is exactly 0 and the writer used an int literal.
        Sexp::Atom(Atom::Int(n)) => Some(*n as f64),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_policy_roundtrips_through_sexp() {
        // A policy serialized to Sexp and parsed back must be
        // bit-identical. This is the foundation of "the model is
        // in Lisp" — if the roundtrip loses anything, the Lisp
        // form is NOT a faithful representation.
        let p = LinearPolicy::new();
        let sexp = policy_to_sexp(&p);
        let back = policy_from_sexp(&sexp).expect("valid form parses");
        assert_eq!(p, back);
    }

    #[test]
    fn tensor_seeking_prior_roundtrips_through_sexp() {
        let p = LinearPolicy::tensor_seeking_prior();
        let sexp = policy_to_sexp(&p);
        let back = policy_from_sexp(&sexp).expect("valid form parses");
        assert_eq!(p, back);
        // Non-zero weights must survive the roundtrip.
        assert!(back.weights[4] > 0.0, "tensor_density weight must persist");
    }

    #[test]
    fn trained_policy_roundtrips_through_sexp() {
        use mathscape_core::trajectory::{ActionKind, Trajectory, TrajectoryStep};

        let mut p = LinearPolicy::new();
        let feat = LibraryFeatures {
            rule_count: 2,
            mean_lhs_size: 3.0,
            mean_rhs_size: 1.5,
            mean_compression: 2.0,
            tensor_density: 0.5,
            tensor_distributive_count: 1,
            tensor_meta_count: 0,
            distinct_heads: 2,
            max_rule_depth: 3,
        };
        let mut traj = Trajectory::new();
        traj.record(TrajectoryStep {
            epoch: 0,
            corpus_index: 0,
            pre_state: feat.clone(),
            action: ActionKind::Discover,
            accepted: true,
            delta_dl: 1.25,
        });
        traj.finalize(feat);
        p.train_from_trajectory(&traj, 0.1);

        let sexp = policy_to_sexp(&p);
        let back = policy_from_sexp(&sexp).expect("valid form parses");
        assert_eq!(p, back);
    }

    #[test]
    fn sexp_form_is_recognizable_as_policy() {
        let p = LinearPolicy::new();
        let sexp = policy_to_sexp(&p);
        // Should be a List starting with the symbol "policy".
        match &sexp {
            Sexp::List(items) => {
                assert!(!items.is_empty());
                match &items[0] {
                    Sexp::Atom(Atom::Symbol(s)) => assert_eq!(s, "policy"),
                    other => panic!("expected (policy ...), head is {other:?}"),
                }
            }
            other => panic!("expected List, got {other:?}"),
        }
    }

    #[test]
    fn malformed_sexp_returns_none() {
        // Various kinds of malformation all yield None — the
        // caller can treat them uniformly as "not a valid policy".
        let not_a_list = Sexp::int(42);
        assert!(policy_from_sexp(&not_a_list).is_none());

        let wrong_head = Sexp::List(vec![Sexp::symbol("not-policy")]);
        assert!(policy_from_sexp(&wrong_head).is_none());

        let missing_fields = Sexp::List(vec![Sexp::symbol("policy")]);
        assert!(policy_from_sexp(&missing_fields).is_none());

        // Wrong weight count.
        let bad_weights = Sexp::List(vec![
            Sexp::symbol("policy"),
            Sexp::keyword("generation"),
            Sexp::int(0),
            Sexp::keyword("trained-steps"),
            Sexp::int(0),
            Sexp::keyword("bias"),
            Sexp::float(0.0),
            Sexp::keyword("weights"),
            Sexp::List(vec![Sexp::float(0.0), Sexp::float(0.0)]), // only 2
        ]);
        assert!(policy_from_sexp(&bad_weights).is_none());
    }

    #[test]
    fn self_producing_loop_via_lisp() {
        // The essential self-producing invariant: a policy
        // emitted to Lisp, then re-absorbed, continues training
        // as if it had never left Rust. This is what makes the
        // "Lisp-resident model" real — not just a textual dump,
        // but a persistence format that round-trips through the
        // learning loop.
        use mathscape_core::trajectory::{ActionKind, Trajectory, TrajectoryStep};

        // Generation 1: train a policy.
        let mut gen1 = LinearPolicy::new();
        let feat = LibraryFeatures {
            rule_count: 1,
            mean_lhs_size: 2.0,
            mean_rhs_size: 1.0,
            mean_compression: 2.0,
            tensor_density: 1.0,
            tensor_distributive_count: 1,
            tensor_meta_count: 0,
            distinct_heads: 2,
            max_rule_depth: 2,
        };
        let mut traj1 = Trajectory::new();
        traj1.record(TrajectoryStep {
            epoch: 0,
            corpus_index: 0,
            pre_state: feat.clone(),
            action: ActionKind::Discover,
            accepted: true,
            delta_dl: 1.0,
        });
        traj1.finalize(feat.clone());
        gen1.train_from_trajectory(&traj1, 0.1);

        // Emit as Lisp.
        let lisp_form = policy_to_sexp(&gen1);

        // Simulate persistence: serialize Sexp to string, parse back.
        // (Skipping string roundtrip here since tatara-lisp's
        // printer/parser live in another crate; the AST-level
        // roundtrip via policy_from_sexp proves the data is
        // faithful regardless of textual form.)
        let mut gen2 = policy_from_sexp(&lisp_form).expect("lisp form parses");
        assert_eq!(gen1, gen2);

        // Gen-2 continues training — the learning accumulates
        // across the Lisp boundary.
        let mut traj2 = Trajectory::new();
        traj2.record(TrajectoryStep {
            epoch: 0,
            corpus_index: 0,
            pre_state: feat.clone(),
            action: ActionKind::Discover,
            accepted: true,
            delta_dl: 1.0,
        });
        traj2.finalize(feat.clone());
        gen2.train_from_trajectory(&traj2, 0.1);

        assert_eq!(gen2.generation, 2);
        assert!(gen2.trained_steps > gen1.trained_steps);
    }
}
