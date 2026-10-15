use super::vocabulary::{InvariantDef, Pattern, Vocabulary};
use crate::builtin;
use crate::eval::RewriteRule;
use crate::term::Term;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Orientation {
    Forward,
    Reversed,
}

pub(crate) type Bindings = BTreeMap<String, Term>;

#[derive(Clone, Debug)]
pub(crate) struct Match<'v> {
    pub def: &'v InvariantDef,
    pub orientation: Orientation,
    pub bindings: Bindings,
    pub specificity: usize,
}

pub(crate) fn matches<'v>(vocabulary: &'v Vocabulary, rule: &RewriteRule) -> Vec<Match<'v>> {
    let mut found: Vec<Match<'v>> = vocabulary
        .invariants
        .iter()
        .filter_map(|def| best_form(def, rule))
        .collect();
    found.sort_by(|a, b| {
        b.specificity
            .cmp(&a.specificity)
            .then((a.orientation == Orientation::Reversed).cmp(&(b.orientation == Orientation::Reversed)))
    });
    found
}

fn best_form<'v>(def: &'v InvariantDef, rule: &RewriteRule) -> Option<Match<'v>> {
    let orientations = [
        (Orientation::Forward, &rule.lhs, &rule.rhs),
        (Orientation::Reversed, &rule.rhs, &rule.lhs),
    ];
    for (lhs_pattern, rhs_pattern) in &def.forms {
        for (orientation, lhs, rhs) in orientations {
            let solution = unify(lhs_pattern, lhs, Bindings::new(), false)
                .into_iter()
                .flat_map(|b| unify(rhs_pattern, rhs, b, false))
                .find(injective);
            if let Some(bindings) = solution {
                return Some(Match {
                    def,
                    orientation,
                    bindings,
                    specificity: specificity(lhs_pattern) + specificity(rhs_pattern),
                });
            }
        }
    }
    None
}

fn specificity(p: &Pattern) -> usize {
    match p {
        Pattern::Lit(_) => 2,
        Pattern::Const(_) => 1,
        Pattern::Meta(_) => 0,
        Pattern::App(h, args) => specificity(h) + args.iter().map(specificity).sum::<usize>(),
    }
}

fn injective(b: &Bindings) -> bool {
    let values: Vec<&Term> = b.values().collect();
    values
        .iter()
        .enumerate()
        .all(|(i, v)| values[i + 1..].iter().all(|w| w != v))
}

fn unify(p: &Pattern, t: &Term, b: Bindings, head: bool) -> Vec<Bindings> {
    match p {
        Pattern::Meta(name) => {
            if !head && !mentions_rule_variable(t) {
                return Vec::new();
            }
            bind(name, t, b)
        }
        Pattern::Const(name) => {
            if is_constant(t) {
                bind(name, t, b)
            } else {
                Vec::new()
            }
        }
        Pattern::Lit(term) => {
            if term == t {
                vec![b]
            } else {
                Vec::new()
            }
        }
        Pattern::App(ph, pargs) => {
            let (th, targs) = match t {
                Term::Apply(h, args) => (h.as_ref().clone(), args.as_slice()),
                Term::Symbol(id, args) => (Term::Symbol(*id, Vec::new()), args.as_slice()),
                _ => return Vec::new(),
            };
            if pargs.len() != targs.len() {
                return Vec::new();
            }
            let mut orders: Vec<Vec<&Term>> = vec![targs.iter().collect()];
            if targs.len() == 2 && builtin::is_commutative(&th) {
                orders.push(vec![&targs[1], &targs[0]]);
            }
            let mut out = Vec::new();
            for heads in unify(ph, &th, b, true) {
                for order in &orders {
                    let mut frontier = vec![heads.clone()];
                    for (pa, ta) in pargs.iter().zip(order) {
                        frontier = frontier
                            .into_iter()
                            .flat_map(|fb| unify(pa, ta, fb, false))
                            .collect();
                        if frontier.is_empty() {
                            break;
                        }
                    }
                    out.extend(frontier);
                }
            }
            out
        }
    }
}

fn bind(name: &str, t: &Term, mut b: Bindings) -> Vec<Bindings> {
    match b.get(name) {
        Some(existing) if existing == t => vec![b],
        Some(_) => Vec::new(),
        None => {
            b.insert(name.to_owned(), t.clone());
            vec![b]
        }
    }
}

pub(crate) fn is_constant(t: &Term) -> bool {
    match t {
        Term::Number(_) | Term::Point(_) => true,
        Term::Apply(h, args) => args.is_empty() && matches!(h.as_ref(), Term::Var(id) if *id < 100),
        _ => false,
    }
}

pub(crate) fn mentions_rule_variable(t: &Term) -> bool {
    match t {
        Term::Var(id) => *id >= 100,
        Term::Point(_) | Term::Number(_) => false,
        Term::Fn(_, body) => mentions_rule_variable(body),
        Term::Apply(h, args) => mentions_rule_variable(h) || args.iter().any(mentions_rule_variable),
        Term::Symbol(_, args) => args.iter().any(mentions_rule_variable),
    }
}
