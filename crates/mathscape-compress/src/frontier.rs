use crate::egraph::{entailed_by, library_rewrites};
use crate::law_generator::Domain;
use mathscape_core::builtin::lookup;
use mathscape_core::eval::{RewriteRule, eval};
use mathscape_core::term::Term;
use std::collections::{BTreeSet, HashMap};

pub const FIRST_VAR: u32 = 200;
pub const ENTAIL_ITERS: usize = 8;
pub const ENTAIL_NODES: usize = 4000;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FrontierSpec {
    pub max_size: usize,
    pub vocab: Vec<u32>,
    pub constants: Vec<u64>,
    pub variables: u32,
    pub samples: usize,
    pub seed: u64,
}

impl FrontierSpec {
    #[must_use]
    pub fn extends(&self, earlier: &Self) -> bool {
        self.vocab == earlier.vocab
            && self.constants == earlier.constants
            && self.variables == earlier.variables
            && self.samples == earlier.samples
            && self.seed == earlier.seed
            && self.max_size >= earlier.max_size
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Equation {
    pub index: usize,
    pub size: usize,
    pub lhs: Term,
    pub rhs: Term,
}

impl Equation {
    #[must_use]
    pub fn rule(&self) -> RewriteRule {
        RewriteRule {
            name: format!("F_{}", self.index),
            lhs: self.lhs.clone(),
            rhs: self.rhs.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Frontier {
    spec: FrontierSpec,
    equations: Vec<Equation>,
    terms: usize,
}

type Rep = (Term, Vec<Term>);

struct Sort {
    domain: Domain,
    ops: Vec<(u32, usize)>,
    by_size: Vec<Vec<Rep>>,
    classes: HashMap<Vec<Term>, Term>,
}

fn vars_of(t: &Term, out: &mut BTreeSet<u32>) {
    match t {
        Term::Var(v) if *v >= 100 => {
            out.insert(*v);
        }
        Term::Apply(f, args) => {
            vars_of(f, out);
            for a in args {
                vars_of(a, out);
            }
        }
        Term::Symbol(_, args) => {
            for a in args {
                vars_of(a, out);
            }
        }
        Term::Fn(_, body) => vars_of(body, out),
        _ => {}
    }
}

fn well_formed(lhs: &Term, rhs: &Term) -> bool {
    let (mut lv, mut rv) = (BTreeSet::new(), BTreeSet::new());
    vars_of(lhs, &mut lv);
    vars_of(rhs, &mut rv);
    !lv.is_empty() && rv.is_subset(&lv) && lhs != rhs
}

fn argument_sizes(arity: usize, total: usize) -> Vec<Vec<usize>> {
    match arity {
        0 => Vec::new(),
        1 => vec![vec![total]],
        _ => (1..total)
            .flat_map(|first| {
                argument_sizes(arity - 1, total - first).into_iter().map(move |mut rest| {
                    rest.insert(0, first);
                    rest
                })
            })
            .collect(),
    }
}

fn product<'a>(choices: &[&'a [Rep]]) -> Vec<Vec<&'a Rep>> {
    choices.iter().fold(vec![Vec::new()], |acc, options| {
        acc.into_iter()
            .flat_map(|prefix| {
                options.iter().map(move |o| {
                    let mut next = prefix.clone();
                    next.push(o);
                    next
                })
            })
            .collect()
    })
}

impl Sort {
    fn classify(&mut self, term: Term, sig: Vec<Term>, size: usize, level: &mut Vec<Rep>, equations: &mut Vec<Equation>) {
        match self.classes.get(&sig) {
            Some(rep) => {
                if well_formed(&term, rep) {
                    equations.push(Equation {
                        index: equations.len(),
                        size,
                        lhs: term,
                        rhs: rep.clone(),
                    });
                }
            }
            None => {
                self.classes.insert(sig.clone(), term.clone());
                level.push((term, sig));
            }
        }
    }
}

impl Frontier {
    #[must_use]
    pub fn build(spec: FrontierSpec) -> Self {
        let mut sorts: Vec<Sort> = Domain::ALL
            .iter()
            .filter_map(|&domain| {
                let ops: Vec<(u32, usize)> = spec
                    .vocab
                    .iter()
                    .filter(|op| Domain::from_op_id(**op) == Some(domain))
                    .filter_map(|op| lookup(*op).map(|b| (*op, b.arity)))
                    .filter(|(_, arity)| (1..=2).contains(arity))
                    .collect();
                (!ops.is_empty()).then(|| Sort {
                    domain,
                    ops,
                    by_size: vec![Vec::new()],
                    classes: HashMap::new(),
                })
            })
            .collect();
        let mut equations = Vec::new();
        let mut terms = 0usize;
        for sort in &mut sorts {
            let mut level = Vec::new();
            for v in 0..spec.variables {
                let sig = (0..spec.samples)
                    .map(|i| sort.domain.sample(spec.seed, i, v as usize))
                    .collect();
                terms += 1;
                sort.classify(Term::Var(FIRST_VAR + v), sig, 1, &mut level, &mut equations);
            }
            for c in &spec.constants {
                let value = sort.domain.constant(*c);
                let sig = vec![value.clone(); spec.samples];
                terms += 1;
                sort.classify(value, sig, 1, &mut level, &mut equations);
            }
            sort.by_size.push(level);
        }
        for size in 2..=spec.max_size {
            for sort in &mut sorts {
                let mut level = Vec::new();
                let ops = sort.ops.clone();
                for (op, arity) in ops {
                    for sizes in argument_sizes(arity, size - 1) {
                        let pools: Vec<&[Rep]> = sizes.iter().map(|s| sort.by_size[*s].as_slice()).collect();
                        let built: Vec<(Term, Vec<Term>)> = product(&pools)
                            .into_iter()
                            .filter_map(|args| {
                                let sig = (0..spec.samples)
                                    .map(|i| {
                                        let ground = Term::Apply(
                                            Box::new(Term::Var(op)),
                                            args.iter().map(|a| a.1[i].clone()).collect(),
                                        );
                                        match eval(&ground, &[], 16) {
                                            Ok(v @ Term::Number(_)) => Some(v),
                                            _ => None,
                                        }
                                    })
                                    .collect::<Option<Vec<_>>>()?;
                                let term = Term::Apply(Box::new(Term::Var(op)), args.iter().map(|a| a.0.clone()).collect());
                                Some((term, sig))
                            })
                            .collect();
                        for (term, sig) in built {
                            terms += 1;
                            sort.classify(term, sig, size, &mut level, &mut equations);
                        }
                    }
                }
                sort.by_size.push(level);
            }
        }
        Self { spec, equations, terms }
    }

    #[must_use]
    pub fn spec(&self) -> &FrontierSpec {
        &self.spec
    }

    #[must_use]
    pub fn equations(&self) -> &[Equation] {
        &self.equations
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.equations.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.equations.is_empty()
    }

    #[must_use]
    pub fn terms(&self) -> usize {
        self.terms
    }

    #[must_use]
    pub fn slice(&self, from: usize, budget: usize) -> &[Equation] {
        let start = from.min(self.equations.len());
        let end = start.saturating_add(budget).min(self.equations.len());
        &self.equations[start..end]
    }
}

#[must_use]
pub fn entailed_by_library(rule: &RewriteRule, library: &[RewriteRule]) -> bool {
    entailed_by(rule, &library_rewrites(library), ENTAIL_ITERS, ENTAIL_NODES)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mathscape_core::value::Value;

    fn spec(max_size: usize, vocab: Vec<u32>) -> FrontierSpec {
        FrontierSpec {
            max_size,
            vocab,
            constants: vec![0, 1],
            variables: 3,
            samples: 24,
            seed: 7,
        }
    }

    fn op(id: u32, args: Vec<Term>) -> Term {
        Term::Apply(Box::new(Term::Var(id)), args)
    }

    #[test]
    fn the_smallest_peano_laws_come_first_and_every_equation_is_well_formed() {
        let f = Frontier::build(spec(3, vec![1, 2, 3]));
        let x = Term::Var(FIRST_VAR);
        let zero = Term::Number(Value::Nat(0));
        let found = |lhs: &Term, rhs: &Term| f.equations().iter().any(|e| &e.lhs == lhs && &e.rhs == rhs);
        assert!(found(&op(2, vec![x.clone(), zero.clone()]), &x));
        assert!(found(&op(3, vec![x.clone(), zero.clone()]), &zero));
        assert!(f.equations().iter().all(|e| well_formed(&e.lhs, &e.rhs)));
        assert!(f.equations().windows(2).all(|w| w[0].size <= w[1].size));
    }

    #[test]
    fn growing_only_the_size_extends_the_frontier_without_reordering_it() {
        let small = Frontier::build(spec(4, vec![1, 2, 3]));
        let large = Frontier::build(spec(5, vec![1, 2, 3]));
        assert!(large.spec().extends(small.spec()));
        assert!(large.len() > small.len());
        assert_eq!(&large.equations()[..small.len()], small.equations());
        let other = spec(5, vec![1, 2]);
        assert!(!other.extends(small.spec()));
    }

    #[test]
    fn each_domain_is_enumerated_over_its_own_values() {
        let f = Frontier::build(spec(3, vec![2, 12, 31]));
        let heads: BTreeSet<u32> = f
            .equations()
            .iter()
            .filter_map(|e| match &e.lhs {
                Term::Apply(h, _) => match **h {
                    Term::Var(id) => Some(id),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        assert_eq!(heads, BTreeSet::from([2, 12, 31]));
    }

    #[test]
    fn an_instance_of_a_known_law_is_entailed_in_every_domain() {
        for (add, zero) in [
            (2, Term::Number(Value::Nat(0))),
            (12, Term::Number(Value::Int(0))),
            (31, Term::Number(Value::Float(0.0f64.to_bits()))),
        ] {
            let law = RewriteRule {
                name: "identity".into(),
                lhs: op(add, vec![Term::Var(FIRST_VAR), zero.clone()]),
                rhs: Term::Var(FIRST_VAR),
            };
            let instance = RewriteRule {
                name: "instance".into(),
                lhs: op(add, vec![op(add, vec![Term::Var(201), zero.clone()]), zero.clone()]),
                rhs: Term::Var(201),
            };
            assert!(!entailed_by_library(&instance, &[]));
            assert!(entailed_by_library(&instance, &[law]), "not entailed for op {add}");
        }
    }
}
