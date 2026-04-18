//! Expression evaluator — reduces terms using Peano arithmetic builtins
//! and user-defined library rewrite rules.

use crate::term::Term;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// R5: builtin operator ids come from the central registry in
// `crate::builtin`. Re-exported for backward compat with code
// and tests that used the old `BUILTIN_*` constants.
pub use crate::builtin::{ADD as BUILTIN_ADD, MUL as BUILTIN_MUL,
    SUCC as BUILTIN_SUCC, ZERO as BUILTIN_ZERO};

/// A rewrite rule: lhs pattern => rhs template.
/// Pattern variables (Var) in lhs are matched and substituted into rhs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RewriteRule {
    pub name: String,
    pub lhs: Term,
    pub rhs: Term,
}

/// Result of evaluation — either a reduced term or an error.
pub type EvalResult = Result<Term, EvalError>;

#[derive(Debug, Clone)]
pub enum EvalError {
    /// Evaluation exceeded the step limit (prevents infinite loops).
    StepLimitExceeded,
    /// Type error during evaluation (e.g., applying a number).
    TypeError(String),
}

impl std::fmt::Display for EvalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EvalError::StepLimitExceeded => write!(f, "step limit exceeded"),
            EvalError::TypeError(msg) => write!(f, "type error: {msg}"),
        }
    }
}

impl std::error::Error for EvalError {}

/// Evaluate a term to normal form using builtins and library rules.
/// `step_limit` prevents divergent evaluations.
pub fn eval(term: &Term, library: &[RewriteRule], step_limit: usize) -> EvalResult {
    let mut current = term.clone();
    let mut steps = 0;

    loop {
        let next = step(&current, library)?;
        if next == current || steps >= step_limit {
            if steps >= step_limit {
                return Err(EvalError::StepLimitExceeded);
            }
            return Ok(current);
        }
        current = next;
        steps += 1;
    }
}

/// One step of evaluation: try builtins first, then library rules.
fn step(term: &Term, library: &[RewriteRule]) -> EvalResult {
    match term {
        // Leaves are already in normal form
        Term::Point(_) | Term::Number(_) | Term::Var(_) | Term::Symbol(_, _) => {
            Ok(term.clone())
        }

        // Function abstraction: reduce body
        Term::Fn(params, body) => {
            let new_body = step(body, library)?;
            Ok(Term::Fn(params.clone(), Box::new(new_body)))
        }

        // Application: the heart of evaluation
        Term::Apply(func, args) => {
            // First, try to reduce the function
            let reduced_func = step(func, library)?;

            // Try built-in evaluation
            if let Some(result) = try_builtin(&reduced_func, args)? {
                return Ok(result);
            }

            // Try beta reduction (applying a Fn to args)
            if let Term::Fn(params, body) = &reduced_func {
                if args.len() == params.len() {
                    let mut result = *body.clone();
                    for (param, arg) in params.iter().zip(args.iter()) {
                        result = result.substitute(*param, arg);
                    }
                    return Ok(result);
                }
            }

            // Try library rewrite rules on the whole term
            for rule in library {
                if let Some(bindings) = pattern_match(&rule.lhs, term) {
                    let mut result = rule.rhs.clone();
                    for (var, val) in &bindings {
                        result = result.substitute(*var, val);
                    }
                    return Ok(result);
                }
            }

            // Reduce args
            let reduced_args: Vec<Term> = args
                .iter()
                .map(|a| step(a, library))
                .collect::<Result<_, _>>()?;

            Ok(Term::Apply(Box::new(reduced_func), reduced_args))
        }
    }
}

/// Try to evaluate using the builtin registry (R5).
///
/// Dispatches on `Term::Var(id)` — looks up the builtin by id and
/// calls its eval function. Returns None when the operator isn't a
/// builtin, or when args aren't reduced to a shape the builtin
/// accepts (e.g., still an Apply instead of a Number).
fn try_builtin(func: &Term, args: &[Term]) -> Result<Option<Term>, EvalError> {
    let Term::Var(id) = func else {
        return Ok(None);
    };
    let Some(builtin) = crate::builtin::lookup(*id) else {
        return Ok(None);
    };
    if args.len() != builtin.arity {
        return Ok(None);
    }
    Ok((builtin.eval)(args))
}

