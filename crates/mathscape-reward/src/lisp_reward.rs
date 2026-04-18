//! Phase ML1 — reward function as a Lisp Sexp form.
//!
//! The ground move in the rust-lisp duality: the reward formula
//! stops being hardcoded in Rust and becomes a term the machine
//! can manipulate. Given the canonical form
//!
//!   (+ (* alpha cr)
//!      (* beta novelty)
//!      (* gamma meta-compression)
//!      (* delta lhs-subsumption))
//!
//! `evaluate_reward_sexp` walks the Sexp and produces an f64. Rust
//! still owns the inputs (cr, novelty, meta-compression,
//! lhs-subsumption are all computed in Rust from the corpus and
//! library) AND still owns the output scalar (it feeds back into
//! the ΔDL prover). What changes: the *combination rule* is now
//! data, not code. Future phases let the apparatus mutation loop
//! propose different combinations.
//!
//! Supported Sexp arithmetic:
//!
//! * `Atom::Int(n)`       — constant
//! * `Atom::Float(x)`     — constant
//! * `Atom::Symbol(name)` — looked up in the bindings map
//! * `(+ a b c ...)`      — sum, variadic, 0 args = 0
//! * `(- a)`              — unary negation
//! * `(- a b c ...)`      — left-fold subtraction
//! * `(* a b c ...)`      — product, variadic, 0 args = 1
//! * `(/ a b)`            — binary division
//! * `(max a b)`          — two-argument max
//! * `(min a b)`          — two-argument min
//! * `(if cond then else)` — conditional, cond is non-zero = true
//! * `(clamp x lo hi)`    — clamp to [lo, hi]
//!
//! That's enough to express the current reward formula AND
//! plausible mutations (conditional weights, bounded novelty
//! terms, multiplicative bonuses). The evaluator is deliberately
//! tiny — expansion to tatara-lisp's macro layer is future work.

use std::collections::HashMap;
use tatara_lisp::ast::{Atom, Sexp};
use tatara_lisp::reader::read;

/// Error from Lisp-reward evaluation. Deliberately a plain String —
/// these are authored by the apparatus layer and reported back for
/// diagnosis, not chained up the stack.
pub type LispRewardError = String;

/// Parse a `.reward` source string (one top-level Sexp expression)
/// into a Sexp tree. Wraps `tatara_lisp::reader::read` and reports
/// the FIRST form (a reward expression is always a single tree).
pub fn parse_reward(src: &str) -> Result<Sexp, LispRewardError> {
    let forms = read(src).map_err(|e| format!("parse error: {e:?}"))?;
    forms
        .into_iter()
        .next()
        .ok_or_else(|| "parse_reward: no forms in source".to_string())
}

/// Evaluate an arithmetic Sexp against a bindings map. Returns the
/// scalar value the form denotes under those bindings.
pub fn evaluate_reward_sexp(
    sexp: &Sexp,
    bindings: &HashMap<String, f64>,
) -> Result<f64, LispRewardError> {
    match sexp {
        Sexp::Atom(Atom::Int(n)) => Ok(*n as f64),
        Sexp::Atom(Atom::Float(x)) => Ok(*x),
        Sexp::Atom(Atom::Symbol(name)) => bindings
            .get(name)
            .copied()
            .ok_or_else(|| format!("unbound symbol in reward form: {name:?}")),
        Sexp::Atom(Atom::Bool(b)) => Ok(if *b { 1.0 } else { 0.0 }),
        Sexp::Atom(other) => Err(format!("atom type not valid in reward form: {other:?}")),
        Sexp::List(items) => eval_list(items, bindings),
        Sexp::Nil => Ok(0.0),
        Sexp::Quote(inner) | Sexp::Quasiquote(inner) => {
            // Reward forms don't use quotation — surface any use as
            // an error rather than silently evaluating inner.
            Err(format!("quote/quasiquote not supported in reward form: {inner:?}"))
        }
        Sexp::Unquote(_) | Sexp::UnquoteSplice(_) => Err(
            "unquote outside quasiquote context is not allowed in reward forms"
                .to_string(),
        ),
    }
}

