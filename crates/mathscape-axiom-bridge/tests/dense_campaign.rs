//! Dense discovery campaign — parallelized probe sweep at scale.
//!
//! Each probe samples (apparatus, corpus_family, seed, budget,
//! depth, extract_config, min_score) uniformly-at-random and runs
//! a single fast traversal. Rules are keyed by structural identity
//! (anonymized LHS+RHS) and provenance is tracked across every
//! sampled axis.
//!
//! Rules that appear across many distinct apparatus × corpus ×
//! config × scale cells are TRUE cross-dimensional universals.
//! Rules in narrow slices are apparatus/corpus/config-specific
//! discoveries.
//!
//! Parallelism: rayon fans the probe loop across cores. The
//! aggregation uses thread-local HashMaps that are merged at the
//! end to avoid contention on the hot path. On a 14-core system
//! this gives ~10x speedup over sequential.
//!
//! Default probe count = 100,000. Override via MATHSCAPE_DENSE_PROBES.
//! Typical budgets:
//!   10,000   : ~15s  — smoke test
//!   100,000  : ~2min — standard campaign
//!   1,000,000: ~20min — deep sweep
//!   10,000,000: ~3h+ — background job, saturates discovery

mod common;

use common::experiment::{format_term, run_probe_fast, CorpusFamily};
use mathscape_compress::extract::ExtractConfig;
use mathscape_core::eval::{anonymize_term, RewriteRule};
use rayon::prelude::*;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Mutex;

// ── Parameter space ──────────────────────────────────────────────

// 30 apparatuses spanning ablation, shape, weight, conditional, and
// new exotic forms. All forms must parse under the Lisp evaluator
// (operators: +, -, *, /, max, min, if, clamp).
const APPARATUSES: &[(&str, &str)] = &[
    // Baseline
    ("canonical", "(+ (* alpha cr) (* beta novelty) (* gamma meta-compression) (* delta lhs-subsumption))"),
    // Axis ablation
    ("cr-only",       "(* alpha cr)"),
    ("novelty-only",  "(* beta novelty)"),
    ("meta-only",     "(* gamma meta-compression)"),
    ("sub-only",      "(* delta lhs-subsumption)"),
    // Two-axis
    ("cr+nov",        "(+ (* alpha cr) (* beta novelty))"),
    ("cr+meta",       "(+ (* alpha cr) (* gamma meta-compression))"),
    ("cr+sub",        "(+ (* alpha cr) (* delta lhs-subsumption))"),
    ("nov+meta",      "(+ (* beta novelty) (* gamma meta-compression))"),
    ("nov+sub",       "(+ (* beta novelty) (* delta lhs-subsumption))"),
    ("meta+sub",      "(+ (* gamma meta-compression) (* delta lhs-subsumption))"),
    // Shapes
    ("max-pair",      "(max (+ (* alpha cr) (* beta novelty)) (+ (* gamma meta-compression) (* delta lhs-subsumption)))"),
    ("max-all",       "(max (max (* alpha cr) (* beta novelty)) (max (* gamma meta-compression) (* delta lhs-subsumption)))"),
    ("min-pair",      "(+ (min (* alpha cr) (* beta novelty)) (* gamma meta-compression) (* delta lhs-subsumption))"),
    ("cr*nov",        "(+ (* cr novelty) (* gamma meta-compression) (* delta lhs-subsumption))"),
    ("harmonic",      "(+ (/ (* cr novelty) (max (+ cr novelty) 0.001)) (* gamma meta-compression) (* delta lhs-subsumption))"),
    ("clamped",       "(clamp (+ (* alpha cr) (* beta novelty) (* gamma meta-compression) (* delta lhs-subsumption)) -1 2)"),
    ("threshold-cr",  "(+ (* alpha cr) (if (max (- cr 0.05) 0) (* beta novelty) 0) (* gamma meta-compression) (* delta lhs-subsumption))"),
    ("squared-nov",   "(+ (* alpha cr) (* beta (* novelty novelty)) (* gamma meta-compression) (* delta lhs-subsumption))"),
    ("cr-minus-sub",  "(+ (* alpha cr) (* beta novelty) (- (* gamma meta-compression) (* 0.1 lhs-subsumption)))"),
    // Weight perturbations
    ("alpha-x2",      "(+ (* (* 2 alpha) cr) (* beta novelty) (* gamma meta-compression) (* delta lhs-subsumption))"),
    ("beta-x2",       "(+ (* alpha cr) (* (* 2 beta) novelty) (* gamma meta-compression) (* delta lhs-subsumption))"),
    ("delta-x3",      "(+ (* alpha cr) (* beta novelty) (* gamma meta-compression) (* (* 3 delta) lhs-subsumption))"),
    ("uniform-025",   "(+ (* 0.25 cr) (* 0.25 novelty) (* 0.25 meta-compression) (* 0.25 lhs-subsumption))"),
    ("uniform-05",    "(+ (* 0.5 cr) (* 0.5 novelty) (* 0.5 meta-compression) (* 0.5 lhs-subsumption))"),
    ("meta-heavy",    "(+ (* alpha cr) (* beta novelty) (* (* 5 gamma) meta-compression) (* delta lhs-subsumption))"),
    // Exotic — penalty / negative terms
    ("cr-penalty",    "(+ (* alpha cr) (* (- 0 beta) novelty) (* gamma meta-compression) (* delta lhs-subsumption))"),
    ("sub-penalty",   "(+ (* alpha cr) (* beta novelty) (* gamma meta-compression) (* (- 0 delta) lhs-subsumption))"),
    ("tanh-like",     "(+ (clamp (* 2 cr) -1 1) (* beta novelty) (* gamma meta-compression) (* delta lhs-subsumption))"),
    ("quadratic-cr",  "(+ (* alpha (* cr cr)) (* beta novelty) (* gamma meta-compression) (* delta lhs-subsumption))"),
];