/// Classical subsumption: `subsumer.lhs` pattern-matches
/// `subsumed.lhs` — every term matched by subsumed is also matched
/// by subsumer. One-directional; the subsumer is strictly more
/// general (or equivalent, if both directions succeed).
///
/// Used across mathscape-core (reduction, promotion gate),
/// mathscape-compress (generator dedup, extract dedup), and
/// mathscape-reward (novelty scoring). Centralized here so all
/// consumers share the exact semantics.
#[must_use]
pub fn subsumes(subsumer: &Term, subsumed: &Term) -> bool {
    pattern_match(subsumer, subsumed).is_some()
}

/// Two patterns are *pattern-equivalent* iff each subsumes the
/// other — they define the same equivalence class under rewriting.
/// `extract_rules` dedups candidates by this relation; the
/// reinforcement pass picks the canonical representative (lowest
/// hash) when a pair is equivalent rather than strictly subsumed.
#[must_use]
pub fn pattern_equivalent(a: &Term, b: &Term) -> bool {
    subsumes(a, b) && subsumes(b, a)
}

/// Anonymize symbol ids and fresh variable ids in a term so that
/// terms differing only by nominal id choices map to the same
/// canonical form. Concrete operators (var id < 100) are preserved
/// as vocabulary; fresh variables (var id ≥ 100) and Symbol ids
/// get renumbered in first-appearance order.
///
/// Two runs producing rules that encode the same pattern under
/// different fresh ids (inevitable because rule-id minting advances
/// per run) will produce IDENTICAL anonymized terms. This is the
/// foundation of the eager-collapse principle: any two rules that
/// CAN be one rule SHOULD be one rule.
pub fn anonymize_term(t: &Term) -> Term {
    // Single-term anonymization — uses a fresh var_map. For rule
    // anonymization that preserves cross-side variable identity
    // (LHS and RHS share the SAME var_map), use
    // `anonymize_rule`.
    let mut var_map = BTreeMap::new();
    let mut symbol_map = BTreeMap::new();
    anonymize_walk(t, &mut var_map, &mut symbol_map)
}

fn anonymize_walk(
    t: &Term,
    var_map: &mut BTreeMap<u32, u32>,
    symbol_map: &mut BTreeMap<u32, u32>,
) -> Term {
    match t {
        Term::Point(p) => Term::Point(p.clone()),
        Term::Number(n) => Term::Number(n.clone()),
        Term::Var(v) => {
            // Concrete ops (id < 100) are vocabulary — preserve.
            // Fresh vars (id ≥ 100) get canonical renumbering.
            if *v < 100 {
                Term::Var(*v)
            } else {
                let next = var_map.len() as u32;
                let id = *var_map.entry(*v).or_insert(next + 100);
                Term::Var(id)
            }
        }
        Term::Fn(params, body) => {
            // C3 correctness fix (2026-04-18): pre-register Fn
            // params in var_map so the body's references to those
            // params get the SAME canonical id as the param list.
            // Before this fix, body vars were renumbered but
            // params were cloned verbatim — breaking the Fn's
            // binding (param [200] and body Var(100) no longer
            // match after anonymization).
            //
            // Params with id < 100 are vocabulary (operator refs)
            // and pass through unchanged — consistent with how
            // Var leaves are treated below.
            let new_params: Vec<u32> = params
                .iter()
                .map(|p| {
                    if *p < 100 {
                        *p
                    } else {
                        let next = var_map.len() as u32;
                        *var_map.entry(*p).or_insert(next + 100)
                    }
                })
                .collect();
            let b = anonymize_walk(body, var_map, symbol_map);
            Term::Fn(new_params, Box::new(b))
        }
        Term::Apply(f, args) => {
            let f2 = anonymize_walk(f, var_map, symbol_map);
            let args2 = args
                .iter()
                .map(|a| anonymize_walk(a, var_map, symbol_map))
                .collect();
            Term::Apply(Box::new(f2), args2)
        }
        Term::Symbol(id, args) => {
            let next = symbol_map.len() as u32;
            let canonical = *symbol_map.entry(*id).or_insert(next);
            let args2 = args
                .iter()
                .map(|a| anonymize_walk(a, var_map, symbol_map))
                .collect();
            Term::Symbol(canonical, args2)
        }
    }
}

