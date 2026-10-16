#![forbid(unsafe_code)]

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Term {
    Var(u32),
    Lam(Rc<Term>),
    App(Rc<Term>, Rc<Term>),
    Sym(u32),
}

impl Term {
    #[must_use]
    pub fn lam(body: Term) -> Self {
        Self::Lam(Rc::new(body))
    }

    #[must_use]
    pub fn app(f: Term, a: Term) -> Self {
        Self::App(Rc::new(f), Rc::new(a))
    }

    #[must_use]
    pub fn apps(f: Term, args: impl IntoIterator<Item = Term>) -> Self {
        args.into_iter().fold(f, Self::app)
    }

    #[must_use]
    pub fn lams(n: usize, body: Term) -> Self {
        (0..n).fold(body, |b, _| Self::lam(b))
    }

    #[must_use]
    pub fn size(&self) -> usize {
        match self {
            Self::Var(_) | Self::Sym(_) => 1,
            Self::Lam(b) => 1 + b.size(),
            Self::App(f, a) => 1 + f.size() + a.size(),
        }
    }

    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.closed_under(0)
    }

    fn closed_under(&self, depth: u32) -> bool {
        match self {
            Self::Var(i) => *i < depth,
            Self::Sym(_) => true,
            Self::Lam(b) => b.closed_under(depth + 1),
            Self::App(f, a) => f.closed_under(depth) && a.closed_under(depth),
        }
    }

    #[must_use]
    pub fn leading_lambdas(&self) -> usize {
        match self {
            Self::Lam(b) => 1 + b.leading_lambdas(),
            _ => 0,
        }
    }

    pub fn symbols(&self, out: &mut Vec<u32>) {
        match self {
            Self::Var(_) => {}
            Self::Sym(k) => out.push(*k),
            Self::Lam(b) => b.symbols(out),
            Self::App(f, a) => {
                f.symbols(out);
                a.symbols(out);
            }
        }
    }

    #[must_use]
    pub fn uses_symbols(&self) -> bool {
        match self {
            Self::Var(_) => false,
            Self::Sym(_) => true,
            Self::Lam(b) => b.uses_symbols(),
            Self::App(f, a) => f.uses_symbols() || a.uses_symbols(),
        }
    }
}

impl fmt::Display for Term {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Var(i) => write!(f, "{i}"),
            Self::Sym(k) => write!(f, "#{k}"),
            Self::Lam(b) => write!(f, "(λ {b})"),
            Self::App(g, a) => {
                let mut spine = vec![a.as_ref()];
                let mut head = g.as_ref();
                while let Self::App(h, x) = head {
                    spine.push(x.as_ref());
                    head = h.as_ref();
                }
                write!(f, "({head}")?;
                for x in spine.iter().rev() {
                    write!(f, " {x}")?;
                }
                write!(f, ")")
            }
        }
    }
}

#[must_use]
pub fn numeral(k: u64) -> Term {
    let body = (0..k).fold(Term::Var(0), |acc, _| Term::app(Term::Var(1), acc));
    Term::lams(2, body)
}

#[must_use]
pub fn as_numeral(t: &Term) -> Option<u64> {
    let Term::Lam(outer) = t else { return None };
    let Term::Lam(body) = outer.as_ref() else { return None };
    let mut k = 0;
    let mut cur = body.as_ref();
    loop {
        match cur {
            Term::Var(0) => return Some(k),
            Term::App(f, a) if matches!(f.as_ref(), Term::Var(1)) => {
                k += 1;
                cur = a.as_ref();
            }
            _ => return None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Halt {
    Fuel,
    Size,
    Depth,
    Open,
    Unbound(u32),
}

impl fmt::Display for Halt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fuel => write!(f, "fuel exhausted"),
            Self::Size => write!(f, "normal form exceeds the node budget"),
            Self::Depth => write!(f, "evaluation nests deeper than the depth budget"),
            Self::Open => write!(f, "open term"),
            Self::Unbound(k) => write!(f, "symbol #{k} is not held"),
        }
    }
}

impl std::error::Error for Halt {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budget {
    pub fuel: u64,
    pub max_nodes: usize,
    pub max_depth: usize,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            fuel: 20_000,
            max_nodes: 4_096,
            max_depth: 1_024,
        }
    }
}

enum Val {
    Lam(Env, Rc<Term>),
    Neu(u32, Vec<Thunk>),
}

type Env = Option<Rc<Frame>>;

struct Frame {
    head: Thunk,
    tail: Env,
}