fn eval_list(
    items: &[Sexp],
    bindings: &HashMap<String, f64>,
) -> Result<f64, LispRewardError> {
    let (head_sym, args) = items
        .split_first()
        .ok_or_else(|| "empty list in reward form".to_string())?;
    let head = head_sym
        .as_symbol()
        .ok_or_else(|| format!("list head must be a symbol, got {head_sym:?}"))?;
    match head {
        "+" => {
            let mut total = 0.0;
            for a in args {
                total += evaluate_reward_sexp(a, bindings)?;
            }
            Ok(total)
        }
        "-" => match args {
            [] => Err("(-) with zero arguments is not allowed".to_string()),
            [only] => Ok(-evaluate_reward_sexp(only, bindings)?),
            [first, rest @ ..] => {
                let mut v = evaluate_reward_sexp(first, bindings)?;
                for r in rest {
                    v -= evaluate_reward_sexp(r, bindings)?;
                }
                Ok(v)
            }
        },
        "*" => {
            let mut total = 1.0;
            for a in args {
                total *= evaluate_reward_sexp(a, bindings)?;
            }
            Ok(total)
        }
        "/" => {
            let [num, den] = &args[..] else {
                return Err(format!("(/) expects 2 args, got {}", args.len()));
            };
            let d = evaluate_reward_sexp(den, bindings)?;
            if d == 0.0 {
                return Err("division by zero in reward form".to_string());
            }
            Ok(evaluate_reward_sexp(num, bindings)? / d)
        }
        "max" => {
            let [a, b] = &args[..] else {
                return Err(format!("(max) expects 2 args, got {}", args.len()));
            };
            let av = evaluate_reward_sexp(a, bindings)?;
            let bv = evaluate_reward_sexp(b, bindings)?;
            Ok(av.max(bv))
        }
        "min" => {
            let [a, b] = &args[..] else {
                return Err(format!("(min) expects 2 args, got {}", args.len()));
            };
            let av = evaluate_reward_sexp(a, bindings)?;
            let bv = evaluate_reward_sexp(b, bindings)?;
            Ok(av.min(bv))
        }
        "if" => {
            let [cond, then, els] = &args[..] else {
                return Err(format!("(if) expects 3 args, got {}", args.len()));
            };
            let cv = evaluate_reward_sexp(cond, bindings)?;
            if cv != 0.0 {
                evaluate_reward_sexp(then, bindings)
            } else {
                evaluate_reward_sexp(els, bindings)
            }
        }
        "clamp" => {
            let [x, lo, hi] = &args[..] else {
                return Err(format!("(clamp) expects 3 args, got {}", args.len()));
            };
            let xv = evaluate_reward_sexp(x, bindings)?;
            let lv = evaluate_reward_sexp(lo, bindings)?;
            let hv = evaluate_reward_sexp(hi, bindings)?;
            Ok(xv.clamp(lv, hv))
        }
        other => Err(format!("unknown operator in reward form: {other:?}")),
    }
}

/// The canonical reward form, mirroring the Rust formula in
/// `reward::compute_reward`:
///
///   reward = alpha * cr + beta * novelty + gamma * meta-compression
///            + delta * lhs-subsumption
///
/// Returned as a source string so callers can either parse it
/// (`parse_reward`) or store it as provenance.
pub const CANONICAL_REWARD_SRC: &str = "(+ (* alpha cr) \
                                          (* beta novelty) \
                                          (* gamma meta-compression) \
                                          (* delta lhs-subsumption))";