/// Anonymize a rule's LHS and RHS with a SHARED var_map /
/// symbol_map so cross-side variable identity is preserved.
///
/// Correctness fix (2026-04-18): previously `alpha_equivalent`
/// called `anonymize_term(&lhs)` and `anonymize_term(&rhs)`
/// independently — each side got its own fresh var map. This
/// collapsed commutativity-inverted rules (e.g., `add(a, b) →
/// add(b, a)`) to the identity form `add(?v100, ?v101) →
/// add(?v100, ?v101)`, losing the commutativity signal.
///
/// With shared maps: a variable that appears on both LHS and RHS
/// gets the SAME canonical id on both sides. If the RHS
/// structurally differs from the LHS, the anonymized forms
/// remain distinct.
#[must_use]
pub fn anonymize_rule(rule: &RewriteRule) -> RewriteRule {
    let mut var_map = BTreeMap::new();
    let mut symbol_map = BTreeMap::new();
    let lhs = anonymize_walk(&rule.lhs, &mut var_map, &mut symbol_map);
    let rhs = anonymize_walk(&rule.rhs, &mut var_map, &mut symbol_map);
    RewriteRule {
        name: rule.name.clone(),
        lhs,
        rhs,
    }
}

/// *Alpha equivalence* — the eager-collapse predicate. Two rules
/// are alpha-equivalent iff their SHARED-var-map anonymized
/// forms are identical. This is the machine's "can be one term
/// without breaking anything" check: alpha-equivalent rules
/// encode identical patterns under different fresh-id choices.
///
/// Stronger than `pattern_equivalent` (which only checks LHSs) and
/// stricter than `proper_subsumes` (which allows asymmetric
/// subsumption). When this returns true, the two rules are THE SAME
/// rule modulo naming.
#[must_use]
pub fn alpha_equivalent(r1: &RewriteRule, r2: &RewriteRule) -> bool {
    let a1 = anonymize_rule(r1);
    let a2 = anonymize_rule(r2);
    a1.lhs == a2.lhs && a1.rhs == a2.rhs
}

/// *Proper subsumption* — the absolute measure of rule-level
/// irreducibility that the user flagged as the gate for meta-rule
/// collapse. A rule `r1` properly subsumes `r2` iff:
///
/// 1. `r1.lhs` pattern-subsumes `r2.lhs` (classical syntactic
///    subsumption), AND
/// 2. Under the substitution σ that makes `r1.lhs` match `r2.lhs`,
///    σ(r1.rhs) == r2.rhs (the two rules agree on the reduction
///    of anything r2's pattern matches)
///
/// The second clause is the "absolute" part. Without it, a rule
/// like `?op(?x, ?id) => ?x` would syntactically subsume
/// `?op(?x, ?x) => f(?x)` because their LHSs unify with `?id = ?x`
/// — but applying them to `add(5, 5)` yields `5` vs `f(5)`,
/// different outcomes. Those are DISTINCT rules, and collapsing
/// them is loss of information, not efficiency.
///
/// Proper subsumption gives the reinforcement pass an irreducibility
/// check that's independent of the library's state or corpus: it's
/// structural, computable from the rules alone, and decisive —
/// either σ(r1.rhs) equals r2.rhs under normal form or it doesn't.
///
/// Used in `reduction::detect_subsumption_pairs` as the stronger
/// check. Classical `subsumes(lhs, lhs)` remains in place as the
/// fast rejection predicate (if LHSs don't match, no need to
/// consider RHS).
#[must_use]
pub fn proper_subsumes(r1: &RewriteRule, r2: &RewriteRule) -> bool {
    let bindings = match pattern_match(&r1.lhs, &r2.lhs) {
        Some(b) => b,
        None => return false,
    };
    let mut r1_rhs_substituted = r1.rhs.clone();
    for (v, val) in &bindings {
        r1_rhs_substituted = r1_rhs_substituted.substitute(*v, val);
    }
    r1_rhs_substituted == r2.rhs
}

