use crate::certify::{PATTERN_BASE, mix};
use mathscape_core::term::Term as Core;
use mathscape_kernel::{Budget, Term, numeral, observe_numeral};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LTerm {
    Var(u8),
    Op(u32, Vec<LTerm>),
}

impl LTerm {
    #[must_use]
    pub fn size(&self) -> usize {
        match self {
            Self::Var(_) => 1,
            Self::Op(_, args) => 1 + args.iter().map(Self::size).sum::<usize>(),
        }
    }

    #[must_use]
    pub fn ground(&self) -> bool {
        match self {
            Self::Var(_) => false,
            Self::Op(_, args) => args.iter().all(Self::ground),
        }
    }

    #[must_use]
    pub fn mentions(&self, k: u32) -> bool {
        match self {
            Self::Var(_) => false,
            Self::Op(o, args) => *o == k || args.iter().any(|a| a.mentions(k)),
        }
    }

    #[must_use]
    pub fn kernel(&self, assignment: &[u64]) -> Term {
        match self {
            Self::Var(v) => numeral(assignment[*v as usize]),
            Self::Op(k, args) => Term::apps(Term::Sym(*k), args.iter().map(|a| a.kernel(assignment))),
        }
    }

    #[must_use]
    pub fn core(&self) -> Core {
        match self {
            Self::Var(v) => Core::Var(PATTERN_BASE + u32::from(*v)),
            Self::Op(k, args) => Core::Symbol(*k, args.iter().map(Self::core).collect()),
        }
    }

    #[must_use]
    pub fn from_core(t: &Core) -> Option<Self> {
        match t {
            Core::Var(v) if *v >= PATTERN_BASE && *v < PATTERN_BASE + 256 => Some(Self::Var((*v - PATTERN_BASE) as u8)),
            Core::Symbol(k, args) => Some(Self::Op(*k, args.iter().map(Self::from_core).collect::<Option<Vec<_>>>()?)),
            _ => None,
        }
    }

    fn rename(&self, map: &mut Vec<u8>) -> Self {
        match self {
            Self::Var(v) => {
                let pos = map.iter().position(|x| x == v).unwrap_or_else(|| {
                    map.push(*v);
                    map.len() - 1
                });
                Self::Var(pos as u8)
            }
            Self::Op(k, args) => Self::Op(*k, args.iter().map(|a| a.rename(map)).collect()),
        }
    }
}

