use crate::behavior::{Arity, behaviour, operator_shaped};
use mathscape_core::certification::{CertificationVerdict, Certifier};
use mathscape_core::eval::RewriteRule;
use mathscape_core::term::Term as Core;
use mathscape_kernel::{Budget, Halt, Term, numeral, observe};
use std::collections::BTreeMap;

pub const PATTERN_BASE: u32 = 100;
pub const BINDER_BASE: u32 = 1_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outside {
    Builtin(u32),
    Number,
    Point,
    Unbound(u32),
}

impl std::fmt::Display for Outside {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Builtin(id) => write!(f, "builtin operator #{id} is outside the emergent substrate"),
            Self::Number => write!(f, "a Number literal is outside the emergent substrate"),
            Self::Point => write!(f, "a Point is outside the emergent substrate"),
            Self::Unbound(v) => write!(f, "variable {v} is neither bound nor a pattern variable"),
        }
    }
}

#[must_use]
pub fn to_core(t: &Term) -> Core {
    fn go(t: &Term, depth: u32) -> Core {
        match t {
            Term::Var(i) => Core::Var(BINDER_BASE + depth - 1 - i),
            Term::Sym(k) => Core::Symbol(*k, Vec::new()),
            Term::Lam(b) => Core::Fn(vec![BINDER_BASE + depth], Box::new(go(b, depth + 1))),
            Term::App(f, a) => Core::Apply(Box::new(go(f, depth)), vec![go(a, depth)]),
        }
    }
    go(t, 0)
}

