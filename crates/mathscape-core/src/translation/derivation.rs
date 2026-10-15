use super::latex::{term_latex_with, VarNames};
use super::vocabulary::vocabulary;
use super::Translation;
use crate::builtin;
use crate::eval::RewriteRule;
use crate::term::Term;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum StepKind {
    Rewrite {
        rule_name: String,
        canonical_name: String,
        substitution: Vec<(String, String)>,
        modulo_commutativity: bool,
    },
    Kernel {
        operator: String,
        axiom: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DerivationStep {
    pub before: Term,
    pub after: Term,
    pub position: Vec<usize>,
    pub kind: StepKind,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Derivation {
    pub start: Term,
    pub steps: Vec<DerivationStep>,
    pub normal_form: Term,
    pub complete: bool,
}

pub fn derive(term: &Term, library: &[RewriteRule], step_limit: usize) -> Derivation {
    let translations: Vec<Translation> = library.iter().map(Translation::of).collect();
    let mut current = term.clone();
    let mut steps = Vec::new();
    let mut visited = std::collections::BTreeSet::from([term.clone()]);
    let complete = loop {
        if steps.len() >= step_limit {
            break false;
        }
        let Some(step) = rewrite_step(&current, library, &translations, &visited)
            .or_else(|| kernel_step(&current))
        else {
            break true;
        };
        current = step.after.clone();
        visited.insert(current.clone());
        steps.push(step);
    };
    Derivation {
        start: term.clone(),
        steps,
        normal_form: current,
        complete,
    }
}

fn rewrite_step(
    term: &Term,
    library: &[RewriteRule],
    translations: &[Translation],
    visited: &std::collections::BTreeSet<Term>,
) -> Option<DerivationStep> {
    for (position, sub) in positions(term) {
        for (rule, translation) in library.iter().zip(translations) {
            let bindings = match_rule(&rule.lhs, sub).map(|b| (b, false)).or_else(|| {
                swapped(sub).and_then(|s| match_rule(&rule.lhs, &s)).map(|b| (b, true))
            });
            let Some((bindings, modulo_commutativity)) = bindings else { continue };
            let replacement = substitute_all(&rule.rhs, &bindings);
            let after = replace_at(term, &position, replacement);
            if visited.contains(&after) {
                continue;
            }
            let names = VarNames::for_rule(rule);
            let display = VarNames::for_term(term);
            let substitution = bindings
                .iter()
                .map(|(v, val)| (names.name(*v), term_latex_with(val, &display, vocabulary())))
                .collect();
            return Some(DerivationStep {
                before: term.clone(),
                after,
                position,
                kind: StepKind::Rewrite {
                    rule_name: rule.name.clone(),
                    canonical_name: translation.canonical_name().to_owned(),
                    substitution,
                    modulo_commutativity,
                },
            });
        }
    }
    None
}

fn kernel_step(term: &Term) -> Option<DerivationStep> {
    let mut candidates = positions(term);
    candidates.reverse();
    for (position, sub) in candidates {
        let Term::Apply(head, args) = sub else { continue };
        let Term::Var(id) = head.as_ref() else { continue };
        let Some(b) = builtin::lookup(*id) else { continue };
        if args.len() != b.arity || !args.iter().all(|a| matches!(a, Term::Number(_))) {
            continue;
        }
        let Some(result) = (b.eval)(args) else { continue };
        if &result == sub {
            continue;
        }
        let axiom = vocabulary()
            .operator(*id)
            .map(|o| o.axiom.clone())
            .unwrap_or_default();
        return Some(DerivationStep {
            before: term.clone(),
            after: replace_at(term, &position, result),
            position,
            kind: StepKind::Kernel {
                operator: b.name.to_owned(),
                axiom,
            },
        });
    }
    None
}

fn substitute_all(t: &Term, bindings: &BTreeMap<u32, Term>) -> Term {
    match t {
        Term::Var(v) => bindings.get(v).cloned().unwrap_or_else(|| t.clone()),
        Term::Apply(h, args) => Term::Apply(
            Box::new(substitute_all(h, bindings)),
            args.iter().map(|a| substitute_all(a, bindings)).collect(),
        ),
        Term::Symbol(id, args) => Term::Symbol(*id, args.iter().map(|a| substitute_all(a, bindings)).collect()),
        Term::Fn(params, body) => {
            let inner: BTreeMap<u32, Term> = bindings
                .iter()
                .filter(|(k, _)| !params.contains(k))
                .map(|(k, v)| (*k, v.clone()))
                .collect();
            Term::Fn(params.clone(), Box::new(substitute_all(body, &inner)))
        }
        other => other.clone(),
    }
}

fn swapped(t: &Term) -> Option<Term> {
    match t {
        Term::Apply(h, args) if args.len() == 2 && builtin::is_commutative(h) => {
            Some(Term::Apply(h.clone(), vec![args[1].clone(), args[0].clone()]))
        }
        _ => None,
    }
}

fn positions(term: &Term) -> Vec<(Vec<usize>, &Term)> {
    fn walk<'t>(t: &'t Term, path: &mut Vec<usize>, out: &mut Vec<(Vec<usize>, &'t Term)>) {
        out.push((path.clone(), t));
        let children: &[Term] = match t {
            Term::Apply(_, args) | Term::Symbol(_, args) => args,
            _ => &[],
        };
        for (i, c) in children.iter().enumerate() {
            path.push(i);
            walk(c, path, out);
            path.pop();
        }
    }
    let mut out = Vec::new();
    walk(term, &mut Vec::new(), &mut out);
    out
}

fn replace_at(term: &Term, path: &[usize], replacement: Term) -> Term {
    let Some((first, rest)) = path.split_first() else {
        return replacement;
    };
    match term {
        Term::Apply(h, args) => {
            let mut args = args.clone();
            args[*first] = replace_at(&args[*first], rest, replacement);
            Term::Apply(h.clone(), args)
        }
        Term::Symbol(id, args) => {
            let mut args = args.clone();
            args[*first] = replace_at(&args[*first], rest, replacement);
            Term::Symbol(*id, args)
        }
        other => other.clone(),
    }
}

fn match_rule(pattern: &Term, term: &Term) -> Option<BTreeMap<u32, Term>> {
    fn go(p: &Term, t: &Term, b: &mut BTreeMap<u32, Term>) -> bool {
        match p {
            Term::Var(v) if *v >= 100 => match b.get(v) {
                Some(existing) => existing == t,
                None => {
                    b.insert(*v, t.clone());
                    true
                }
            },
            Term::Apply(ph, pargs) => match t {
                Term::Apply(th, targs) if pargs.len() == targs.len() => {
                    go(ph, th, b) && pargs.iter().zip(targs).all(|(x, y)| go(x, y, b))
                }
                _ => false,
            },
            Term::Symbol(pid, pargs) => match t {
                Term::Symbol(tid, targs) if pid == tid && pargs.len() == targs.len() => {
                    pargs.iter().zip(targs).all(|(x, y)| go(x, y, b))
                }
                _ => false,
            },
            other => other == t,
        }
    }
    let mut b = BTreeMap::new();
    go(pattern, term, &mut b).then_some(b)
}