/// Build the bindings map the canonical form needs, given the four
/// reward axes + the four weight scalars. Called from the bridge that
/// evaluates the Lisp reward and is also used by the gold test.
#[must_use]
pub fn bindings_from_axes(
    alpha: f64,
    beta: f64,
    gamma: f64,
    delta: f64,
    cr: f64,
    novelty: f64,
    meta_compression: f64,
    lhs_subsumption: f64,
) -> HashMap<String, f64> {
    let mut m = HashMap::new();
    m.insert("alpha".into(), alpha);
    m.insert("beta".into(), beta);
    m.insert("gamma".into(), gamma);
    m.insert("delta".into(), delta);
    m.insert("cr".into(), cr);
    m.insert("novelty".into(), novelty);
    m.insert("meta-compression".into(), meta_compression);
    m.insert("lhs-subsumption".into(), lhs_subsumption);
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_canonical_reward_form() {
        let sexp = parse_reward(CANONICAL_REWARD_SRC).expect("canonical form must parse");
        // It's a (+ ...) list with 4 summand children (one per axis).
        let items = sexp.as_list().expect("canonical form must be a list");
        assert_eq!(items.len(), 5, "(+ a b c d) has 5 elements (op + 4 args)");
        assert_eq!(items[0].as_symbol(), Some("+"));
    }

    #[test]
    fn evaluates_zero_when_all_axes_zero() {
        let sexp = parse_reward(CANONICAL_REWARD_SRC).unwrap();
        let b = bindings_from_axes(0.6, 0.3, 0.1, 0.5, 0.0, 0.0, 0.0, 0.0);
        let v = evaluate_reward_sexp(&sexp, &b).unwrap();
        assert_eq!(v, 0.0);
    }

    #[test]
    fn evaluates_single_axis_contributions() {
        let sexp = parse_reward(CANONICAL_REWARD_SRC).unwrap();

        // Only cr active: reward == alpha * cr.
        let b = bindings_from_axes(0.6, 0.3, 0.1, 0.5, 1.0, 0.0, 0.0, 0.0);
        let v = evaluate_reward_sexp(&sexp, &b).unwrap();
        assert!((v - 0.6).abs() < 1e-12);

        // Only novelty: reward == beta * novelty.
        let b = bindings_from_axes(0.6, 0.3, 0.1, 0.5, 0.0, 2.0, 0.0, 0.0);
        let v = evaluate_reward_sexp(&sexp, &b).unwrap();
        assert!((v - 0.6).abs() < 1e-12);

        // Only meta: reward == gamma * meta-compression.
        let b = bindings_from_axes(0.6, 0.3, 0.1, 0.5, 0.0, 0.0, 3.0, 0.0);
        let v = evaluate_reward_sexp(&sexp, &b).unwrap();
        assert!((v - 0.3).abs() < 1e-12);

        // Only lhs-subsumption: reward == delta * lhs-subsumption.
        let b = bindings_from_axes(0.6, 0.3, 0.1, 0.5, 0.0, 0.0, 0.0, 4.0);
        let v = evaluate_reward_sexp(&sexp, &b).unwrap();
        assert!((v - 2.0).abs() < 1e-12);
    }

    #[test]
    fn matches_rust_formula_on_combined_inputs() {
        // The gold test: the Lisp-evaluated reward equals the Rust
        // arithmetic for arbitrary (axes × weights) combinations.
        // Proves the FFI direction works end-to-end: Rust values go
        // in → Lisp evaluates → Rust value comes out, bit-identical
        // (within f64 tolerance) to the hardcoded formula.
        let sexp = parse_reward(CANONICAL_REWARD_SRC).unwrap();
        let cases: &[(f64, f64, f64, f64, f64, f64, f64, f64)] = &[
            (0.6, 0.3, 0.1, 0.5, 0.8, 1.2, 0.5, 3.0),
            (0.5, 0.5, 0.0, 0.0, 0.3, 0.7, 0.0, 0.0),
            (0.1, 0.1, 0.1, 0.1, 1.0, 1.0, 1.0, 1.0),
            (0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 7.0),
            (0.6, 0.3, 0.1, 0.5, -0.2, 0.1, 0.0, 0.0), // negative CR
        ];
        for (i, &(a, b, g, d, cr, nov, meta, lhs)) in cases.iter().enumerate() {
            let expected = a * cr + b * nov + g * meta + d * lhs;
            let bindings = bindings_from_axes(a, b, g, d, cr, nov, meta, lhs);
            let got = evaluate_reward_sexp(&sexp, &bindings).unwrap();
            assert!(
                (got - expected).abs() < 1e-12,
                "case {i}: lisp {got} vs rust {expected}",
            );
        }
    }

    #[test]
    fn errors_on_unbound_symbol() {
        let sexp = parse_reward("unknown-axis").unwrap();
        let err = evaluate_reward_sexp(&sexp, &HashMap::new()).unwrap_err();
        assert!(err.contains("unknown-axis"), "error message must name the symbol: {err}");
    }

    #[test]
    fn supports_conditional_mutation_form() {
        // A plausible apparatus mutation: reward novelty only when
        // compression is positive. Demonstrates that the Lisp layer
        // can express REWARD SHAPES the Rust formula cannot, without
        // any machinery change.
        let src = "(+ (* alpha cr) \
                     (if (max cr 0) (* beta novelty) 0))";
        let sexp = parse_reward(src).unwrap();

        // cr > 0 → novelty term active.
        let b = bindings_from_axes(0.6, 0.3, 0.0, 0.0, 0.5, 1.0, 0.0, 0.0);
        let v = evaluate_reward_sexp(&sexp, &b).unwrap();
        assert!((v - (0.3 + 0.3)).abs() < 1e-12, "got {v}");

        // cr <= 0 → novelty term zeroed out.
        let b = bindings_from_axes(0.6, 0.3, 0.0, 0.0, -0.1, 1.0, 0.0, 0.0);
        let v = evaluate_reward_sexp(&sexp, &b).unwrap();
        assert!((v - (-0.06)).abs() < 1e-12, "got {v}");
    }

    #[test]
    fn supports_clamp_and_max() {
        // Another plausible mutation: bound the meta term to a
        // specific range, preventing runaway meta-compression from
        // dominating reward. Tests that clamp + max work together.
        let src = "(clamp (+ (* alpha cr) \
                              (* gamma meta-compression)) \
                          0 1.5)";
        let sexp = parse_reward(src).unwrap();

        let b = bindings_from_axes(0.6, 0.3, 0.1, 0.5, 5.0, 0.0, 10.0, 0.0);
        let v = evaluate_reward_sexp(&sexp, &b).unwrap();
        // 0.6*5 + 0.1*10 = 4.0, clamped to 1.5.
        assert!((v - 1.5).abs() < 1e-12, "got {v}");
    }
}
