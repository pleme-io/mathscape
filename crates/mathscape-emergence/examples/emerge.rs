use mathscape_core::certification::{CertificationVerdict, Certifier};
use mathscape_core::eval::RewriteRule;
use mathscape_emergence::{Basis, EmergenceConfig, Engine};
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let epochs: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(40);
    let basis = if args.get(2).is_some_and(|s| s == "combinators") { Basis::Combinators } else { Basis::Lambda };

    let mut cfg = EmergenceConfig { basis, ..EmergenceConfig::default() };
    for kv in args.iter().skip(3) {
        let Some((k, v)) = kv.split_once('=') else { continue };
        let flag = v == "true";
        let n: usize = v.parse().unwrap_or(0);
        match k {
            "library_atoms" => cfg.library_atoms = flag,
            "generative" => cfg.generative = flag,
            "definable_filter" => cfg.definable_filter = flag,
            "max_laws_per_name" => cfg.max_laws_per_name = n,
            "law_universe_max" => cfg.law_universe_max = n,
            "names_per_epoch" => cfg.names_per_epoch = n,
            other => panic!("unknown override {other}"),
        }
    }
    eprintln!("{cfg:?}");
    let certifier = cfg.certifier(7);
    let mut engine = Engine::new(cfg, 7);
    let mut library: Vec<RewriteRule> = Vec::new();
    let started = Instant::now();
    println!("epoch\tseconds\tsize\tenumerated\tscreened\theld\tlaws\tevent");
    for epoch in 0..epochs {
        let mut gate = |rule: RewriteRule| {
            let ok = matches!(certifier.try_certify(&rule, &library), CertificationVerdict::Certified { .. });
            if ok {
                library.push(rule);
            }
            ok
        };
        let r = engine.step(&mut gate);
        let secs = started.elapsed().as_secs_f64();
        let events: Vec<String> = r.named.iter().chain(r.laws.iter()).cloned().collect();
        println!("{epoch}\t{secs:.2}\t{}\t{}\t{}\t{}\t{}\t{}", r.size, r.enumerated, r.screened, engine.held().len(), engine.laws().len(), events.join(" | "));
        if r.exhausted {
            break;
        }
    }
}
