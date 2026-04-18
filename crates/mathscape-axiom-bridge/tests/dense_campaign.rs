//! Dense discovery campaign — 10,000 individual traversal probes
//! with randomly-sampled parameters across every axis we control.
//!
//! Each probe samples (apparatus, corpus_family, seed, budget,
//! depth, extract_config, min_score, meta_on) uniformly-at-random
//! (reproducibly via xorshift) from the full parameter space, runs
//! a single traversal, and records every axiomatized rule's
//! PROVENANCE across all axes.
//!
//! Rules that appear across many distinct apparatus × corpus ×
//! config × scale cells are TRUE cross-dimensional universals.
//! Rules that appear in narrow slices are apparatus/corpus/config-
//! specific discoveries.
//!
//! At ~8-15ms per traversal in release mode, 10,000 probes complete
//! in 100-200s. This is the largest single discovery run the
//! machinery supports without structural changes.
//!
//! Invocation:
//!   MATHSCAPE_DENSE_PROBES=10000 cargo test -p mathscape-axiom-bridge \
//!     --release --test dense_campaign dense_campaign -- --ignored --nocapture
//!
//! Default probe count = 10,000. Override via MATHSCAPE_DENSE_PROBES
//! for faster smoke-test runs.

mod common;

use common::experiment::{run_one, rule_key, CorpusFamily};
use mathscape_compress::extract::ExtractConfig;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// A single probe configuration sampled from the parameter space.
#[derive(Clone, Debug)]
struct ProbeConfig {
    apparatus: &'static str,
    apparatus_src: &'static str,
    corpus: CorpusFamily,
    corpus_name: &'static str,
    seed: u64,
    budget: usize,
    depth: usize,
    extract_config: ExtractConfig,
    ec_name: &'static str,
    min_score: f64,
    use_meta_gen: bool,
}

/// Provenance bookkeeping for a single discovered rule across probes.
#[derive(Default)]
struct Provenance {
    total_runs: usize,
    apparatuses: BTreeSet<&'static str>,
    corpora: BTreeSet<&'static str>,
    ec_names: BTreeSet<&'static str>,
    budgets: BTreeSet<usize>,
    depths: BTreeSet<usize>,
    meta_states: BTreeSet<bool>,
}

fn xorshift(mut x: u64) -> u64 {
    if x == 0 {
        x = 0x9E37_79B9_7F4A_7C15;
    }
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    x
}

fn sample_config(iter: u64) -> ProbeConfig {
    let apparatuses: &[(&str, &str)] = &[
        ("canonical",     "(+ (* alpha cr) (* beta novelty) (* gamma meta-compression) (* delta lhs-subsumption))"),
        ("cr-only",       "(* alpha cr)"),
        ("novelty-only",  "(* beta novelty)"),
        ("meta-only",     "(* gamma meta-compression)"),
        ("sub-only",      "(* delta lhs-subsumption)"),
        ("cr+nov",        "(+ (* alpha cr) (* beta novelty))"),
        ("cr+meta",       "(+ (* alpha cr) (* gamma meta-compression))"),
        ("cr+sub",        "(+ (* alpha cr) (* delta lhs-subsumption))"),
        ("nov+meta",      "(+ (* beta novelty) (* gamma meta-compression))"),
        ("nov+sub",       "(+ (* beta novelty) (* delta lhs-subsumption))"),
        ("meta+sub",      "(+ (* gamma meta-compression) (* delta lhs-subsumption))"),
        ("max-pair",      "(max (+ (* alpha cr) (* beta novelty)) (+ (* gamma meta-compression) (* delta lhs-subsumption)))"),
        ("cr*nov",        "(+ (* cr novelty) (* gamma meta-compression) (* delta lhs-subsumption))"),
        ("harmonic",      "(+ (/ (* cr novelty) (max (+ cr novelty) 0.001)) (* gamma meta-compression) (* delta lhs-subsumption))"),
        ("clamped",       "(clamp (+ (* alpha cr) (* beta novelty) (* gamma meta-compression) (* delta lhs-subsumption)) -1 2)"),
        ("threshold-cr",  "(+ (* alpha cr) (if (max (- cr 0.05) 0) (* beta novelty) 0) (* gamma meta-compression) (* delta lhs-subsumption))"),
        ("alpha-x2",      "(+ (* (* 2 alpha) cr) (* beta novelty) (* gamma meta-compression) (* delta lhs-subsumption))"),
        ("beta-x2",       "(+ (* alpha cr) (* (* 2 beta) novelty) (* gamma meta-compression) (* delta lhs-subsumption))"),
        ("delta-x3",      "(+ (* alpha cr) (* beta novelty) (* gamma meta-compression) (* (* 3 delta) lhs-subsumption))"),
        ("uniform",       "(+ (* 0.25 cr) (* 0.25 novelty) (* 0.25 meta-compression) (* 0.25 lhs-subsumption))"),
        ("meta-heavy",    "(+ (* alpha cr) (* beta novelty) (* (* 5 gamma) meta-compression) (* delta lhs-subsumption))"),
    ];
    let corpora: &[(CorpusFamily, &str)] = &[
        (CorpusFamily::Procedural,        "procedural"),
        (CorpusFamily::ZooPlusProcedural, "zoo+proc"),
        (CorpusFamily::AsymmetricArith,   "asymmetric"),
        (CorpusFamily::DeeplyNested,      "nested"),
        (CorpusFamily::SuccessorChain,    "succ-chain"),
        (CorpusFamily::MixedOperators,    "mixed"),
    ];
    let extract_configs: &[(ExtractConfig, &str)] = &[
        (ExtractConfig { min_shared_size: 2, min_matches: 2, max_new_rules: 5 }, "ec-loose"),
        (ExtractConfig { min_shared_size: 3, min_matches: 2, max_new_rules: 5 }, "ec-default"),
        (ExtractConfig { min_shared_size: 4, min_matches: 2, max_new_rules: 5 }, "ec-strict"),
        (ExtractConfig { min_shared_size: 2, min_matches: 3, max_new_rules: 8 }, "ec-triple-match"),
        (ExtractConfig { min_shared_size: 3, min_matches: 2, max_new_rules: 12}, "ec-wide-emit"),
    ];
    let budgets = [8usize, 12, 16, 20, 24];
    let depths = [2usize, 3, 4, 5, 6];
    let min_scores = [0.0, 0.005, 0.02, 0.05];
    let meta_states = [true, false];

    let mut r = xorshift(iter.wrapping_mul(2654435761));
    let apparatus_i = (r as usize) % apparatuses.len();
    r = xorshift(r);
    let corpus_i = (r as usize) % corpora.len();
    r = xorshift(r);
    let seed = r % 100_000;
    r = xorshift(r);
    let budget = budgets[(r as usize) % budgets.len()];
    r = xorshift(r);
    let depth = depths[(r as usize) % depths.len()];
    r = xorshift(r);
    let ec_i = (r as usize) % extract_configs.len();
    r = xorshift(r);
    let min_score = min_scores[(r as usize) % min_scores.len()];
    r = xorshift(r);
    let meta_on = meta_states[(r as usize) % meta_states.len()];

    let (ap_label, ap_src) = apparatuses[apparatus_i];
    let (corp, corp_label) = corpora[corpus_i];
    let (ref ec, ec_label) = extract_configs[ec_i];

    ProbeConfig {
        apparatus: ap_label,
        apparatus_src: ap_src,
        corpus: corp,
        corpus_name: corp_label,
        seed,
        budget,
        depth,
        extract_config: ec.clone(),
        ec_name: ec_label,
        min_score,
        use_meta_gen: meta_on,
    }
}

