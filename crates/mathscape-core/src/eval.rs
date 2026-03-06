//! Expression evaluator — reduces terms using Peano arithmetic builtins
//! and user-defined library rewrite rules.

use crate::term::Term;
use crate::value::Value;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Built-in operations available in the base evaluator.
const BUILTIN_ZERO: u32 = 0;
const BUILTIN_SUCC: u32 = 1;
const BUILTIN_ADD: u32 = 2;
const BUILTIN_MUL: u32 = 3;

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

/// Try to evaluate as a built-in Peano arithmetic operation.
fn try_builtin(func: &Term, args: &[Term]) -> Result<Option<Term>, EvalError> {
    match func {
        Term::Var(BUILTIN_ZERO) if args.is_empty() => Ok(Some(Term::Number(Value::zero()))),

        Term::Var(BUILTIN_SUCC) if args.len() == 1 => {
            if let Term::Number(v) = &args[0] {
                Ok(Some(Term::Number(v.succ())))
            } else {
                Ok(None) // arg not yet reduced to a number
            }
        }

        Term::Var(BUILTIN_ADD) if args.len() == 2 => {
            if let (Term::Number(Value::Nat(a)), Term::Number(Value::Nat(b))) =
                (&args[0], &args[1])
            {
                Ok(Some(Term::Number(Value::Nat(a + b))))
            } else {
                Ok(None)
            }
        }

        Term::Var(BUILTIN_MUL) if args.len() == 2 => {
            if let (Term::Number(Value::Nat(a)), Term::Number(Value::Nat(b))) =
                (&args[0], &args[1])
            {
                Ok(Some(Term::Number(Value::Nat(a * b))))
            } else {
                Ok(None)
            }
        }

        _ => Ok(None),
    }
}

/// Match a pattern term against a concrete term, returning variable bindings.
/// Returns None if the pattern doesn't match.
pub fn pattern_match(pattern: &Term, term: &Term) -> Option<HashMap<u32, Term>> {
    let mut bindings = HashMap::new();
    if match_inner(pattern, term, &mut bindings) {
        Some(bindings)
    } else {
        None
    }
}

fn match_inner(pattern: &Term, term: &Term, bindings: &mut HashMap<u32, Term>) -> bool {
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
