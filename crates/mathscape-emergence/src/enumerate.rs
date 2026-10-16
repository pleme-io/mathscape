use mathscape_kernel::Term;
use std::cell::RefCell;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Atom {
    pub term: Term,
    pub cap: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grammar {
    pub lambdas: bool,
    pub atoms: Vec<Atom>,
}

type Key = (usize, u32, usize, usize);

pub struct Enumerator {
    grammar: Grammar,
    classes: Vec<Option<usize>>,
    normal: RefCell<HashMap<(usize, u32), u128>>,
    neutral: RefCell<HashMap<Key, u128>>,
}

impl Enumerator {
    #[must_use]
    pub fn new(grammar: Grammar) -> Self {
        let mut classes = vec![None];
        for a in &grammar.atoms {
            if !classes.contains(&a.cap) {
                classes.push(a.cap);
            }
        }
        Self {
            grammar,
            classes,
            normal: RefCell::new(HashMap::new()),
            neutral: RefCell::new(HashMap::new()),
        }
    }

    #[must_use]
    pub fn grammar(&self) -> &Grammar {
        &self.grammar
    }

    #[must_use]
    pub fn count(&self, size: usize) -> u128 {
        self.normal_count(size, 0)
    }

    #[must_use]
    pub fn term(&self, size: usize, index: u128) -> Option<Term> {
        (index < self.count(size)).then(|| self.unrank_normal(size, 0, index))
    }

    fn heads(&self, depth: u32, class: usize) -> u128 {
        let cap = self.classes[class];
        let atoms = self.grammar.atoms.iter().filter(|a| a.cap == cap).count() as u128;
        if class == 0 { u128::from(depth) + atoms } else { atoms }
    }

    fn head(&self, depth: u32, class: usize, index: u128) -> Term {
        if class == 0 && index < u128::from(depth) {
            return Term::Var(index as u32);
        }
        let skip = if class == 0 { index - u128::from(depth) } else { index };
        let cap = self.classes[class];
        self.grammar
            .atoms
            .iter()
            .filter(|a| a.cap == cap)
            .nth(skip as usize)
            .map(|a| a.term.clone())
            .expect("head index within count")
    }

    fn allows(&self, class: usize, args: usize) -> bool {
        self.classes[class].is_none_or(|cap| args <= cap)
    }

    fn normal_count(&self, size: usize, depth: u32) -> u128 {
        if size == 0 {
            return 0;
        }
        if let Some(v) = self.normal.borrow().get(&(size, depth)) {
            return *v;
        }
        let lam = if self.grammar.lambdas && size >= 2 { self.normal_count(size - 1, depth + 1) } else { 0 };
        let mut total = lam;
        for class in 0..self.classes.len() {
            for args in 0..size {
                total += self.neutral_count(size, depth, class, args);
            }
        }
        self.normal.borrow_mut().insert((size, depth), total);
        total
    }

    fn neutral_count(&self, size: usize, depth: u32, class: usize, args: usize) -> u128 {
        if size == 0 || !self.allows(class, args) {
            return 0;
        }
        if args == 0 {
            return if size == 1 { self.heads(depth, class) } else { 0 };
        }
        if let Some(v) = self.neutral.borrow().get(&(size, depth, class, args)) {
            return *v;
        }
        let mut total = 0;
        for hs in 1..size.saturating_sub(1) {
            total += self.neutral_count(hs, depth, class, args - 1) * self.normal_count(size - 1 - hs, depth);
        }
        self.neutral.borrow_mut().insert((size, depth, class, args), total);
        total
    }

    fn unrank_normal(&self, size: usize, depth: u32, mut index: u128) -> Term {
        if self.grammar.lambdas && size >= 2 {
            let lam = self.normal_count(size - 1, depth + 1);
            if index < lam {
                return Term::lam(self.unrank_normal(size - 1, depth + 1, index));
            }
            index -= lam;
        }
        for class in 0..self.classes.len() {
            for args in 0..size {
                let n = self.neutral_count(size, depth, class, args);
                if index < n {
                    return self.unrank_neutral(size, depth, class, args, index);
                }
                index -= n;
            }
        }
        unreachable!("index within normal count")
    }

    fn unrank_neutral(&self, size: usize, depth: u32, class: usize, args: usize, mut index: u128) -> Term {
        if args == 0 {
            return self.head(depth, class, index);
        }
        for hs in 1..size.saturating_sub(1) {
            let heads = self.neutral_count(hs, depth, class, args - 1);
            let tails = self.normal_count(size - 1 - hs, depth);
            let block = heads * tails;
            if index < block {
                let f = self.unrank_neutral(hs, depth, class, args - 1, index / tails);
                let a = self.unrank_normal(size - 1 - hs, depth, index % tails);
                return Term::app(f, a);
            }
            index -= block;
        }
        unreachable!("index within neutral count")
    }
}

#[must_use]
pub fn lambda() -> Grammar {
    Grammar {
        lambdas: true,
        atoms: Vec::new(),
    }
}

#[must_use]
pub fn combinators(s: u32, k: u32) -> Grammar {
    Grammar {
        lambdas: false,
        atoms: vec![
            Atom { term: Term::Sym(s), cap: Some(2) },
            Atom { term: Term::Sym(k), cap: Some(1) },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mathscape_kernel::{Budget, normalize};
    use std::collections::BTreeSet;

    const CLOSED_LAMBDA: [u128; 12] = [0, 1, 2, 4, 10, 25, 72, 223, 728, 2549, 9254, 35168];

    #[test]
    fn lambda_counts_match_the_closed_normal_form_sequence() {
        let e = Enumerator::new(lambda());
        for (size, want) in CLOSED_LAMBDA.iter().enumerate() {
            assert_eq!(e.count(size + 1), *want, "size {}", size + 1);
        }
    }

    #[test]
    fn unranking_is_a_bijection_onto_closed_normal_forms() {
        let e = Enumerator::new(lambda());
        for size in 1..=9 {
            let mut seen = BTreeSet::new();
            for i in 0..e.count(size) {
                let t = e.term(size, i).unwrap();
                assert!(t.is_closed());
                assert_eq!(t.size(), size);
                assert_eq!(normalize(&t, &[], Budget::default()).as_ref(), Ok(&t));
                assert!(seen.insert(t));
            }
        }
        assert_eq!(e.term(3, 2), None);
    }

    #[test]
    fn combinator_terms_never_contain_a_redex() {
        let e = Enumerator::new(combinators(0, 1));
        let sizes: Vec<u128> = (1..=11).map(|s| e.count(s)).collect();
        assert_eq!(sizes, vec![2, 0, 4, 0, 12, 0, 40, 0, 144, 0, 544]);
        fn redex(t: &Term) -> bool {
            match t {
                Term::App(f, a) => {
                    let k2 = matches!(f.as_ref(), Term::App(g, _) if matches!(g.as_ref(), Term::Sym(1)));
                    let s3 = matches!(f.as_ref(), Term::App(g, _) if matches!(g.as_ref(), Term::App(h, _) if matches!(h.as_ref(), Term::Sym(0))));
                    k2 || s3 || redex(f) || redex(a)
                }
                _ => false,
            }
        }
        for size in [7, 9] {
            for i in 0..e.count(size) {
                assert!(!redex(&e.term(size, i).unwrap()));
            }
        }
    }
}