#[must_use]
pub fn canonical(lhs: &LTerm, rhs: &LTerm) -> (LTerm, LTerm) {
    let mut map = Vec::new();
    let l = lhs.rename(&mut map);
    let r = rhs.rename(&mut map);
    (l, r)
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Head {
    Var(u8),
    Op(u32),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Node {
    head: Head,
    args: Vec<usize>,
}

pub struct Universe {
    nodes: Vec<Node>,
    index: HashMap<Node, usize>,
    pub truncated: bool,
}

impl Universe {
    #[must_use]
    pub fn build(signature: &[(u32, usize)], vars: u8, max_size: usize, cap: usize) -> Self {
        let mut u = Self {
            nodes: Vec::new(),
            index: HashMap::new(),
            truncated: false,
        };
        let mut by_size: Vec<Vec<usize>> = vec![Vec::new(); max_size + 1];
        'sizes: for size in 1..=max_size {
            let mut fresh = Vec::new();
            if size == 1 {
                fresh.extend((0..vars).map(|v| Node { head: Head::Var(v), args: Vec::new() }));
            }
            for (k, arity) in signature {
                if *arity == 0 {
                    if size == 1 {
                        fresh.push(Node { head: Head::Op(*k), args: Vec::new() });
                    }
                    continue;
                }
                for split in compositions(size - 1, *arity) {
                    let mut combos: Vec<Vec<usize>> = vec![Vec::new()];
                    for part in split {
                        combos = combos
                            .into_iter()
                            .flat_map(|c| {
                                by_size[part].iter().map(move |id| {
                                    let mut c = c.clone();
                                    c.push(*id);
                                    c
                                })
                            })
                            .collect();
                    }
                    fresh.extend(combos.into_iter().map(|args| Node { head: Head::Op(*k), args }));
                }
            }
            for node in fresh {
                if u.nodes.len() >= cap {
                    u.truncated = true;
                    break 'sizes;
                }
                let id = u.nodes.len();
                u.index.insert(node.clone(), id);
                u.nodes.push(node);
                by_size[size].push(id);
            }
        }
        u
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    #[must_use]
    pub fn leaves(&self) -> Vec<usize> {
        (0..self.nodes.len()).filter(|i| self.nodes[*i].args.is_empty()).collect()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    #[must_use]
    pub fn term(&self, id: usize) -> LTerm {
        let n = &self.nodes[id];
        match n.head {
            Head::Var(v) => LTerm::Var(v),
            Head::Op(k) => LTerm::Op(k, n.args.iter().map(|a| self.term(*a)).collect()),
        }
    }

    fn matches(&self, pattern: &LTerm, id: usize, sigma: &mut Vec<Option<usize>>) -> bool {
        match pattern {
            LTerm::Var(v) => {
                let slot = &mut sigma[*v as usize];
                match slot {
                    Some(b) => *b == id,
                    None => {
                        *slot = Some(id);
                        true
                    }
                }
            }
            LTerm::Op(k, args) => {
                let n = &self.nodes[id];
                n.head == Head::Op(*k)
                    && n.args.len() == args.len()
                    && args.iter().zip(n.args.clone()).all(|(p, a)| self.matches(p, a, sigma))
            }
        }
    }

    fn instantiate(&self, pattern: &LTerm, sigma: &[Option<usize>]) -> Option<usize> {
        match pattern {
            LTerm::Var(v) => sigma[*v as usize],
            LTerm::Op(k, args) => {
                let args = args.iter().map(|a| self.instantiate(a, sigma)).collect::<Option<Vec<_>>>()?;
                self.index.get(&Node { head: Head::Op(*k), args }).copied()
            }
        }
    }
}

fn compositions(total: usize, parts: usize) -> Vec<Vec<usize>> {
    if parts == 1 {
        return if total >= 1 { vec![vec![total]] } else { Vec::new() };
    }
    let mut out = Vec::new();
    for first in 1..total {
        for mut rest in compositions(total - first, parts - 1) {
            rest.insert(0, first);
            out.push(rest);
        }
    }
    out
}

#[derive(Clone)]
pub struct Closure {
    parent: Vec<usize>,
}

impl Closure {
    #[must_use]
    pub fn new(universe: &Universe) -> Self {
        Self {
            parent: (0..universe.len()).collect(),
        }
    }

    pub fn find(&mut self, mut x: usize) -> usize {
        while self.parent[x] != x {
            self.parent[x] = self.parent[self.parent[x]];
            x = self.parent[x];
        }
        x
    }

    fn union(&mut self, a: usize, b: usize) -> bool {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra == rb {
            return false;
        }
        let (lo, hi) = if ra < rb { (ra, rb) } else { (rb, ra) };
        self.parent[hi] = lo;
        true
    }

    pub fn congruent(&mut self, a: usize, b: usize) -> bool {
        self.find(a) == self.find(b)
    }

    pub fn add_consistent(&mut self, universe: &Universe, lhs: &LTerm, rhs: &LTerm, atoms: &[usize]) -> bool {
        let saved = self.parent.clone();
        self.add_law(universe, lhs, rhs);
        let mut roots: Vec<usize> = atoms.iter().map(|a| self.find(*a)).collect();
        roots.sort_unstable();
        roots.dedup();
        if roots.len() == atoms.len() {
            return true;
        }
        self.parent = saved;
        false
    }

    pub fn add_laws<'a>(&mut self, universe: &Universe, laws: impl IntoIterator<Item = (&'a LTerm, &'a LTerm)>) {
        for (lhs, rhs) in laws {
            self.instantiate(universe, lhs, rhs);
        }
        self.saturate(universe);
    }

    pub fn add_law(&mut self, universe: &Universe, lhs: &LTerm, rhs: &LTerm) {
        self.instantiate(universe, lhs, rhs);
        self.saturate(universe);
    }

    fn instantiate(&mut self, universe: &Universe, lhs: &LTerm, rhs: &LTerm) {
        let width = 1 + usize::from(max_var(lhs).max(max_var(rhs)));
        for (from, to) in [(lhs, rhs), (rhs, lhs)] {
            for id in 0..universe.len() {
                let mut sigma = vec![None; width];
                if universe.matches(from, id, &mut sigma)
                    && let Some(other) = universe.instantiate(to, &sigma)
                {
                    self.union(id, other);
                }
            }
        }
    }

    fn saturate(&mut self, universe: &Universe) {
        loop {
            let mut changed = false;
            let mut seen: HashMap<(Head, Vec<usize>), usize> = HashMap::new();
            for id in 0..universe.len() {
                let n = &universe.nodes[id];
                if n.args.is_empty() {
                    continue;
                }
                let key = (n.head.clone(), n.args.iter().map(|a| self.find(*a)).collect());
                match seen.get(&key) {
                    Some(other) => changed |= self.union(id, *other),
                    None => {
                        seen.insert(key, id);
                    }
                }
            }
            if !changed {
                return;
            }
        }
    }
}

fn max_var(t: &LTerm) -> u8 {
    match t {
        LTerm::Var(v) => *v,
        LTerm::Op(_, args) => args.iter().map(max_var).max().unwrap_or(0),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fingerprinter {
    pub assignments: Vec<Vec<u64>>,
    pub budget: Budget,
}

impl Fingerprinter {
    #[must_use]
    pub fn new(samples: usize, pool: u64, vars: u8, seed: u64, budget: Budget) -> Self {
        let assignments = (0..samples as u64)
            .map(|s| (0..u64::from(vars)).map(|v| mix(seed, s, v) % pool.max(1)).collect())
            .collect();
        Self { assignments, budget }
    }

    #[must_use]
    pub fn of(&self, t: &LTerm, defs: &[Term]) -> Option<Vec<u64>> {
        self.assignments
            .iter()
            .map(|a| observe_numeral(&t.kernel(a), defs, self.budget).ok().flatten())
            .collect()
    }
}

pub struct Explorer {
    reps: HashMap<Vec<u64>, Vec<usize>>,
}

impl Explorer {
    #[must_use]
    pub fn new() -> Self {
        Self { reps: HashMap::new() }
    }

    pub fn explore(
        &mut self,
        universe: &Universe,
        closure: &mut Closure,
        mut fingerprint: impl FnMut(usize, &LTerm) -> Option<Vec<u64>>,
        mut try_law: impl FnMut(&LTerm, &LTerm) -> bool,
        mut accepted: impl FnMut(&LTerm, &LTerm),
    ) {
        let leaves = universe.leaves();
        for id in 0..universe.len() {
            let t = universe.term(id);
            let Some(fp) = fingerprint(id, &t) else { continue };
            let reps = self.reps.entry(fp).or_default();
            let mut placed = false;
            for &r in reps.iter() {
                if closure.congruent(id, r) {
                    placed = true;
                    break;
                }
                let (lhs, rhs) = canonical(&t, &universe.term(r));
                if try_law(&lhs, &rhs) && closure.add_consistent(universe, &lhs, &rhs, &leaves) {
                    accepted(&lhs, &rhs);
                    placed = true;
                    break;
                }
            }
            if !placed {
                reps.push(id);
            }
        }
    }
}

impl Default for Explorer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add() -> Term {
        Term::lams(4, Term::apps(Term::Var(3), [Term::Var(1), Term::apps(Term::Var(2), [Term::Var(1), Term::Var(0)])]))
    }

    #[test]
    fn universe_terms_have_bounded_size_and_are_hash_consed() {
        let u = Universe::build(&[(0, 2), (1, 0)], 2, 3, 10_000);
        assert_eq!(u.len(), 3 + 9);
        assert!((0..u.len()).all(|i| u.term(i).size() <= 3));
    }

    #[test]
    fn exploration_finds_commutativity_associativity_and_identity_of_church_addition() {
        let defs = vec![add(), mathscape_kernel::numeral(0)];
        let u = Universe::build(&[(0, 2), (1, 0)], 3, 5, 50_000);
        let mut closure = Closure::new(&u);
        let fp = Fingerprinter::new(16, 5, 3, 1, Budget::default());
        let mut laws = Vec::new();
        Explorer::new().explore(&u, &mut closure, |_, t| fp.of(t, &defs), |_, _| true, |l, r| laws.push((l.clone(), r.clone())));
        let x = || LTerm::Var(0);
        let y = || LTerm::Var(1);
        let op = |a, b| LTerm::Op(0, vec![a, b]);
        assert!(laws.contains(&(op(y(), x()), op(x(), y()))) || laws.contains(&(op(x(), y()), op(y(), x()))), "{laws:?}");
        assert!(laws.iter().any(|(l, r)| *r == x() && (*l == op(x(), LTerm::Op(1, vec![])) || *l == op(LTerm::Op(1, vec![]), x()))), "{laws:?}");
        assert!(laws.iter().any(|(l, r)| l.size() == 5 && r.size() == 5), "associativity missing: {laws:?}");
        assert!(laws.len() <= 4, "pruning should keep the theory small: {laws:?}");
    }
}
