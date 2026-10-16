use crate::behavior::{Arity, behaviour, operator_shaped};
use crate::certify::{KernelCertifier, definition_symbol, to_core, to_kernel};
use crate::enumerate::{Atom, Enumerator, Grammar, combinators, lambda};
use crate::law::{Closure, Explorer, Fingerprinter, LTerm, Universe};
use mathscape_core::certification::{CertificationVerdict, Certifier};
use mathscape_core::eval::RewriteRule;
use mathscape_core::term::Term as Core;
use mathscape_kernel::{Budget, Term, observe_numeral};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Basis {
    #[default]
    Lambda,
    Combinators,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct EmergenceConfig {
    pub basis: Basis,
    pub terms_per_epoch: u64,
    pub max_term_size: usize,
    pub max_constants: usize,
    pub names_per_epoch: usize,
    pub min_laws: usize,
    pub min_image_unary: usize,
    pub min_image_binary: usize,
    pub library_atoms: bool,
    pub definable_filter: bool,
    pub generative: bool,
    pub max_laws_per_name: usize,
    pub law_max_size: usize,
    pub law_universe_max: usize,
    pub law_vars: u8,
    pub fingerprint_samples: usize,
    pub fingerprint_pool: u64,
    pub certify_samples: usize,
    pub certify_pool: u64,
    pub certify_exhaustive: u64,
    pub fuel: u64,
    pub max_nodes: usize,
    pub max_depth: usize,
}

impl Default for EmergenceConfig {
    fn default() -> Self {
        Self {
            basis: Basis::Lambda,
            terms_per_epoch: 20_000,
            max_term_size: 24,
            max_constants: 2,
            names_per_epoch: 1,
            min_laws: 2,
            min_image_unary: 6,
            min_image_binary: 4,
            library_atoms: false,
            definable_filter: true,
            generative: true,
            max_laws_per_name: 16,
            law_max_size: 5,
            law_universe_max: 20_000,
            law_vars: 3,
            fingerprint_samples: 16,
            fingerprint_pool: 5,
            certify_samples: 64,
            certify_pool: 7,
            certify_exhaustive: 4_096,
            fuel: 20_000,
            max_nodes: 4_096,
            max_depth: 1_024,
        }
    }
}

impl EmergenceConfig {
    #[must_use]
    pub fn budget(&self) -> Budget {
        Budget {
            fuel: self.fuel,
            max_nodes: self.max_nodes,
            max_depth: self.max_depth,
        }
    }

    #[must_use]
    pub fn certifier(&self, seed: u64) -> KernelCertifier {
        KernelCertifier {
            exhaustive: self.certify_exhaustive,
            samples: self.certify_samples,
            pool: self.certify_pool,
            seed: seed ^ 0xCE27_1F1E,
            budget: self.budget(),
        }
    }

    #[must_use]
    pub fn grammar(&self) -> Grammar {
        match self.basis {
            Basis::Lambda => lambda(),
            Basis::Combinators => combinators(0, 1),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Basis,
    Constant,
    Operator(Arity),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Held {
    pub term: Term,
    pub kind: Kind,
    pub table: Vec<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StepReport {
    pub enumerated: u64,
    pub screened: usize,
    pub named: Vec<String>,
    pub laws: Vec<String>,
    pub size: usize,
    pub exhausted: bool,
}

pub const META_SIZE: &str = "stream.emergence.size";
pub const META_INDEX: &str = "stream.emergence.index";
pub const META_FAILED: &str = "stream.emergence.failed";

pub struct Engine {
    cfg: EmergenceConfig,
    certifier: KernelCertifier,
    fingerprinter: Fingerprinter,
    enumerator: Enumerator,
    held: Vec<Held>,
    laws: Vec<(LTerm, LTerm)>,
    size: usize,
    index: u128,
    failed: BTreeSet<String>,
    definable: HashMap<Arity, BTreeSet<Vec<u64>>>,
    prepared: HashMap<Arity, Prepared>,
}

struct Prepared {
    universe: Universe,
    closure: Closure,
    mentions: Vec<bool>,
    prints: Vec<Option<Option<Vec<u64>>>>,
}

fn symbol_name(k: u32, kind: Kind) -> String {
    match kind {
        Kind::Basis => format!("k{k}"),
        Kind::Constant => format!("c{k}"),
        Kind::Operator(Arity::Unary) => format!("u{k}"),
        Kind::Operator(_) => format!("b{k}"),
    }
}

#[must_use]
pub fn render(t: &LTerm) -> String {
    match t {
        LTerm::Var(v) => ["x", "y", "z", "w"].get(*v as usize).map_or_else(|| format!("v{v}"), |s| (*s).to_string()),
        LTerm::Op(k, args) if args.is_empty() => format!("s{k}"),
        LTerm::Op(k, args) => {
            let mut s = format!("(s{k}");
            for a in args {
                let _ = write!(s, " {}", render(a));
            }
            s.push(')');
            s
        }
    }
}

#[must_use]
pub fn law_rule(lhs: &LTerm, rhs: &LTerm) -> RewriteRule {
    RewriteRule {
        name: format!("law {} = {}", render(lhs), render(rhs)),
        lhs: lhs.core(),
        rhs: rhs.core(),
    }
}

#[must_use]
pub fn definition_rule(k: u32, kind: Kind, term: &Term) -> RewriteRule {
    RewriteRule {
        name: format!("def {}", symbol_name(k, kind)),
        lhs: Core::Symbol(k, Vec::new()),
        rhs: to_core(term),
    }
}

fn kind_of(name: &str) -> Option<Kind> {
    let tag = name.strip_prefix("def ")?;
    match tag.chars().next()? {
        'k' => Some(Kind::Basis),
        'c' => Some(Kind::Constant),
        'u' => Some(Kind::Operator(Arity::Unary)),
        'b' => Some(Kind::Operator(Arity::Binary)),
        _ => None,
    }
}

fn variables(t: &LTerm, out: &mut BTreeSet<u8>) {
    match t {
        LTerm::Var(v) => {
            out.insert(*v);
        }
        LTerm::Op(_, args) => args.iter().for_each(|a| variables(a, out)),
    }
}

#[must_use]
pub fn defines(k: u32, head: &LTerm, body: &LTerm) -> bool {
    let LTerm::Op(op, args) = head else { return false };
    let mut params = BTreeSet::new();
    let distinct = args.iter().all(|a| matches!(a, LTerm::Var(v) if params.insert(*v)));
    let mut used = BTreeSet::new();
    variables(body, &mut used);
    *op == k && distinct && !body.mentions(k) && used.is_subset(&params)
}

fn image(table: &[u64]) -> usize {
    table.iter().collect::<BTreeSet<_>>().len()
}

fn digest(arity: Arity, table: &[u64]) -> String {
    let mut h = blake3::Hasher::new();
    h.update(&[arity as u8]);
    for v in table {
        h.update(&v.to_le_bytes());
    }
    h.finalize().to_hex()[..16].to_string()
}

impl Engine {
    #[must_use]
    pub fn new(cfg: EmergenceConfig, seed: u64) -> Self {
        let budget = cfg.budget();
        Self {
            certifier: cfg.certifier(seed),
            fingerprinter: Fingerprinter::new(cfg.fingerprint_samples, cfg.fingerprint_pool, cfg.law_vars, seed, budget),
            enumerator: Enumerator::new(cfg.grammar()),
            cfg,
            held: Vec::new(),
            laws: Vec::new(),
            size: 1,
            index: 0,
            failed: BTreeSet::new(),
            definable: HashMap::new(),
            prepared: HashMap::new(),
        }
    }

    pub fn resume(
        cfg: EmergenceConfig,
        seed: u64,
        library: &[RewriteRule],
        meta: impl Fn(&str) -> Option<String>,
    ) -> Result<Self, String> {
        let mut e = Self::new(cfg, seed);
        for rule in library {
            if definition_symbol(rule) == Some(e.held.len() as u32) {
                let kind = kind_of(&rule.name).ok_or_else(|| format!("definition `{}` names no kind", rule.name))?;
                let term = to_kernel(&rule.rhs, &BTreeMap::new()).map_err(|o| o.to_string())?;
                e.hold(term, kind);
            } else {
                let lhs = LTerm::from_core(&rule.lhs).ok_or_else(|| format!("law `{}` is outside the law language", rule.name))?;
                let rhs = LTerm::from_core(&rule.rhs).ok_or_else(|| format!("law `{}` is outside the law language", rule.name))?;
                e.laws.push((lhs, rhs));
            }
        }
        let parse = |key: &str| meta(key).map(|v| v.parse::<u128>().map_err(|_| format!("{key}: {v}"))).transpose();
        if let Some(size) = parse(META_SIZE)? {
            e.size = size as usize;
        }
        if let Some(index) = parse(META_INDEX)? {
            e.index = index;
        }
        if let Some(list) = meta(META_FAILED) {
            e.failed = list.split(',').filter(|s| !s.is_empty()).map(str::to_string).collect();
        }
        Ok(e)
    }

    #[must_use]
    pub fn state(&self) -> Vec<(&'static str, String)> {
        vec![
            (META_SIZE, self.size.to_string()),
            (META_INDEX, self.index.to_string()),
            (META_FAILED, self.failed.iter().cloned().collect::<Vec<_>>().join(",")),
        ]
    }

    #[must_use]
    pub fn held(&self) -> &[Held] {
        &self.held
    }

    #[must_use]
    pub fn laws(&self) -> &[(LTerm, LTerm)] {
        &self.laws
    }

    #[must_use]
    pub fn cursor(&self) -> (usize, u128) {
        (self.size, self.index)
    }

    #[must_use]
    pub fn config(&self) -> &EmergenceConfig {
        &self.cfg
    }

    fn defs(&self) -> Vec<Term> {
        self.held.iter().map(|h| h.term.clone()).collect()
    }

    fn hold(&mut self, term: Term, kind: Kind) {
        let defs = {
            let mut d = self.defs();
            d.push(term.clone());
            d
        };
        let sym = Term::Sym(self.held.len() as u32);
        let table = match kind {
            Kind::Basis => Vec::new(),
            Kind::Constant => behaviour(&sym, Arity::Nullary, &defs, self.cfg.budget()).unwrap_or_default(),
            Kind::Operator(a) => behaviour(&sym, a, &defs, self.cfg.budget()).unwrap_or_default(),
        };
        self.held.push(Held { term, kind, table });
        self.prepared.clear();
        self.definable.clear();
        if self.cfg.library_atoms && kind != Kind::Basis {
            let mut grammar = self.cfg.grammar();
            grammar.atoms.extend(self.signature().into_iter().map(|(k, _)| Atom { term: Term::Sym(k), cap: None }));
            self.enumerator = Enumerator::new(grammar);
            self.size = 1;
            self.index = 0;
        }
    }

    fn generates(&self, table: &[u64], arity: Arity) -> bool {
        let constants: BTreeSet<u64> = self.held.iter().filter(|h| h.kind == Kind::Constant).flat_map(|h| h.table.iter().copied()).collect();
        let n = arity.grid();
        table.iter().enumerate().any(|(i, v)| {
            let args: Vec<u64> = match arity {
                Arity::Unary => vec![i as u64],
                _ => vec![i as u64 / n, i as u64 % n],
            };
            !args.contains(v) && !constants.contains(v)
        })
    }

    fn definable(&mut self, arity: Arity) -> &BTreeSet<Vec<u64>> {
        if !self.definable.contains_key(&arity) {
            let universe = Universe::build(&self.signature(), arity.count() as u8, self.cfg.law_max_size, self.cfg.law_universe_max);
            let defs = self.defs();
            let budget = self.cfg.budget();
            let grid: Vec<Vec<u64>> = match arity {
                Arity::Unary => (0..arity.grid()).map(|x| vec![x]).collect(),
                _ => (0..arity.grid()).flat_map(|x| (0..arity.grid()).map(move |y| vec![x, y])).collect(),
            };
            let tables = (0..universe.len())
                .filter_map(|id| {
                    let t = universe.term(id);
                    grid.iter()
                        .map(|a| observe_numeral(&t.kernel(a), &defs, budget).ok().flatten())
                        .collect::<Option<Vec<u64>>>()
                })
                .collect();
            self.definable.insert(arity, tables);
        }
        &self.definable[&arity]
    }

    fn context(&self) -> Vec<RewriteRule> {
        self.held
            .iter()
            .enumerate()
            .map(|(k, h)| definition_rule(k as u32, h.kind, &h.term))
            .collect()
    }

    fn signature(&self) -> Vec<(u32, usize)> {
        self.held
            .iter()
            .enumerate()
            .filter_map(|(k, h)| match h.kind {
                Kind::Basis => None,
                Kind::Constant => Some((k as u32, 0)),
                Kind::Operator(a) => Some((k as u32, a.count())),
            })
            .collect()
    }

    fn constants(&self) -> usize {
        self.held.iter().filter(|h| h.kind == Kind::Constant).count()
    }

    pub fn step(&mut self, gate: &mut dyn FnMut(RewriteRule) -> bool) -> StepReport {
        let mut report = StepReport::default();
        if self.cfg.basis == Basis::Combinators && self.held.is_empty() {
            let s = Term::lams(3, Term::apps(Term::Var(2), [Term::Var(0), Term::app(Term::Var(1), Term::Var(0))]));
            let k = Term::lams(2, Term::Var(1));
            for t in [s, k] {
                let rule = definition_rule(self.held.len() as u32, Kind::Basis, &t);
                if !gate(rule) {
                    return report;
                }
                self.hold(t, Kind::Basis);
            }
        }
        while report.enumerated < self.cfg.terms_per_epoch && report.named.len() < self.cfg.names_per_epoch.max(1) {
            if self.size > self.cfg.max_term_size {
                report.exhausted = true;
                break;
            }
            report.size = self.size;
            let Some(t) = self.enumerator.term(self.size, self.index) else {
                self.size += 1;
                self.index = 0;
                continue;
            };
            self.index += 1;
            report.enumerated += 1;
            self.consider(&t, gate, &mut report);
        }
        report
    }

    fn consider(&mut self, t: &Term, gate: &mut dyn FnMut(RewriteRule) -> bool, report: &mut StepReport) {
        let defs = self.defs();
        let budget = self.cfg.budget();
        if self.constants() < self.cfg.max_constants
            && let Ok(Some(v)) = observe_numeral(t, &defs, budget)
            && !self.held.iter().any(|h| h.kind == Kind::Constant && h.table == [v])
        {
            let k = self.held.len() as u32;
            if gate(definition_rule(k, Kind::Constant, t)) {
                self.hold(t.clone(), Kind::Constant);
                report.named.push(format!("{} = {v}", symbol_name(k, Kind::Constant)));
            }
            return;
        }
        for arity in [Arity::Unary, Arity::Binary] {
            let Some(table) = behaviour(t, arity, &defs, budget) else { continue };
            let floor = match arity {
                Arity::Unary => self.cfg.min_image_unary,
                _ => self.cfg.min_image_binary,
            };
            if !operator_shaped(&table, arity)
                || image(&table) < floor
                || (self.cfg.generative && !self.generates(&table, arity))
                || self.held.iter().any(|h| h.kind == Kind::Operator(arity) && h.table == table) {
                continue;
            }
            let key = digest(arity, &table);
            if self.failed.contains(&key) {
                continue;
            }
            if self.cfg.definable_filter && self.definable(arity).contains(&table) {
                self.failed.insert(key);
                continue;
            }
            report.screened += 1;
            if self.screen(t, arity, gate, report) {
                return;
            }
            self.failed.insert(key);
        }
    }

    fn prepare(&mut self, k: u32, arity: Arity) -> Prepared {
        if let Some(p) = self.prepared.remove(&arity) {
            return p;
        }
        let mut signature = self.signature();
        signature.insert(0, (k, arity.count()));
        let universe = Universe::build(&signature, self.cfg.law_vars, self.cfg.law_max_size, self.cfg.law_universe_max);
        let mut closure = Closure::new(&universe);
        closure.add_laws(&universe, self.laws.iter().map(|(l, r)| (l, r)));
        let mentions = (0..universe.len()).map(|i| universe.term(i).mentions(k)).collect();
        let prints = vec![None; universe.len()];
        Prepared { universe, closure, mentions, prints }
    }

    fn screen(&mut self, t: &Term, arity: Arity, gate: &mut dyn FnMut(RewriteRule) -> bool, report: &mut StepReport) -> bool {
        let k = self.held.len() as u32;
        let kind = Kind::Operator(arity);
        let mut defs = self.defs();
        defs.push(t.clone());
        let mut prepared = self.prepare(k, arity);
        let mut closure = prepared.closure.clone();
        let mut context = self.context();
        context.push(definition_rule(k, kind, t));
        let mut found = Vec::new();
        let certifier = &self.certifier;
        let fingerprinter = &self.fingerprinter;
        let mentions = &prepared.mentions;
        let prints = &mut prepared.prints;
        let reducible = std::cell::Cell::new(false);
        Explorer::new().explore(
            &prepared.universe,
            &mut closure,
            |id, lt| {
                if reducible.get() {
                    return None;
                }
                if mentions[id] {
                    return fingerprinter.of(lt, &defs);
                }
                prints[id].get_or_insert_with(|| fingerprinter.of(lt, &defs)).clone()
            },
            |l, r| {
                (l.mentions(k) || r.mentions(k))
                    && !(l.ground() && r.ground())
                    && matches!(certifier.try_certify(&law_rule(l, r), &context), CertificationVerdict::Certified { .. })
            },
            |l, r| {
                reducible.set(reducible.get() || defines(k, l, r) || defines(k, r, l));
                found.push((l.clone(), r.clone()));
            },
        );
        self.prepared.insert(arity, prepared);
        if reducible.get() {
            return false;
        }
        if found.iter().filter(|(l, r)| l.mentions(k) || r.mentions(k)).count() < self.cfg.min_laws {
            return false;
        }
        if !gate(definition_rule(k, kind, t)) {
            return false;
        }
        self.hold(t.clone(), kind);
        report.named.push(format!("{} := {t}", symbol_name(k, kind)));
        for (l, r) in found.into_iter().take(self.cfg.max_laws_per_name.max(self.cfg.min_laws)) {
            let rule = law_rule(&l, &r);
            let name = rule.name.clone();
            if gate(rule) {
                self.laws.push((l, r));
                report.laws.push(name);
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::certify::KernelCertifier;

    fn run(cfg: EmergenceConfig, epochs: usize) -> (Engine, Vec<RewriteRule>) {
        let certifier = KernelCertifier { ..cfg.certifier(7) };
        let mut engine = Engine::new(cfg, 7);
        let mut library: Vec<RewriteRule> = Vec::new();
        for _ in 0..epochs {
            let mut gate = |rule: RewriteRule| {
                let ok = matches!(certifier.try_certify(&rule, &library), CertificationVerdict::Certified { .. });
                if ok {
                    library.push(rule);
                }
                ok
            };
            engine.step(&mut gate);
        }
        (engine, library)
    }

    fn small() -> EmergenceConfig {
        EmergenceConfig {
            library_atoms: false,
            law_universe_max: 4_000,
            ..EmergenceConfig::default()
        }
    }

    #[test]
    fn multiplication_and_its_commutativity_emerge_from_lambda_terms_alone() {
        let (engine, library) = run(small(), 8);
        let product: Vec<u64> = (0..4).flat_map(|m| (0..4).map(move |n| m * n)).collect();
        let k = engine
            .held()
            .iter()
            .position(|h| h.kind == Kind::Operator(Arity::Binary) && h.table == product)
            .expect("a held operator multiplies") as u32;
        let xy = LTerm::Op(k, vec![LTerm::Var(0), LTerm::Var(1)]).core();
        let yx = LTerm::Op(k, vec![LTerm::Var(1), LTerm::Var(0)]).core();
        let swapped = |r: &RewriteRule| (r.lhs == xy && r.rhs == yx) || (r.lhs == yx && r.rhs == xy);
        assert!(library.iter().any(swapped), "{:?}", library.iter().map(|r| &r.name).collect::<Vec<_>>());
        assert!(library.iter().all(|r| !r.name.contains("builtin")));
    }

    #[test]
    fn an_engine_resumed_from_its_library_and_state_continues_identically() {
        let (mut whole, _) = run(small(), 6);
        let (half, library) = run(small(), 4);
        let state: BTreeMap<&str, String> = half.state().into_iter().collect();
        let mut resumed = Engine::resume(small(), 7, &library, |k| state.get(k).cloned()).unwrap();
        assert_eq!(resumed.cursor(), half.cursor());
        assert_eq!(resumed.held(), half.held());
        let mut lib = library.clone();
        let certifier = small().certifier(7);
        for _ in 0..2 {
            let mut gate = |rule: RewriteRule| {
                let ok = matches!(certifier.try_certify(&rule, &lib), CertificationVerdict::Certified { .. });
                if ok {
                    lib.push(rule);
                }
                ok
            };
            resumed.step(&mut gate);
        }
        let mut gate = |_: RewriteRule| true;
        let _ = whole.step(&mut gate);
        assert_eq!(resumed.held()[..], whole.held()[..resumed.held().len()]);
    }
}