enum Slot {
    Delayed(Env, Rc<Term>),
    Ready(Rc<Val>),
}

type Thunk = Rc<RefCell<Slot>>;

fn push(env: &Env, head: Thunk) -> Env {
    Some(Rc::new(Frame { head, tail: env.clone() }))
}

fn ready(v: Rc<Val>) -> Thunk {
    Rc::new(RefCell::new(Slot::Ready(v)))
}

struct Depth<'m>(&'m Cell<usize>);

impl Drop for Depth<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

struct Machine<'d> {
    defs: &'d [Term],
    fuel: Cell<u64>,
    nodes: Cell<usize>,
    depth: Cell<usize>,
    cache: RefCell<Vec<Option<Rc<Val>>>>,
}

impl<'d> Machine<'d> {
    fn new(defs: &'d [Term], budget: Budget) -> Self {
        Self {
            defs,
            fuel: Cell::new(budget.fuel),
            nodes: Cell::new(budget.max_nodes),
            depth: Cell::new(budget.max_depth),
            cache: RefCell::new(vec![None; defs.len()]),
        }
    }

    fn tick(&self) -> Result<(), Halt> {
        let f = self.fuel.get();
        if f == 0 {
            return Err(Halt::Fuel);
        }
        self.fuel.set(f - 1);
        Ok(())
    }

    fn node(&self) -> Result<(), Halt> {
        let n = self.nodes.get();
        if n == 0 {
            return Err(Halt::Size);
        }
        self.nodes.set(n - 1);
        Ok(())
    }

    fn force(&self, th: &Thunk) -> Result<Rc<Val>, Halt> {
        let (env, term) = match &*th.borrow() {
            Slot::Ready(v) => return Ok(v.clone()),
            Slot::Delayed(env, term) => (env.clone(), term.clone()),
        };
        let v = self.eval(&env, &term)?;
        *th.borrow_mut() = Slot::Ready(v.clone());
        Ok(v)
    }

    fn lookup(env: &Env, index: u32) -> Result<Thunk, Halt> {
        let mut cur = env.as_ref();
        for _ in 0..index {
            cur = cur.and_then(|f| f.tail.as_ref());
        }
        cur.map(|f| f.head.clone()).ok_or(Halt::Open)
    }

    fn symbol(&self, k: u32) -> Result<Rc<Val>, Halt> {
        if let Some(Some(v)) = self.cache.borrow().get(k as usize) {
            return Ok(v.clone());
        }
        let def = Rc::new(self.defs.get(k as usize).ok_or(Halt::Unbound(k))?.clone());
        self.tick()?;
        let v = self.eval(&None, &def)?;
        self.cache.borrow_mut()[k as usize] = Some(v.clone());
        Ok(v)
    }

    fn enter(&self) -> Result<Depth<'_>, Halt> {
        let d = self.depth.get();
        if d == 0 {
            return Err(Halt::Depth);
        }
        self.depth.set(d - 1);
        Ok(Depth(&self.depth))
    }

    fn eval(&self, env: &Env, t: &Rc<Term>) -> Result<Rc<Val>, Halt> {
        let _guard = self.enter()?;
        let mut env = env.clone();
        let mut cur = t.clone();
        let mut args: Vec<Thunk> = Vec::new();
        loop {
            let head = match cur.as_ref() {
                Term::App(f, a) => {
                    args.push(Rc::new(RefCell::new(Slot::Delayed(env.clone(), a.clone()))));
                    cur = f.clone();
                    continue;
                }
                Term::Lam(b) => match args.pop() {
                    Some(arg) => {
                        self.tick()?;
                        env = push(&env, arg);
                        cur = b.clone();
                        continue;
                    }
                    None => return Ok(Rc::new(Val::Lam(env, b.clone()))),
                },
                Term::Var(i) => self.force(&Self::lookup(&env, *i)?)?,
                Term::Sym(k) => self.symbol(*k)?,
            };
            match head.as_ref() {
                Val::Lam(cenv, body) => match args.pop() {
                    Some(arg) => {
                        self.tick()?;
                        env = push(cenv, arg);
                        cur = body.clone();
                    }
                    None => return Ok(head),
                },
                Val::Neu(h, spine) => {
                    let mut spine = spine.clone();
                    spine.extend(args.drain(..).rev());
                    return Ok(Rc::new(Val::Neu(*h, spine)));
                }
            }
        }
    }

    fn quote(&self, level: u32, v: &Val) -> Result<Term, Halt> {
        let _guard = self.enter()?;
        self.node()?;
        match v {
            Val::Lam(env, body) => {
                let x = ready(Rc::new(Val::Neu(level, Vec::new())));
                let inner = self.eval(&push(env, x), body)?;
                Ok(Term::lam(self.quote(level + 1, &inner)?))
            }
            Val::Neu(h, spine) => {
                let mut acc = Term::Var(level - h - 1);
                for th in spine {
                    let arg = self.force(th)?;
                    acc = Term::app(acc, self.quote(level, &arg)?);
                }
                Ok(acc)
            }
        }
    }
}