#[test]
#[ignore = "phase ML2-dense: 10,000 randomly-sampled traversal probes, ~2min, --ignored"]
fn dense_campaign() {
    let n_probes: usize = std::env::var("MATHSCAPE_DENSE_PROBES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10_000);

    println!();
    println!("╔══════════════════════════════════════════════════════════════════════╗");
    println!("║ MATHSCAPE DENSE DISCOVERY CAMPAIGN                                   ║");
    println!(
        "║   {} randomly-sampled probes over 7-axis parameter space{}║",
        n_probes,
        " ".repeat(14 - n_probes.to_string().len()),
    );
    println!("╚══════════════════════════════════════════════════════════════════════╝");
    println!();

    let start = std::time::Instant::now();
    let mut provenance: HashMap<String, Provenance> = HashMap::new();
    let mut total_rules_emitted = 0usize;
    let mut barren_probes = 0usize;
    let mut per_apparatus_touches: BTreeMap<&str, usize> = BTreeMap::new();
    let mut per_corpus_touches: BTreeMap<&str, usize> = BTreeMap::new();

    // Progress printing cadence
    let checkpoint = (n_probes / 10).max(1);

    for i in 0..n_probes as u64 {
        let cfg = sample_config(i);
        let corpus_instance = cfg.corpus.build(cfg.seed, cfg.budget, cfg.depth);
        let (_, _axiom, rules) = run_one(
            cfg.apparatus_src,
            &corpus_instance,
            &cfg.extract_config,
            cfg.min_score,
            cfg.use_meta_gen,
        );

        *per_apparatus_touches.entry(cfg.apparatus).or_insert(0) += 1;
        *per_corpus_touches.entry(cfg.corpus_name).or_insert(0) += 1;
        if rules.is_empty() {
            barren_probes += 1;
        }

        for rule in rules {
            let k = rule_key(&rule);
            let p = provenance.entry(k).or_default();
            p.total_runs += 1;
            p.apparatuses.insert(cfg.apparatus);
            p.corpora.insert(cfg.corpus_name);
            p.ec_names.insert(cfg.ec_name);
            p.budgets.insert(cfg.budget);
            p.depths.insert(cfg.depth);
            p.meta_states.insert(cfg.use_meta_gen);
            total_rules_emitted += 1;
        }

        if (i + 1) as usize % checkpoint == 0 {
            let elapsed = start.elapsed().as_secs_f64();
            let rate = (i + 1) as f64 / elapsed;
            let eta = (n_probes as f64 - (i + 1) as f64) / rate;
            println!(
                "  progress: {}/{} ({:.1}/s, {:.1}s elapsed, ETA {:.1}s, {} barren)",
                i + 1,
                n_probes,
                rate,
                elapsed,
                eta,
                barren_probes
            );
        }
    }

    let elapsed = start.elapsed().as_secs_f64();
    println!();
    println!("campaign completed in {elapsed:.1}s");
    println!("  total rule emissions          : {total_rules_emitted}");
    println!(
        "  unique structural rules       : {}",
        provenance.len()
    );
    println!("  barren probes (empty library) : {barren_probes}");
    println!("  mean rules per probe          : {:.2}",
        total_rules_emitted as f64 / n_probes as f64);
    println!();

    // Per-apparatus touches — confirm the sampler reached every apparatus.
    println!("apparatus sampling distribution (top 10):");
    let mut app_ranks: Vec<(&&str, &usize)> = per_apparatus_touches.iter().collect();
    app_ranks.sort_by(|a, b| b.1.cmp(a.1));
    for (a, c) in app_ranks.iter().take(10) {
        println!("  {:<16} {:>5} probes", a, c);
    }
    println!();

    // Rank rules by total_runs.
    let mut by_runs: Vec<(&String, &Provenance)> = provenance.iter().collect();
    by_runs.sort_by(|a, b| b.1.total_runs.cmp(&a.1.total_runs));

    println!("top-30 rules by total emission count (raw dominance):");
    println!(
        "  {:>6}  {:>4}  {:>4}  {:>4}  rule",
        "runs", "apps", "corp", "ecs"
    );
    for (k, p) in by_runs.iter().take(30) {
        println!(
            "  {:>6}  {:>4}  {:>4}  {:>4}  {}",
            p.total_runs,
            p.apparatuses.len(),
            p.corpora.len(),
            p.ec_names.len(),
            k
        );
    }
    println!();

    // Cross-dimensional coverage score: sum of coverage fractions
    // across axes. A rule with max coverage on all axes scores 6.0.
    // Rules that appear across ALL apparatus, ALL corpus, ALL ec,
    // ALL budget, ALL depth, BOTH meta states score 6.0.
    let n_apparatuses = app_ranks.len();
    let n_corpora = per_corpus_touches.len();
    let n_ecs = 5;
    let n_budgets = 5;
    let n_depths = 5;
    let n_metas = 2;
    let mut by_coverage: Vec<(&String, &Provenance, f64)> = provenance
        .iter()
        .map(|(k, p)| {
            let score =
                (p.apparatuses.len() as f64 / n_apparatuses as f64) +
                (p.corpora.len() as f64 / n_corpora as f64) +
                (p.ec_names.len() as f64 / n_ecs as f64) +
                (p.budgets.len() as f64 / n_budgets as f64) +
                (p.depths.len() as f64 / n_depths as f64) +
                (p.meta_states.len() as f64 / n_metas as f64);
            (k, p, score)
        })
        .collect();
    by_coverage.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());

    println!("top-30 rules by CROSS-DIMENSIONAL coverage (max 6.00):");
    println!(
        "  {:>6}  {:>4}/{:<2}  {:>4}/{:<2}  {:>4}/{:<2}  {:>4}/{:<2}  {:>4}/{:<2}  {:>4}/{:<2}  rule",
        "score",
        "apps", n_apparatuses,
        "corp", n_corpora,
        "ecs", n_ecs,
        "bud", n_budgets,
        "dep", n_depths,
        "mta", n_metas,
    );
    for (k, p, score) in by_coverage.iter().take(30) {
        println!(
            "  {:>6.2}  {:>4}/{:<2}  {:>4}/{:<2}  {:>4}/{:<2}  {:>4}/{:<2}  {:>4}/{:<2}  {:>4}/{:<2}  {}",
            score,
            p.apparatuses.len(), n_apparatuses,
            p.corpora.len(), n_corpora,
            p.ec_names.len(), n_ecs,
            p.budgets.len(), n_budgets,
            p.depths.len(), n_depths,
            p.meta_states.len(), n_metas,
            k,
        );
    }
    println!();

    // Histogram: how many rules have how many apparatus-touches?
    let mut apparatus_count_hist: BTreeMap<usize, usize> = BTreeMap::new();
    for p in provenance.values() {
        *apparatus_count_hist.entry(p.apparatuses.len()).or_insert(0) += 1;
    }
    println!("apparatus-coverage histogram (rules × apparatus count):");
    for (apps, count) in apparatus_count_hist.iter().rev().take(15) {
        let bar: String = "█".repeat((*count).min(80));
        println!("  apps={:>2}: {:>5}  {}", apps, count, bar);
    }
    println!();

    // Invariants.
    assert!(!provenance.is_empty(), "campaign must discover some rules");
    assert!(n_probes >= 100, "probe count must be at least 100");
    let productive = n_probes - barren_probes;
    assert!(
        (productive as f64) / (n_probes as f64) >= 0.5,
        "over half of probes must be productive"
    );
}