const CORPORA: &[(CorpusFamily, &str)] = &[
    (CorpusFamily::Procedural,              "procedural"),
    (CorpusFamily::ZooPlusProcedural,       "zoo+proc"),
    (CorpusFamily::AsymmetricArith,         "asymmetric"),
    (CorpusFamily::DeeplyNested,            "nested"),
    (CorpusFamily::SuccessorChain,          "succ-chain"),
    (CorpusFamily::MixedOperators,          "mixed"),
    (CorpusFamily::FullArithmetic,          "full-arith"),
    (CorpusFamily::PeanoSymmetric,          "peano-sym"),
    (CorpusFamily::DistributivityScaffold,  "distrib"),
];

fn extract_configs() -> Vec<(ExtractConfig, &'static str)> {
    vec![
        (ExtractConfig { min_shared_size: 2, min_matches: 2, max_new_rules: 5 }, "ec-loose"),
        (ExtractConfig { min_shared_size: 3, min_matches: 2, max_new_rules: 5 }, "ec-default"),
        (ExtractConfig { min_shared_size: 4, min_matches: 2, max_new_rules: 5 }, "ec-strict"),
        (ExtractConfig { min_shared_size: 2, min_matches: 3, max_new_rules: 8 }, "ec-triple"),
        (ExtractConfig { min_shared_size: 3, min_matches: 2, max_new_rules: 12}, "ec-wide"),
        (ExtractConfig { min_shared_size: 2, min_matches: 2, max_new_rules: 3 }, "ec-narrow"),
        (ExtractConfig { min_shared_size: 5, min_matches: 2, max_new_rules: 5 }, "ec-deeper"),
    ]
}

const BUDGETS: &[usize] = &[6, 8, 10, 12, 16];
const DEPTHS: &[usize] = &[2, 3, 4, 5, 6];
const MIN_SCORES: &[f64] = &[0.0, 0.005, 0.02, 0.05];