/// Match a pattern term against a concrete term, returning variable bindings.
/// Returns None if the pattern doesn't match.
///
/// Correctness fix (2026-04-18): was `HashMap` which has
/// non-deterministic iteration order. Downstream code that
/// iterates bindings (substitution in rule application, etc.)
/// was non-deterministic across runs. Switched to `BTreeMap` so
/// iteration is sorted by var id — repeatable by construction.
pub fn pattern_match(pattern: &Term, term: &Term) -> Option<BTreeMap<u32, Term>> {
    let mut bindings: BTreeMap<u32, Term> = BTreeMap::new();
    if match_inner(pattern, term, &mut bindings) {
        Some(bindings)
    } else {
        None
    }
}

fn match_inner(pattern: &Term, term: &Term, bindings: &mut BTreeMap<u32, Term>) -> bool {
    match pattern {
        // Pattern variable: bind or check consistency
        Term::Var(v) => {
            if let Some(existing) = bindings.get(v) {
                existing == term
            } else {
                bindings.insert(*v, term.clone());
                true
            }
        }

        // Structural matching
        Term::Point(a) => matches!(term, Term::Point(b) if a == b),
        Term::Number(a) => matches!(term, Term::Number(b) if a == b),

        Term::Apply(pf, pargs) => {
            if let Term::Apply(tf, targs) = term {
                if pargs.len() != targs.len() {
                    return false;
                }
                if !match_inner(pf, tf, bindings) {
                    return false;
                }
                pargs
                    .iter()
                    .zip(targs.iter())
                    .all(|(p, t)| match_inner(p, t, bindings))
            } else {
                false
            }
        }

        Term::Fn(pp, pb) => {
            if let Term::Fn(tp, tb) = term {
                if pp.len() != tp.len() {
                    return false;
                }
                // For pattern matching purposes, treat params as structural
                if pp != tp {
                    return false;
                }
                match_inner(pb, tb, bindings)
            } else {
                false
            }
        }

        Term::Symbol(pid, pargs) => {
            if let Term::Symbol(tid, targs) = term {
                if pid != tid || pargs.len() != targs.len() {
                    return false;
                }
                pargs
                    .iter()
                    .zip(targs.iter())
                    .all(|(p, t)| match_inner(p, t, bindings))
            } else {
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{apply, nat, var};
    use crate::value::Value;

    #[test]
    fn anonymize_preserves_multi_param_fn_bindings() {
        // (fn (?201 ?205) (apply add ?205 ?201)) —
        // two params, body uses both.
        let t = Term::Fn(
            vec![201, 205],
            Box::new(apply(var(2), vec![var(205), var(201)])),
        );
        let anon = anonymize_term(&t);
        if let Term::Fn(params, body) = &anon {
            assert_eq!(params.len(), 2);
            // Collect the param ids — what matters is that the
            // body references them consistently.
            let p0 = params[0];
            let p1 = params[1];
            if let Term::Apply(head, args) = body.as_ref() {
                assert_eq!(**head, Term::Var(2));
                assert_eq!(args.len(), 2);
                // args[0] should reference p1 (=205 originally)
                // args[1] should reference p0 (=201 originally)
                assert_eq!(
                    args[0],
                    Term::Var(p1),
                    "body arg[0] must match second param"
                );
                assert_eq!(
                    args[1],
                    Term::Var(p0),
                    "body arg[1] must match first param"
                );
            } else {
                panic!("expected Apply in body");
            }
        } else {
            panic!("expected Fn");
        }
    }

    #[test]
    fn anonymize_fn_bindings_are_alpha_equivalent_across_renamings() {
        // (fn (?200) ?200) and (fn (?300) ?300) denote the same
        // lambda — identity on a single arg. After anonymization,
        // both must produce IDENTICAL terms. That's the point of
        // canonical renumbering.
        let a = Term::Fn(vec![200], Box::new(Term::Var(200)));
        let b = Term::Fn(vec![300], Box::new(Term::Var(300)));
        assert_ne!(a, b, "different param ids pre-anonymization");
        let anon_a = anonymize_term(&a);
        let anon_b = anonymize_term(&b);
        assert_eq!(
            anon_a, anon_b,
            "alpha-equivalent lambdas must anonymize to same term"
        );
    }

    #[test]
    fn anonymize_preserves_fn_param_binding_probe() {
        // Probe: does anonymization preserve the binding between
        // a Fn's param and its body's bound var?
        //
        // Input: (fn (?200) ?200) — the identity lambda on id=200.
        // After anonymization, the body's Var(200) gets renumbered
        // to Var(100) (fresh-start). But the param list is cloned
        // verbatim. If params aren't also renumbered, the param
        // [200] and body Var(100) no longer match — the binding
        // is broken.
        let t = Term::Fn(vec![200], Box::new(Term::Var(200)));
        let anon = anonymize_term(&t);
        if let Term::Fn(params, body) = &anon {
            let param_id = params[0];
            if let Term::Var(body_var) = body.as_ref() {
                assert_eq!(
                    param_id, *body_var,
                    "KERNEL BUG: Fn param id={param_id} but body references Var({body_var}); \
                     binding broken by anonymization"
                );
            } else {
                panic!("expected body to be a Var");
            }
        } else {
            panic!("expected Fn");
        }
    }

    #[test]
    fn shared_anonymize_preserves_commutativity_signal() {
        // C1 correctness fix: `add(?a, ?b) → add(?b, ?a)` is NOT
        // the identity rule — it's commutativity. Before the fix,
        // independent anonymize_term calls on LHS and RHS produced
        // identical anonymized forms (both became
        // `add(?100, ?101) → add(?100, ?101)` since each side's
        // fresh var map assigned from first-seen order
        // independently). With shared var_map, the cross-side
        // variable identity is preserved.
        let identity_rule = RewriteRule {
            name: "identity".into(),
            lhs: apply(var(2), vec![var(100), var(101)]),
            rhs: apply(var(2), vec![var(100), var(101)]),
        };
        let commute_rule = RewriteRule {
            name: "commute".into(),
            lhs: apply(var(2), vec![var(100), var(101)]),
            rhs: apply(var(2), vec![var(101), var(100)]),
        };
        let anon_id = anonymize_rule(&identity_rule);
        let anon_com = anonymize_rule(&commute_rule);
        assert_eq!(anon_id.lhs, anon_com.lhs, "LHSs are the same");
        assert_ne!(
            anon_id.rhs, anon_com.rhs,
            "RHSs must DIFFER — identity vs commutativity — anonymization preserves distinction"
        );
    }

    #[test]
    fn pattern_match_returns_deterministic_ordering() {
        // C2 correctness fix: pattern_match uses BTreeMap so
        // iteration is sorted by var id. Before: HashMap had
        // non-deterministic iteration order across runs.
        // Pattern binds head (var 2) + args (var 100, 101).
        let pattern = apply(var(2), vec![var(100), var(101)]);
        let concrete = apply(var(2), vec![nat(5), nat(7)]);
        let bindings = pattern_match(&pattern, &concrete).unwrap();
        let keys: Vec<u32> = bindings.keys().copied().collect();
        // BTreeMap iteration is sorted by key — always. This is
        // the property — keys come back in ascending order
        // deterministically.
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted, "binding keys must be in sorted order");
    }

    #[test]
    fn eval_add() {
        let expr = apply(var(BUILTIN_ADD), vec![nat(2), nat(3)]);
        let result = eval(&expr, &[], 100).unwrap();
        assert_eq!(result, nat(5));
    }

    #[test]
    fn eval_mul() {
        let expr = apply(var(BUILTIN_MUL), vec![nat(3), nat(4)]);
        let result = eval(&expr, &[], 100).unwrap();
        assert_eq!(result, nat(12));
    }

    #[test]
    fn eval_succ() {
        let expr = apply(var(BUILTIN_SUCC), vec![nat(7)]);
        let result = eval(&expr, &[], 100).unwrap();
        assert_eq!(result, nat(8));
    }

    #[test]
    fn eval_nested_add() {
        // add(add(1, 2), 3) = 6
        let inner = apply(var(BUILTIN_ADD), vec![nat(1), nat(2)]);
        let outer = apply(var(BUILTIN_ADD), vec![inner, nat(3)]);
        let result = eval(&outer, &[], 100).unwrap();
        assert_eq!(result, nat(6));
    }

    #[test]
    fn eval_beta_reduction() {
        // (fn (?10) (add ?10 1)) applied to 5 => add(5, 1) => 6
        let f = Term::Fn(vec![10], Box::new(apply(var(BUILTIN_ADD), vec![var(10), nat(1)])));
        let expr = apply(f, vec![nat(5)]);
        let result = eval(&expr, &[], 100).unwrap();
        assert_eq!(result, nat(6));
    }

    #[test]
    fn eval_library_rule() {
        // Rule: add(?x, 0) => ?x (additive identity)
        let rule = RewriteRule {
            name: "add-identity".into(),
            lhs: apply(var(BUILTIN_ADD), vec![var(100), nat(0)]),
            rhs: var(100),
        };
        let expr = apply(var(BUILTIN_ADD), vec![nat(42), nat(0)]);
        let result = eval(&expr, &[rule], 100).unwrap();
        assert_eq!(result, nat(42));
    }

    #[test]
    fn pattern_match_basic() {
        // Pattern: add(?x, ?y)
        let pattern = apply(var(BUILTIN_ADD), vec![var(100), var(101)]);
        let term = apply(var(BUILTIN_ADD), vec![nat(3), nat(4)]);
        let bindings = pattern_match(&pattern, &term).unwrap();
        assert_eq!(bindings[&100], nat(3));
        assert_eq!(bindings[&101], nat(4));
    }

    #[test]
    fn pattern_match_repeated_var() {
        // Pattern: add(?x, ?x) — same variable twice
        let pattern = apply(var(BUILTIN_ADD), vec![var(100), var(100)]);

        // Should match: add(3, 3)
        let t1 = apply(var(BUILTIN_ADD), vec![nat(3), nat(3)]);
        assert!(pattern_match(&pattern, &t1).is_some());

        // Should NOT match: add(3, 4)
        let t2 = apply(var(BUILTIN_ADD), vec![nat(3), nat(4)]);
        assert!(pattern_match(&pattern, &t2).is_none());
    }

    #[test]
    fn step_limit_applied() {
        // Chain of nested applications that require many reduction steps
        // succ(succ(succ(... succ(0) ...))) — 20 layers
        let mut expr = nat(0);
        for _ in 0..20 {
            expr = apply(var(BUILTIN_SUCC), vec![expr]);
        }
        // With a limit of 5, should hit the limit
        let result = eval(&expr, &[], 5);
        // Either it reduces partially or hits the limit; it should not fully reduce
        match result {
            Ok(Term::Number(Value::Nat(n))) => assert!(n < 20, "should not fully reduce with step limit 5"),
            Err(EvalError::StepLimitExceeded) => {} // expected
            other => panic!("unexpected result: {other:?}"),
        }
    }
}