pub fn to_kernel(t: &Core, sample: &BTreeMap<u32, Term>) -> Result<Term, Outside> {
    fn go(t: &Core, binders: &mut Vec<u32>, sample: &BTreeMap<u32, Term>) -> Result<Term, Outside> {
        match t {
            Core::Var(v) => {
                if let Some(pos) = binders.iter().rposition(|b| b == v) {
                    return Ok(Term::Var((binders.len() - 1 - pos) as u32));
                }
                if let Some(s) = sample.get(v) {
                    return Ok(s.clone());
                }
                if *v < PATTERN_BASE {
                    Err(Outside::Builtin(*v))
                } else {
                    Err(Outside::Unbound(*v))
                }
            }
            Core::Number(_) => Err(Outside::Number),
            Core::Point(_) => Err(Outside::Point),
            Core::Symbol(k, args) => {
                let args = args.iter().map(|a| go(a, binders, sample)).collect::<Result<Vec<_>, _>>()?;
                Ok(Term::apps(Term::Sym(*k), args))
            }
            Core::Fn(params, body) => {
                let n = params.len();
                binders.extend(params.iter().copied());
                let b = go(body, binders, sample);
                binders.truncate(binders.len() - n);
                Ok(Term::lams(n, b?))
            }
            Core::Apply(f, args) => {
                let f = go(f, binders, sample)?;
                let args = args.iter().map(|a| go(a, binders, sample)).collect::<Result<Vec<_>, _>>()?;
                Ok(Term::apps(f, args))
            }
        }
    }
    go(t, &mut Vec::new(), sample)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Sort {
    Numeral,
    Operator(usize),
}

pub fn sorts(t: &Core, out: &mut BTreeMap<u32, Sort>) {
    fn go(t: &Core, bound: &mut Vec<u32>, out: &mut BTreeMap<u32, Sort>) {
        match t {
            Core::Var(v) if *v >= PATTERN_BASE && !bound.contains(v) => {
                out.entry(*v).or_insert(Sort::Numeral);
            }
            Core::Var(_) | Core::Number(_) | Core::Point(_) => {}
            Core::Apply(f, args) => {
                if let Core::Var(v) = f.as_ref()
                    && *v >= PATTERN_BASE
                    && !bound.contains(v)
                {
                    out.insert(*v, Sort::Operator(args.len()));
                } else {
                    go(f, bound, out);
                }
                for a in args {
                    go(a, bound, out);
                }
            }
            Core::Symbol(_, args) => {
                for a in args {
                    go(a, bound, out);
                }
            }
            Core::Fn(params, body) => {
                let n = params.len();
                bound.extend(params.iter().copied());
                go(body, bound, out);
                bound.truncate(bound.len() - n);
            }
        }
    }
    go(t, &mut Vec::new(), out);
}

#[must_use]
pub fn definition_symbol(rule: &RewriteRule) -> Option<u32> {
    match &rule.lhs {
        Core::Symbol(k, args) if args.is_empty() => Some(*k),
        _ => None,
    }
}

pub fn held_definitions(context: &[RewriteRule]) -> Result<Vec<Term>, Outside> {
    let mut defs = Vec::new();
    for rule in context {
        if definition_symbol(rule) == Some(defs.len() as u32) {
            defs.push(to_kernel(&rule.rhs, &BTreeMap::new())?);
        }
    }
    Ok(defs)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KernelCertifier {
    pub exhaustive: u64,
    pub samples: usize,
    pub pool: u64,
    pub seed: u64,
    pub budget: Budget,
}

impl Default for KernelCertifier {
    fn default() -> Self {
        Self {
            exhaustive: 4_096,
            samples: 64,
            pool: 7,
            seed: 0x5EED_CE27,
            budget: Budget::default(),
        }
    }
}

#[must_use]
pub fn mix(seed: u64, a: u64, b: u64) -> u64 {
    let mut z = seed ^ a.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ b.wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

impl KernelCertifier {
    fn definition(&self, k: u32, rule: &RewriteRule, defs: &[Term]) -> CertificationVerdict {
        if k as usize != defs.len() {
            return CertificationVerdict::Rejected {
                reason: format!("symbol #{k} is not the next fresh symbol #{}", defs.len()),
            };
        }
        let body = match to_kernel(&rule.rhs, &BTreeMap::new()) {
            Ok(b) => b,
            Err(e) => return CertificationVerdict::Rejected { reason: e.to_string() },
        };
        let mut used = Vec::new();
        body.symbols(&mut used);
        if !body.is_closed() || used.iter().any(|s| *s >= k) {
            return CertificationVerdict::Rejected {
                reason: "a definition must be closed over earlier symbols".into(),
            };
        }
        let claim = match rule.name.strip_prefix("def ").and_then(|n| n.chars().next()) {
            Some('k') => return CertificationVerdict::Certified { samples_passed: 0 },
            Some('c') => Arity::Nullary,
            Some('u') => Arity::Unary,
            Some('b') => Arity::Binary,
            _ => {
                return CertificationVerdict::Rejected {
                    reason: format!("`{}` claims no kind: def k (basis), c (constant), u (unary) or b (binary)", rule.name),
                };
            }
        };
        let mut all = defs.to_vec();
        all.push(body);
        match behaviour(&Term::Sym(k), claim, &all, self.budget) {
            Some(t) if claim == Arity::Nullary || operator_shaped(&t, claim) => CertificationVerdict::Certified { samples_passed: t.len() },
            _ => CertificationVerdict::Rejected {
                reason: format!("the definition does not behave as a {claim:?} operator on numerals"),
            },
        }
    }

    fn operators(&self, defs: &[Term], arity: usize) -> Vec<u32> {
        let a = match arity {
            1 => Arity::Unary,
            2 => Arity::Binary,
            _ => return Vec::new(),
        };
        (0..defs.len() as u32)
            .filter(|k| behaviour(&Term::Sym(*k), a, defs, self.budget).is_some_and(|t| operator_shaped(&t, a)))
            .collect()
    }
}

impl Certifier for KernelCertifier {
    fn try_certify(&self, rule: &RewriteRule, context: &[RewriteRule]) -> CertificationVerdict {
        let defs = match held_definitions(context) {
            Ok(d) => d,
            Err(e) => return CertificationVerdict::Rejected { reason: e.to_string() },
        };
        if let Some(k) = definition_symbol(rule)
            && k as usize >= defs.len()
        {
            return self.definition(k, rule, &defs);
        }
        let mut vars = BTreeMap::new();
        sorts(&rule.lhs, &mut vars);
        sorts(&rule.rhs, &mut vars);
        let mut pools: BTreeMap<u32, Vec<Term>> = BTreeMap::new();
        for (v, sort) in &vars {
            let pool: Vec<Term> = match sort {
                Sort::Numeral => (0..self.pool).map(numeral).collect(),
                Sort::Operator(a) => self.operators(&defs, *a).into_iter().map(Term::Sym).collect(),
            };
            if pool.is_empty() {
                return CertificationVerdict::Rejected {
                    reason: format!("no held operator of arity {} to sample ?{v} from", match sort {
                        Sort::Operator(a) => *a,
                        Sort::Numeral => 0,
                    }),
                };
            }
            pools.insert(*v, pool);
        }
        let space: u64 = pools.values().map(|p| p.len() as u64).try_fold(1u64, u64::checked_mul).unwrap_or(u64::MAX);
        let exhaustive = space <= self.exhaustive;
        let runs = if exhaustive { space } else { self.samples as u64 };
        let mut decided = 0;
        for s in 0..runs {
            let mut rest = s;
            let sample: BTreeMap<u32, Term> = pools
                .iter()
                .enumerate()
                .map(|(pos, (v, pool))| {
                    let n = pool.len() as u64;
                    let i = if exhaustive {
                        let i = rest % n;
                        rest /= n;
                        i
                    } else {
                        mix(self.seed, s, pos as u64) % n
                    };
                    (*v, pool[i as usize].clone())
                })
                .collect();
            let (l, r) = match (to_kernel(&rule.lhs, &sample), to_kernel(&rule.rhs, &sample)) {
                (Ok(l), Ok(r)) => (l, r),
                (Err(e), _) | (_, Err(e)) => return CertificationVerdict::Rejected { reason: e.to_string() },
            };
            match (observe(&l, &defs, self.budget), observe(&r, &defs, self.budget)) {
                (Ok(a), Ok(b)) if a == b => decided += 1,
                (Ok(_), Ok(_)) => {
                    return CertificationVerdict::Rejected {
                        reason: format!("the sides disagree at sample {s}"),
                    };
                }
                (Err(Halt::Unbound(k)), _) | (_, Err(Halt::Unbound(k))) => {
                    return CertificationVerdict::Rejected {
                        reason: format!("symbol #{k} is not held"),
                    };
                }
                (Err(_), Err(_)) => {}
                (Err(h), Ok(_)) | (Ok(_), Err(h)) => {
                    return CertificationVerdict::Rejected {
                        reason: format!("one side halts ({h}) where the other normalizes, at sample {s}"),
                    };
                }
            }
        }
        if decided * 2 < runs as usize {
            return CertificationVerdict::Rejected {
                reason: format!("only {decided} of {runs} samples normalized within budget"),
            };
        }
        CertificationVerdict::Certified { samples_passed: decided }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mathscape_core::certification::DefaultCertifier;

    fn v(i: u32) -> Term {
        Term::Var(i)
    }

    fn add() -> Term {
        Term::lams(4, Term::apps(v(3), [v(1), Term::apps(v(2), [v(1), v(0)])]))
    }

    fn mul() -> Term {
        Term::lams(3, Term::app(v(2), Term::app(v(1), v(0))))
    }

    fn exp() -> Term {
        Term::lams(2, Term::app(v(0), v(1)))
    }

    fn def(k: u32, t: &Term) -> RewriteRule {
        RewriteRule {
            name: format!("def b{k}"),
            lhs: Core::Symbol(k, Vec::new()),
            rhs: to_core(t),
        }
    }

    fn p(i: u32) -> Core {
        Core::Var(PATTERN_BASE + i)
    }

    fn law(lhs: Core, rhs: Core) -> RewriteRule {
        RewriteRule { name: "law".into(), lhs, rhs }
    }

    fn certified(v: &CertificationVerdict) -> bool {
        matches!(v, CertificationVerdict::Certified { .. })
    }

    #[test]
    fn lambda_terms_round_trip_through_the_core_representation() {
        for t in [add(), mul(), exp(), numeral(3)] {
            assert_eq!(to_kernel(&to_core(&t), &BTreeMap::new()), Ok(t));
        }
    }

    #[test]
    fn a_definition_is_certified_only_as_the_next_fresh_symbol_over_the_primitives() {
        let c = KernelCertifier::default();
        assert!(certified(&c.try_certify(&def(0, &add()), &[])));
        assert!(!certified(&c.try_certify(&def(1, &add()), &[])));
        let mislabelled = RewriteRule { name: "def u0".into(), ..def(0, &add()) };
        assert!(!certified(&c.try_certify(&mislabelled, &[])));
        let unlabelled = RewriteRule { name: "add".into(), ..def(0, &add()) };
        assert!(!certified(&c.try_certify(&unlabelled, &[])));
        let builtin = RewriteRule {
            name: "def b0".into(),
            lhs: Core::Symbol(0, Vec::new()),
            rhs: Core::Apply(Box::new(Core::Var(2)), vec![p(0), p(1)]),
        };
        assert!(!certified(&c.try_certify(&builtin, &[])));
    }

    #[test]
    fn laws_of_a_held_operator_are_certified_and_false_ones_refuted() {
        let c = KernelCertifier::default();
        let ctx = vec![def(0, &add())];
        let s = |a, b| Core::Symbol(0, vec![a, b]);
        assert!(certified(&c.try_certify(&law(s(p(1), p(0)), s(p(0), p(1))), &ctx)));
        assert!(certified(&c.try_certify(&law(s(s(p(0), p(1)), p(2)), s(p(0), s(p(1), p(2)))), &ctx)));
        assert!(!certified(&c.try_certify(&law(s(p(0), p(1)), p(0)), &ctx)));
    }

    #[test]
    fn an_operator_variable_ranges_over_the_held_operators_of_its_arity() {
        let c = KernelCertifier::default();
        let f = |a, b| Core::Apply(Box::new(Core::Var(PATTERN_BASE + 9)), vec![a, b]);
        let swapped = law(f(p(1), p(0)), f(p(0), p(1)));
        let commutative = vec![def(0, &add()), def(1, &mul())];
        assert!(certified(&c.try_certify(&swapped, &commutative)));
        let mut with_exp = commutative.clone();
        with_exp.push(def(2, &exp()));
        assert!(!certified(&c.try_certify(&swapped, &with_exp)));
        let nothing_held = c.try_certify(&swapped, &[]);
        assert!(matches!(&nothing_held, CertificationVerdict::Rejected { reason } if reason.contains("no held operator")), "{nothing_held:?}");
        assert!(!certified(&DefaultCertifier::default().try_certify(&swapped, &commutative)));
    }
}
