use mathscape_kernel::{Budget, Term, numeral, observe_numeral};

pub const GRID_UNARY: u64 = 7;
pub const GRID_BINARY: u64 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Arity {
    Nullary,
    Unary,
    Binary,
}

impl Arity {
    #[must_use]
    pub fn count(self) -> usize {
        match self {
            Self::Nullary => 0,
            Self::Unary => 1,
            Self::Binary => 2,
        }
    }

    #[must_use]
    pub fn grid(self) -> u64 {
        match self {
            Self::Nullary => 1,
            Self::Unary => GRID_UNARY,
            Self::Binary => GRID_BINARY,
        }
    }
}

fn inputs(arity: Arity, n: u64) -> Vec<Vec<u64>> {
    match arity {
        Arity::Nullary => vec![Vec::new()],
        Arity::Unary => (0..n).map(|x| vec![x]).collect(),
        Arity::Binary => (0..n).flat_map(|x| (0..n).map(move |y| vec![x, y])).collect(),
    }
}

#[must_use]
pub fn tabulate(t: &Term, arity: Arity, n: u64, defs: &[Term], budget: Budget) -> Option<Vec<u64>> {
    inputs(arity, n)
        .into_iter()
        .map(|args| observe_numeral(&Term::apps(t.clone(), args.into_iter().map(numeral)), defs, budget).ok().flatten())
        .collect()
}

#[must_use]
pub fn behaviour(t: &Term, arity: Arity, defs: &[Term], budget: Budget) -> Option<Vec<u64>> {
    tabulate(t, arity, arity.grid(), defs, budget)
}

#[must_use]
pub fn depends_on_all(table: &[u64], arity: Arity, n: u64) -> bool {
    let n = n as usize;
    match arity {
        Arity::Nullary => true,
        Arity::Unary => table.iter().any(|v| *v != table[0]),
        Arity::Binary => {
            let at = |x: usize, y: usize| table[x * n + y];
            let first = (0..n).any(|y| (0..n).any(|x| at(x, y) != at(0, y)));
            let second = (0..n).any(|x| (0..n).any(|y| at(x, y) != at(x, 0)));
            first && second
        }
    }
}

#[must_use]
pub fn is_projection(table: &[u64], arity: Arity) -> bool {
    match arity {
        Arity::Unary => table.iter().enumerate().all(|(i, v)| *v == i as u64),
        Arity::Nullary | Arity::Binary => false,
    }
}

#[must_use]
pub fn operator_shaped(table: &[u64], arity: Arity) -> bool {
    arity != Arity::Nullary && depends_on_all(table, arity, arity.grid()) && !is_projection(table, arity)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(i: u32) -> Term {
        Term::Var(i)
    }

    #[test]
    fn church_multiplication_tabulates_as_a_binary_operator() {
        let mul = Term::lams(3, Term::app(v(2), Term::app(v(1), v(0))));
        let t = behaviour(&mul, Arity::Binary, &[], Budget::default()).unwrap();
        assert_eq!(t, (0..4).flat_map(|m| (0..4).map(move |n| m * n)).collect::<Vec<u64>>());
        assert!(operator_shaped(&t, Arity::Binary));
    }

    #[test]
    fn projections_and_constants_are_not_operators() {
        let first = Term::lams(2, v(1));
        let t = behaviour(&first, Arity::Binary, &[], Budget::default()).unwrap();
        assert!(!operator_shaped(&t, Arity::Binary));
        let id = Term::lam(v(0));
        let u = behaviour(&id, Arity::Unary, &[], Budget::default()).unwrap();
        assert!(!operator_shaped(&u, Arity::Unary));
        let c = behaviour(&Term::lams(3, v(0)), Arity::Unary, &[], Budget::default()).unwrap();
        assert!(!operator_shaped(&c, Arity::Unary));
    }
}