pub fn normalize(t: &Term, defs: &[Term], budget: Budget) -> Result<Term, Halt> {
    if !t.is_closed() {
        return Err(Halt::Open);
    }
    let m = Machine::new(defs, budget);
    let v = m.eval(&None, &Rc::new(t.clone()))?;
    m.quote(0, &v)
}

#[must_use]
pub fn eta2(t: &Term) -> Term {
    Term::lams(2, Term::apps(t.clone(), [Term::Var(1), Term::Var(0)]))
}

pub fn observe(t: &Term, defs: &[Term], budget: Budget) -> Result<Term, Halt> {
    normalize(&eta2(t), defs, budget)
}

pub fn observe_numeral(t: &Term, defs: &[Term], budget: Budget) -> Result<Option<u64>, Halt> {
    observe(t, defs, budget).map(|nf| as_numeral(&nf))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(i: u32) -> Term {
        Term::Var(i)
    }

    fn add() -> Term {
        Term::lams(4, Term::apps(v(3), [v(1), Term::apps(v(2), [v(1), v(0)])]))
    }

    fn mul() -> Term {
        Term::lams(3, Term::app(v(2), Term::app(v(1), v(0))))
    }

    fn omega() -> Term {
        let w = Term::lam(Term::app(v(0), v(0)));
        Term::app(w.clone(), w)
    }

    #[test]
    fn numerals_round_trip() {
        for k in 0..20 {
            assert_eq!(as_numeral(&numeral(k)), Some(k));
        }
        assert_eq!(as_numeral(&Term::lam(v(0))), None);
    }

    #[test]
    fn church_addition_and_multiplication_normalize() {
        let b = Budget::default();
        for m in 0..6 {
            for n in 0..6 {
                let s = normalize(&Term::apps(add(), [numeral(m), numeral(n)]), &[], b).unwrap();
                assert_eq!(as_numeral(&s), Some(m + n));
                let p = normalize(&Term::apps(mul(), [numeral(m), numeral(n)]), &[], b).unwrap();
                assert_eq!(as_numeral(&p), Some(m * n));
            }
        }
    }

    #[test]
    fn observation_reads_eta_variants_as_numerals() {
        let b = Budget::default();
        assert_eq!(observe_numeral(&Term::lam(v(0)), &[], b), Ok(Some(1)));
        let exp = Term::lams(2, Term::app(v(0), v(1)));
        for m in 0..4 {
            for n in 0..4 {
                let r = observe_numeral(&Term::apps(exp.clone(), [numeral(m), numeral(n)]), &[], b).unwrap();
                assert_eq!(r, Some(m.pow(n as u32)));
            }
        }
    }

    #[test]
    fn divergence_halts_on_fuel_and_lazy_arguments_are_not_forced() {
        let b = Budget::default();
        assert_eq!(normalize(&omega(), &[], b), Err(Halt::Fuel));
        let k = Term::lams(2, v(1));
        let r = normalize(&Term::apps(k, [numeral(3), omega()]), &[], b).unwrap();
        assert_eq!(as_numeral(&r), Some(3));
    }

    #[test]
    fn symbols_unfold_from_held_definitions() {
        let defs = vec![add(), Term::lams(2, Term::apps(Term::Sym(0), [v(0), v(1)]))];
        let r = observe_numeral(&Term::apps(Term::Sym(1), [numeral(2), numeral(5)]), &defs, Budget::default());
        assert_eq!(r, Ok(Some(7)));
        assert_eq!(normalize(&Term::Sym(9), &defs, Budget::default()), Err(Halt::Unbound(9)));
    }

    #[test]
    fn open_terms_and_oversized_normal_forms_are_refused() {
        assert_eq!(normalize(&v(0), &[], Budget::default()), Err(Halt::Open));
        let tight = Budget { max_nodes: 10, ..Budget::default() };
        assert_eq!(normalize(&numeral(20), &[], tight), Err(Halt::Size));
    }
}
