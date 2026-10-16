use mathscape_emergence::enumerate::{Atom, Enumerator, Grammar, combinators, lambda};
use mathscape_kernel::{Budget, Term, numeral, observe_numeral};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

const TARGETS: [(&str, fn(u64, u64) -> u64); 4] = [
    ("add", |m, n| m + n),
    ("mul", |m, n| m * n),
    ("m^n", |m, n| m.pow(n as u32)),
    ("n^m", |m, n| n.pow(m as u32)),
];

fn v(i: u32) -> Term {
    Term::Var(i)
}

fn table(t: &Term, defs: &[Term], budget: Budget) -> Option<Vec<u64>> {
    let mut out = Vec::with_capacity(16);
    for m in 0..4 {
        for n in 0..4 {
            let r = observe_numeral(&Term::apps(t.clone(), [numeral(m), numeral(n)]), defs, budget).ok()??;
            out.push(r);
        }
    }
    Some(out)
}

fn iota_trees(size: usize, memo: &mut HashMap<usize, Rc<Vec<Term>>>) -> Rc<Vec<Term>> {
    if let Some(v) = memo.get(&size) {
        return v.clone();
    }
    let mut out = Vec::new();
    if size == 1 {
        out.push(Term::Sym(0));
    }
    for fs in 1..size.saturating_sub(1) {
        let fs_terms = iota_trees(fs, memo);
        let a_terms = iota_trees(size - 1 - fs, memo);
        for f in fs_terms.iter() {
            for a in a_terms.iter() {
                out.push(Term::app(f.clone(), a.clone()));
            }
        }
    }
    let out = Rc::new(out);
    memo.insert(size, out.clone());
    out
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let basis = args.get(1).map_or("lambda", String::as_str);
    let max: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(11);
    let budget = Budget {
        fuel: 4_000,
        max_nodes: 512,
        max_depth: 512,
    };
    let s = Term::lams(3, Term::apps(v(2), [v(0), Term::app(v(1), v(0))]));
    let k = Term::lams(2, v(1));
    let succ = Term::lams(3, Term::app(v(1), Term::apps(v(2), [v(1), v(0)])));
    let iter = Term::lams(3, Term::apps(v(2), [v(1), v(0)]));
    let (defs, atoms): (Vec<Term>, Vec<Term>) = match basis {
        "sk" => (vec![s, k], Vec::new()),
        "iota" => (vec![Term::lam(Term::apps(v(0), [s, k]))], Vec::new()),
        "lambda+nat" => (vec![numeral(0), succ, iter], vec![Term::Sym(0), Term::Sym(1), Term::Sym(2)]),
        _ => (Vec::new(), Vec::new()),
    };
    let grammar = match basis {
        "sk" => combinators(0, 1),
        _ => Grammar {
            atoms: atoms.into_iter().map(|term| Atom { term, cap: None }).collect(),
            ..lambda()
        },
    };
    let enumerator = Enumerator::new(grammar);
    let mut memo = HashMap::new();
    let mut cumulative: u64 = 0;
    let mut found: HashMap<&str, (usize, u64, String)> = HashMap::new();
    let wanted: Vec<(&str, Vec<u64>)> = TARGETS
        .iter()
        .map(|(name, f)| (*name, (0..4).flat_map(|m| (0..4).map(move |n| f(m, n))).collect()))
        .collect();
    println!("basis\tsize\tterms\tcumulative\tseconds\ttarget\tfirst_index\twitness");
    for size in 1..=max {
        let started = Instant::now();
        let terms: Rc<Vec<Term>> = match basis {
            "iota" => iota_trees(size, &mut memo),
            _ => Rc::new((0..enumerator.count(size)).filter_map(|i| enumerator.term(size, i)).collect()),
        };
        let mut hits = Vec::new();
        for (i, t) in terms.iter().enumerate() {
            let Some(tab) = table(t, &defs, budget) else { continue };
            for (name, want) in &wanted {
                if !found.contains_key(name) && &tab == want {
                    let idx = cumulative + i as u64 + 1;
                    found.insert(name, (size, idx, t.to_string()));
                    hits.push((*name, idx, t.to_string()));
                }
            }
        }
        cumulative += terms.len() as u64;
        let secs = started.elapsed().as_secs_f64();
        if hits.is_empty() {
            println!("{basis}\t{size}\t{}\t{cumulative}\t{secs:.3}\t\t\t", terms.len());
        }
        for (name, idx, w) in hits {
            println!("{basis}\t{size}\t{}\t{cumulative}\t{secs:.3}\t{name}\t{idx}\t{w}", terms.len());
        }
        if found.len() == wanted.len() {
            break;
        }
    }
}