#[derive(Clone, Default)]
struct Provenance {
    total_runs: usize,
    apparatuses: BTreeSet<&'static str>,
    corpora: BTreeSet<&'static str>,
    ec_names: BTreeSet<&'static str>,
    budgets: BTreeSet<usize>,
    depths: BTreeSet<usize>,
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

fn rule_key(r: &RewriteRule) -> String {
    format!(
        "{}→{}",
        format_term(&anonymize_term(&r.lhs)),
        format_term(&anonymize_term(&r.rhs)),
    )
}

/// Run a single probe. Deterministic given `iter`.
fn run_probe(iter: u64) -> (Vec<(String, &'static str, &'static str, &'static str, usize, usize)>, bool) {
    let mut r = xorshift(iter.wrapping_mul(2654435761));
    let (ap_label, ap_src) = APPARATUSES[(r as usize) % APPARATUSES.len()];
    r = xorshift(r);
    let (corp, corp_label) = CORPORA[(r as usize) % CORPORA.len()];
    r = xorshift(r);
    let seed = r % 100_000;
    r = xorshift(r);
    let budget = BUDGETS[(r as usize) % BUDGETS.len()];
    r = xorshift(r);
    let depth = DEPTHS[(r as usize) % DEPTHS.len()];
    r = xorshift(r);
    let ecs = extract_configs();
    let ec_i = (r as usize) % ecs.len();
    let (ref ec, ec_label) = ecs[ec_i];
    r = xorshift(r);
    let min_score = MIN_SCORES[(r as usize) % MIN_SCORES.len()];

    let corpus_instance = corp.build(seed, budget, depth);
    let rules = run_probe_fast(ap_src, &corpus_instance, ec, min_score);
    let barren = rules.is_empty();
    let out: Vec<_> = rules
        .iter()
        .map(|rule| (rule_key(rule), ap_label, corp_label, ec_label, budget, depth))
        .collect();
    (out, barren)
}

#[test]
#[ignore = "phase ML2-dense: parallelized probe campaign. Default 100k probes, MATHSCAPE_DENSE_PROBES=n to override. --ignored"]
fn dense_campaign() {
    let n_probes: usize = std::env::var("MATHSCAPE_DENSE_PROBES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100_000);

    let n_cores = rayon::current_num_threads();

    println!();
    println!("╔══════════════════════════════════════════════════════════════════════╗");
    println!("║ MATHSCAPE DENSE DISCOVERY CAMPAIGN (parallel)                        ║");
    println!(
        "║   probes: {:<10}  apparatuses: {:<3}  corpora: {:<3}  cores: {:<2}         ║",
        n_probes, APPARATUSES.len(), CORPORA.len(), n_cores
    );
    println!("╚══════════════════════════════════════════════════════════════════════╝");
    println!();

    let start = std::time::Instant::now();

    // Shared aggregation state. Use a Mutex for simplicity; chunk
    // size is tuned so contention is minimal.
    let provenance: Mutex<HashMap<String, Provenance>> = Mutex::new(HashMap::new());
    let total_emissions: Mutex<usize> = Mutex::new(0);
    let barren_count: Mutex<usize> = Mutex::new(0);
    let apparatus_touches: Mutex<BTreeMap<&str, usize>> =
        Mutex::new(BTreeMap::new());
    let corpus_touches: Mutex<BTreeMap<&str, usize>> =
        Mutex::new(BTreeMap::new());

    // Progress counter.
    let progress = std::sync::atomic::AtomicUsize::new(0);
    let progress_every = (n_probes / 20).max(1);

    // Chunk probes for rayon. We run in chunks so each worker
    // aggregates locally then merges, minimizing lock contention.
    const CHUNK: usize = 256;

    (0..n_probes).into_par_iter().chunks(CHUNK).for_each(|chunk| {
        // Local aggregation — updated per chunk, then merged.
        let mut local_prov: HashMap<String, Provenance> = HashMap::new();
        let mut local_app: BTreeMap<&str, usize> = BTreeMap::new();
        let mut local_corp: BTreeMap<&str, usize> = BTreeMap::new();
        let mut local_emissions = 0usize;
        let mut local_barren = 0usize;

        for iter in chunk {
            let (rules, barren) = run_probe(iter as u64);
            if barren {
                local_barren += 1;
            }
            if let Some((_, ap_label, corp_label, _, _, _)) = rules.first() {
                *local_app.entry(*ap_label).or_insert(0) += 1;
                *local_corp.entry(*corp_label).or_insert(0) += 1;
            } else {
                // Still need to count apparatus/corpus touches
                // even when barren. Re-derive without running
                // the probe by using the same sample stream.
                let mut r = xorshift((iter as u64).wrapping_mul(2654435761));
                let (ap_label, _) = APPARATUSES[(r as usize) % APPARATUSES.len()];
                r = xorshift(r);
                let (_corp, corp_label) = CORPORA[(r as usize) % CORPORA.len()];
                *local_app.entry(ap_label).or_insert(0) += 1;
                *local_corp.entry(corp_label).or_insert(0) += 1;
            }
            for (key, ap, corp, ec, bud, dep) in rules {
                let p = local_prov.entry(key).or_default();
                p.total_runs += 1;
                p.apparatuses.insert(ap);
                p.corpora.insert(corp);
                p.ec_names.insert(ec);
                p.budgets.insert(bud);
                p.depths.insert(dep);
                local_emissions += 1;
            }
        }

        // Progress tick.
        let done = progress.fetch_add(
            CHUNK,
            std::sync::atomic::Ordering::Relaxed,
        ) + CHUNK;
        if done / progress_every > (done - CHUNK) / progress_every.max(1) {
            let elapsed = start.elapsed().as_secs_f64();
            let rate = done as f64 / elapsed;
            let eta = (n_probes.saturating_sub(done)) as f64 / rate.max(1.0);
            eprintln!(
                "  progress: {:>8}/{} ({:.0}/s, {:.1}s elapsed, ETA {:.0}s)",
                done.min(n_probes), n_probes, rate, elapsed, eta
            );
        }

        // Merge local into global.
        {
            let mut g = provenance.lock().unwrap();
            for (k, p) in local_prov {
                let entry = g.entry(k).or_default();
                entry.total_runs += p.total_runs;
                entry.apparatuses.extend(p.apparatuses);
                entry.corpora.extend(p.corpora);
                entry.ec_names.extend(p.ec_names);
                entry.budgets.extend(p.budgets);
                entry.depths.extend(p.depths);
            }
        }
        *total_emissions.lock().unwrap() += local_emissions;
        *barren_count.lock().unwrap() += local_barren;
        {
            let mut g = apparatus_touches.lock().unwrap();
            for (k, v) in local_app {
                *g.entry(k).or_insert(0) += v;
            }
        }
        {
            let mut g = corpus_touches.lock().unwrap();
            for (k, v) in local_corp {
                *g.entry(k).or_insert(0) += v;
            }
        }
    });

    let elapsed = start.elapsed().as_secs_f64();
    let provenance = provenance.into_inner().unwrap();
    let total_emissions = *total_emissions.lock().unwrap();
    let barren = *barren_count.lock().unwrap();
    let apparatus_touches = apparatus_touches.into_inner().unwrap();
    let corpus_touches = corpus_touches.into_inner().unwrap();

    println!();
    println!("campaign completed in {elapsed:.1}s ({:.0} probes/s)", n_probes as f64 / elapsed);
    println!("  total rule emissions          : {total_emissions}");
    println!("  unique structural rules       : {}", provenance.len());
    println!("  barren probes                 : {barren}");
    println!(
        "  mean rules per probe          : {:.2}",
        total_emissions as f64 / n_probes as f64
    );
    println!();

    // Apparatus sampling distribution (confirm uniformity).
    let mut app_ranks: Vec<(&&str, &usize)> = apparatus_touches.iter().collect();
    app_ranks.sort_by(|a, b| b.1.cmp(a.1));
    println!("apparatus distribution (top 10, expected ~{}):",
        n_probes / APPARATUSES.len());
    for (a, c) in app_ranks.iter().take(10) {
        println!("  {:<16} {:>8} probes", a, c);
    }
    println!();

    // Corpus sampling distribution.
    let mut corp_ranks: Vec<(&&str, &usize)> = corpus_touches.iter().collect();
    corp_ranks.sort_by(|a, b| b.1.cmp(a.1));
    println!("corpus distribution (expected ~{}):",
        n_probes / CORPORA.len());
    for (c, n) in &corp_ranks {
        println!("  {:<16} {:>8} probes", c, n);
    }
    println!();

    // Top rules by total emissions.
    let mut by_runs: Vec<(&String, &Provenance)> = provenance.iter().collect();
    by_runs.sort_by(|a, b| b.1.total_runs.cmp(&a.1.total_runs));

    println!("top-40 rules by total emission count (raw dominance):");
    println!(
        "  {:>8}  {:>4}  {:>4}  {:>4}  rule",
        "runs", "apps", "corp", "ecs"
    );
    for (k, p) in by_runs.iter().take(40) {
        println!(
            "  {:>8}  {:>4}  {:>4}  {:>4}  {}",
            p.total_runs,
            p.apparatuses.len(),
            p.corpora.len(),
            p.ec_names.len(),
            k
        );
    }
    println!();

    // Cross-dimensional coverage score (max 5.00 since we have 5
    // axes now: apparatus, corpus, ec, budget, depth).
    let na = APPARATUSES.len() as f64;
    let nc = CORPORA.len() as f64;
    let ne = extract_configs().len() as f64;
    let nb = BUDGETS.len() as f64;
    let nd = DEPTHS.len() as f64;
    let mut by_coverage: Vec<(&String, &Provenance, f64)> = provenance
        .iter()
        .map(|(k, p)| {
            let score =
                (p.apparatuses.len() as f64 / na) +
                (p.corpora.len() as f64 / nc) +
                (p.ec_names.len() as f64 / ne) +
                (p.budgets.len() as f64 / nb) +
                (p.depths.len() as f64 / nd);
            (k, p, score)
        })
        .collect();
    by_coverage.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());

    println!("top-40 rules by CROSS-DIMENSIONAL coverage (max 5.00):");
    println!(
        "  {:>6}  {:>5}/{:<2}  {:>4}/{:<2}  {:>4}/{:<2}  {:>4}/{:<2}  {:>4}/{:<2}  rule",
        "score",
        "apps", APPARATUSES.len(),
        "corp", CORPORA.len(),
        "ecs", extract_configs().len(),
        "bud", BUDGETS.len(),
        "dep", DEPTHS.len(),
    );
    for (k, p, score) in by_coverage.iter().take(40) {
        println!(
            "  {:>6.2}  {:>5}/{:<2}  {:>4}/{:<2}  {:>4}/{:<2}  {:>4}/{:<2}  {:>4}/{:<2}  {}",
            score,
            p.apparatuses.len(), APPARATUSES.len(),
            p.corpora.len(), CORPORA.len(),
            p.ec_names.len(), extract_configs().len(),
            p.budgets.len(), BUDGETS.len(),
            p.depths.len(), DEPTHS.len(),
            k,
        );
    }
    println!();

    // Histogram.
    let mut apparatus_count_hist: BTreeMap<usize, usize> = BTreeMap::new();
    for p in provenance.values() {
        *apparatus_count_hist.entry(p.apparatuses.len()).or_insert(0) += 1;
    }
    println!("apparatus-coverage histogram (rules × apparatus count):");
    for (apps, count) in apparatus_count_hist.iter().rev().take(20) {
        let bar: String = "█".repeat((*count).min(80));
        println!("  apps={:>2}: {:>6}  {}", apps, count, bar);
    }
    println!();

    // Final summary.
    let full_universal = provenance
        .values()
        .filter(|p| p.apparatuses.len() == APPARATUSES.len())
        .count();
    let near_universal = provenance
        .values()
        .filter(|p| p.apparatuses.len() >= APPARATUSES.len() / 2 + 1)
        .count();
    let apparatus_specific = provenance
        .values()
        .filter(|p| p.apparatuses.len() == 1)
        .count();

    println!("╔══════════════════════════════════════════════════════════════════════╗");
    println!(
        "║ {:>8} structural rules · {:>3} full-universal · {:>4} near-universal        ║",
        provenance.len(), full_universal, near_universal
    );
    println!(
        "║ {:>3} apparatus-specific · elapsed {:>5.1}s · {:.0} probes/s                  ║",
        apparatus_specific, elapsed, n_probes as f64 / elapsed
    );
    println!("╚══════════════════════════════════════════════════════════════════════╝");

    // Invariants.
    assert!(!provenance.is_empty(), "campaign must discover rules");
    let productive = n_probes - barren;
    assert!(
        (productive as f64) / (n_probes as f64) >= 0.5,
        "at least 50% of probes must be productive"
    );
    if n_probes >= 10_000 {
        assert!(
            full_universal >= 1,
            "at scale ≥10k probes, at least 1 rule must be full-universal"
        );
    }
}
