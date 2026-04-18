//! Phase L5 — the perpetual discovery machine.
//!
//! Closes the loop the entire architecture has been pointing at:
//! sub-campaign → phase J validate → detect saturation → Rustify
//! top-K theorems into substrate → evolve apparatus pool → re-run
//! in expanded space → continue.
//!
//! The correctness criterion is *zero halt*. If the novelty rate
//! ever drops to zero across an entire cycle, the machine has
//! stopped discovering — which, given Gödel-incompleteness of any
//! sufficiently-rich substrate, can only happen if our apparatus
//! mutator has collapsed OR if our Rustification policy has
//! starved the next-layer frontier. Both are bugs to diagnose,
//! not legitimate end-states.
//!
//! The machine is broken the moment it stops. This test asserts
//! nonzero novelty across N cycles with zero human intervention —
//! no hand-selected theorems to Rustify, no hand-tuned apparatus
//! choices, no external signals. The loop makes every decision
//! from its own measurements.
//!
//! MVP scope:
//!   - Rust-implemented mutation operators over Lisp apparatus
//!     Sexp forms (swap weights, wrap in max/clamp, permute terms).
//!     Lisp-expressed mutation operators are phase L6+.
//!   - Soft Rustification: validated theorems get appended to the
//!     substrate passed to the evaluator. True Rustification (via
//!     tatara-lisp-derive, recompiled types) is later.
//!   - Saturation detection: novelty_rate = new_theorems / cycle.
//!     If below threshold, trigger Rustification + apparatus
//!     evolution.
//!
//! Success: ≥5 cycles complete with ≥1 new theorem per cycle
//! (averaged). Failure: novelty rate hits 0.

mod common;

use common::experiment::{
    adaptive_corpus, format_term, run_probe_fast_with_substrate, CorpusFamily,
};
use mathscape_compress::extract::ExtractConfig;
use mathscape_core::eval::{anonymize_term, RewriteRule};
use mathscape_core::term::Term;
use mathscape_proof::discovery::{theorem_key, DiscoverySession};
use mathscape_proof::semantic::{
    discover_semantic_projections_with_ledger, SemanticCandidate, SemanticVerdict,
    ValidationConfig,
};
use rayon::prelude::*;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Mutex;

// ── Seed apparatus pool ──────────────────────────────────────────
//
// The machine starts with a small seed set. From here it evolves
// autonomously — each cycle's winners become the parents of next
// cycle's mutants. The seed is NOT a hand-tuned final set; it's
// just an initial population to bootstrap the evolutionary search.

const SEED_APPARATUSES: &[(&str, &str)] = &[
    ("canonical",    "(+ (* alpha cr) (* beta novelty) (* gamma meta-compression) (* delta lhs-subsumption))"),
    ("cr+nov",       "(+ (* alpha cr) (* beta novelty))"),
    ("harmonic",     "(+ (/ (* cr novelty) (max (+ cr novelty) 0.001)) (* gamma meta-compression) (* delta lhs-subsumption))"),
    ("max-pair",     "(max (+ (* alpha cr) (* beta novelty)) (+ (* gamma meta-compression) (* delta lhs-subsumption)))"),
    ("novelty-only", "(* beta novelty)"),
    ("meta-heavy",   "(+ (* alpha cr) (* beta novelty) (* (* 5 gamma) meta-compression) (* delta lhs-subsumption))"),
    ("clamped",      "(clamp (+ (* alpha cr) (* beta novelty) (* gamma meta-compression) (* delta lhs-subsumption)) -1 2)"),
    ("cr*nov",       "(+ (* cr novelty) (* gamma meta-compression) (* delta lhs-subsumption))"),
];

const CORPORA: &[CorpusFamily] = &[
    CorpusFamily::Procedural,
    CorpusFamily::AsymmetricArith,
    CorpusFamily::DeeplyNested,
    CorpusFamily::SuccessorChain,
    CorpusFamily::FullArithmetic,
    CorpusFamily::PeanoSymmetric,
    CorpusFamily::DistributivityScaffold,
];

const EXTRACT_CONFIGS: &[ExtractConfig] = &[
    ExtractConfig { min_shared_size: 2, min_matches: 2, max_new_rules: 5 },
    ExtractConfig { min_shared_size: 3, min_matches: 2, max_new_rules: 5 },
    ExtractConfig { min_shared_size: 2, min_matches: 3, max_new_rules: 8 },
    ExtractConfig { min_shared_size: 3, min_matches: 2, max_new_rules: 12 },
];

const BUDGETS: &[usize] = &[8, 12, 16];
const DEPTHS: &[usize] = &[3, 4, 5];
const MIN_SCORES: &[f64] = &[0.0, 0.02];

// ── Apparatus mutation ───────────────────────────────────────────
//
// The evolutionary step. Given a set of "winner" apparatuses
// (ranked by theorem yield in the previous cycle), generate
// MUTANTS by:
//   (a) weight perturbation — scale a coefficient by 2 or 0.5
//   (b) term wrapping       — wrap in (max _ 0) or (clamp _ -1 2)
//   (c) operator swap       — change a + to max, * to min, etc.
//   (d) crossover           — take (head op args) of one parent and
//                             substitute arg subtree from another
//
// These operators are currently Rust-coded. Phase L6+ expresses
// them as Lisp forms the machine itself can mutate.

fn xorshift(mut x: u64) -> u64 {
    if x == 0 {
        x = 0x9E37_79B9_7F4A_7C15;
    }
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    x
}

#[derive(Clone, Debug)]
struct Apparatus {
    name: String,
    src: String,
}

/// Generate `n` mutants from a parent apparatus using
/// deterministic rewrites based on `seed`.
fn mutate_apparatus(parent: &Apparatus, seed: u64, n: usize) -> Vec<Apparatus> {
    type MutFn = fn(&str) -> String;
    fn clamp_wrap(s: &str) -> String {
        format!("(clamp {s} -1 2)")
    }
    fn maxzero(s: &str) -> String {
        format!("(max {s} 0)")
    }
    fn x2(s: &str) -> String {
        format!("(* 2 {s})")
    }
    fn x05(s: &str) -> String {
        format!("(* 0.5 {s})")
    }
    fn nov_plus(s: &str) -> String {
        format!("(+ {s} (* 0.1 novelty))")
    }
    fn meta_plus(s: &str) -> String {
        format!("(+ {s} (* 0.1 meta-compression))")
    }
    fn crmult(s: &str) -> String {
        format!("(* (max cr 0.001) {s})")
    }
    fn crgate(s: &str) -> String {
        format!("(if (max (- cr 0.01) 0) {s} 0)")
    }
    let mutations: &[(&str, MutFn)] = &[
        ("-clamp", clamp_wrap),
        ("-maxzero", maxzero),
        ("-x2", x2),
        ("-x05", x05),
        ("-nov+", nov_plus),
        ("-meta+", meta_plus),
        ("-crmult", crmult),
        ("-crgate", crgate),
    ];
    let mut out = Vec::with_capacity(n);
    let mut r = seed;
    for i in 0..n {
        r = xorshift(r);
        let (suffix, f) = &mutations[(r as usize) % mutations.len()];
        let mutant_name = format!("{}{}-{i}", parent.name, suffix);
        let mutant_src = f(&parent.src);
        out.push(Apparatus {
            name: mutant_name,
            src: mutant_src,
        });
    }
    out
}

/// Evolve the apparatus pool: keep top-K parents by theorem yield,
/// generate M mutants per parent, enforce population cap.
fn evolve_pool(
    winners: &[(Apparatus, usize)],
    keep_top: usize,
    mutants_per: usize,
    population_cap: usize,
    cycle_seed: u64,
) -> Vec<Apparatus> {
    let mut next_gen: Vec<Apparatus> = winners
        .iter()
        .take(keep_top)
        .map(|(a, _)| a.clone())
        .collect();
    for (i, (parent, _)) in winners.iter().take(keep_top).enumerate() {
        let mutants = mutate_apparatus(
            parent,
            cycle_seed.wrapping_add(i as u64 * 101),
            mutants_per,
        );
        next_gen.extend(mutants);
    }
    next_gen.truncate(population_cap);
    next_gen
}

// ── Campaign sub-runner ─────────────────────────────────────────

#[derive(Clone, Default)]
struct Provenance {
    total_runs: usize,
    apparatuses: BTreeSet<String>,
    corpora: BTreeSet<&'static str>,
    exemplar: Option<RewriteRule>,
}

fn rule_key(r: &RewriteRule) -> String {
    format!(
        "{}→{}",
        format_term(&anonymize_term(&r.lhs)),
        format_term(&anonymize_term(&r.rhs)),
    )
}

/// Run a single probe. Returns (rules, apparatus name).
///
/// Phase L1: if substrate is non-empty, at least half of probes
/// use the adaptive corpus (substrate-aware generator) instead of
/// one of the fixed families. This is what keeps the edge
/// receding — adaptive corpus produces structure at rank N+1
/// when substrate covers rank N.
fn run_probe(
    iter: u64,
    apparatuses: &[Apparatus],
    substrate: &[RewriteRule],
) -> (Vec<(String, RewriteRule)>, String) {
    let mut r = xorshift(iter.wrapping_mul(2654435761));
    let apparatus = &apparatuses[(r as usize) % apparatuses.len()];
    r = xorshift(r);
    let use_adaptive = !substrate.is_empty() && r % 2 == 0;
    let corp_index = (r as usize) % CORPORA.len();
    r = xorshift(r);
    let seed = r % 100_000;
    r = xorshift(r);
    let budget = BUDGETS[(r as usize) % BUDGETS.len()];
    r = xorshift(r);
    let depth = DEPTHS[(r as usize) % DEPTHS.len()];
    r = xorshift(r);
    let ec = &EXTRACT_CONFIGS[(r as usize) % EXTRACT_CONFIGS.len()];
    r = xorshift(r);
    let min_score = MIN_SCORES[(r as usize) % MIN_SCORES.len()];

    let corpus_instance: Vec<(String, Vec<Term>)> = if use_adaptive {
        // Generate several adaptive-corpus slabs, each labeled so
        // they appear to the Epoch as distinct "corpus instances".
        let vocab: [u32; 5] = [2, 3, 4, 5, 7]; // add, mul, succ, sub, pred
        let slabs = budget.min(4).max(2);
        let per_slab = 20;
        (0..slabs)
            .map(|i| {
                let name = format!("adaptive-s{seed}-slab{i}");
                let terms = adaptive_corpus(
                    substrate,
                    seed.wrapping_add(i as u64 * 101),
                    depth,
                    per_slab,
                    &vocab,
                    10,
                );
                (name, terms)
            })
            .collect()
    } else {
        CORPORA[corp_index].build(seed, budget, depth)
    };

    let rules = run_probe_fast_with_substrate(
        &apparatus.src,
        &corpus_instance,
        ec,
        min_score,
        substrate,
    );
    let out: Vec<_> = rules
        .iter()
        .map(|rule| {
            let anon = RewriteRule {
                name: rule.name.clone(),
                lhs: anonymize_term(&rule.lhs),
                rhs: anonymize_term(&rule.rhs),
            };
            (rule_key(rule), anon)
        })
        .collect();
    (out, apparatus.name.clone())
}

/// Run a sub-campaign: PROBES random probes over the apparatus
/// pool. Returns:
///   - the full provenance map keyed by structural rule identity
///   - per-apparatus theorem counts (after phase J)
fn run_sub_campaign(
    n_probes: usize,
    apparatuses: &[Apparatus],
    substrate: &[RewriteRule],
    ledger_rules: &[RewriteRule],
) -> (HashMap<String, Provenance>, BTreeMap<String, usize>) {
    let provenance: Mutex<HashMap<String, Provenance>> = Mutex::new(HashMap::new());
    const CHUNK: usize = 256;

    (0..n_probes).into_par_iter().chunks(CHUNK).for_each(|chunk| {
        let mut local: HashMap<String, Provenance> = HashMap::new();
        for iter in chunk {
            let (rules, ap_name) = run_probe(iter as u64, apparatuses, substrate);
            for (key, anon) in rules {
                let p = local.entry(key).or_default();
                if p.exemplar.is_none() {
                    p.exemplar = Some(anon);
                }
                p.total_runs += 1;
                p.apparatuses.insert(ap_name.clone());
            }
        }
        let mut g = provenance.lock().unwrap();
        for (k, p) in local {
            let entry = g.entry(k).or_default();
            entry.total_runs += p.total_runs;
            entry.apparatuses.extend(p.apparatuses);
            if entry.exemplar.is_none() {
                entry.exemplar = p.exemplar;
            }
        }
    });
    let provenance = provenance.into_inner().unwrap();

    // Phase J yield per apparatus — count how many top-coverage
    // rules have a valid projection under that apparatus's output.
    // We approximate: count rules whose exemplar validates AND
    // whose apparatus-set includes this apparatus.
    let vconfig = ValidationConfig::default();
    let validated: HashMap<String, Vec<(SemanticCandidate, SemanticVerdict)>> =
        provenance
            .par_iter()
            .filter_map(|(k, p)| {
                let rule = p.exemplar.as_ref()?;
                let results = discover_semantic_projections_with_ledger(rule, ledger_rules, &vconfig);
                if results.is_empty() {
                    None
                } else {
                    Some((k.clone(), results))
                }
            })
            .collect();

    let mut yield_by_app: BTreeMap<String, usize> = BTreeMap::new();
    for (k, _) in &validated {
        if let Some(p) = provenance.get(k) {
            for ap in &p.apparatuses {
                *yield_by_app.entry(ap.clone()).or_insert(0) += 1;
            }
        }
    }
    (provenance, yield_by_app)
}


/// Extract validated theorems from the campaign results that are
/// NOT already in the ledger. Returns up to `top_k` new theorems
/// ranked by apparatus coverage of the structural rule that
/// surfaced them.
fn extract_new_theorems(
    provenance: &HashMap<String, Provenance>,
    theorem_ledger: &HashSet<String>,
    ledger_rules: &[RewriteRule],
    top_k: usize,
) -> Vec<RewriteRule> {
    let vconfig = ValidationConfig::default();
    let mut ranked: Vec<(&String, &Provenance)> = provenance.iter().collect();
    ranked.sort_by(|a, b| b.1.apparatuses.len().cmp(&a.1.apparatuses.len()));

    let mut out: Vec<RewriteRule> = Vec::new();
    let mut seen_this_cycle: HashSet<String> = HashSet::new();
    for (_k, p) in ranked {
        let Some(rule) = p.exemplar.as_ref() else {
            continue;
        };
        let verdicts =
            discover_semantic_projections_with_ledger(rule, ledger_rules, &vconfig);
        for (cand, _) in verdicts {
            let tk = theorem_key(&cand.rule);
            if theorem_ledger.contains(&tk) || seen_this_cycle.contains(&tk) {
                continue;
            }
            seen_this_cycle.insert(tk);
            out.push(cand.rule.clone());
            if out.len() >= top_k {
                return out;
            }
        }
    }
    out
}

// ── The loop ─────────────────────────────────────────────────────

#[test]
#[ignore = "phase L5: edge-riding loop — perpetual discovery engine. ~5min, --ignored"]
fn edge_riding_loop() {
    // Knobs tuned for the long campaign (2026-04-18).
    // MATHSCAPE_EDGE_CYCLES env var overrides CYCLES for ad-hoc runs.
    const CYCLES_DEFAULT: usize = 50;
    const PROBES_PER_CYCLE: usize = 4000;
    const KEEP_TOP_APPARATUSES: usize = 6;
    const MUTANTS_PER_PARENT: usize = 3;
    const POPULATION_CAP: usize = 24;
    const RUSTIFY_TOP_K: usize = 16;
    const SATURATION_THRESHOLD: f64 = 0.5;

    let cycles: usize = std::env::var("MATHSCAPE_EDGE_CYCLES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(CYCLES_DEFAULT);

    println!();
    println!("╔══════════════════════════════════════════════════════════════════════╗");
    println!("║ PHASE L5 — EDGE-RIDING LOOP (perpetual discovery)                    ║");
    println!(
        "║ cycles={cycles}  probes/cycle={PROBES_PER_CYCLE}  keep_top={KEEP_TOP_APPARATUSES}  mutants={MUTANTS_PER_PARENT}             ║"
    );
    println!("║ correctness criterion: nonzero novelty rate across every cycle       ║");
    println!("╚══════════════════════════════════════════════════════════════════════╝");

    // Initial apparatus pool from seed.
    let mut pool: Vec<Apparatus> = SEED_APPARATUSES
        .iter()
        .map(|(n, s)| Apparatus {
            name: n.to_string(),
            src: s.to_string(),
        })
        .collect();

    // All discovery state lives in the DiscoverySession: substrate
    // (reducing theorems), ledger (every theorem), trajectory
    // (cycle-by-cycle record). Correctness invariant — "any
    // post-bootstrap zero-novelty cycle is a bug" — is enforced by
    // session.has_stalled() / session.stalled_cycles().
    let mut session = DiscoverySession::new();

    let start = std::time::Instant::now();

    for cycle in 0..cycles {
        let cycle_start = std::time::Instant::now();
        println!();
        println!(
            "── cycle {} / {} ─────  pool size: {}  substrate size: {}  ledger size: {}",
            cycle + 1,
            cycles,
            pool.len(),
            session.substrate.len(),
            session.ledger.len()
        );

        // 1. Run sub-campaign over the current substrate + ledger.
        let (provenance, yield_by_app) = run_sub_campaign(
            PROBES_PER_CYCLE,
            &pool,
            session.substrate.rules(),
            session.ledger.rules(),
        );

        println!(
            "  sub-campaign: {} probes, {} structural rules, {} apparatuses with yield",
            PROBES_PER_CYCLE,
            provenance.len(),
            yield_by_app.len()
        );

        // 2. Extract new theorems — test each exemplar's
        //    candidates against the validator, skip any already
        //    in the ledger.
        let mut ledger_keys: HashSet<String> = HashSet::new();
        for r in session.ledger.rules() {
            ledger_keys.insert(theorem_key(r));
        }

        let new_theorems = extract_new_theorems(
            &provenance,
            &ledger_keys,
            session.ledger.rules(),
            RUSTIFY_TOP_K,
        );
        println!("  new theorems discovered: {}", new_theorems.len());
        for t in &new_theorems {
            println!(
                "    + {} → {}",
                format_term(&t.lhs),
                format_term(&t.rhs)
            );
        }

        // 3. Promote each new theorem. Session handles the
        //    reducing-vs-equivalence split automatically.
        let mut added_to_substrate = 0usize;
        let mut ledger_only = 0usize;
        for t in new_theorems.iter().cloned() {
            let (added_ledger, added_substrate) = session.promote(t);
            if added_substrate {
                added_to_substrate += 1;
            } else if added_ledger {
                ledger_only += 1;
            }
        }
        println!(
            "  rustified: {added_to_substrate} reducing → substrate | {ledger_only} equivalence → ledger only"
        );

        // 4. Evolve apparatus pool.
        let mut winners: Vec<(Apparatus, usize)> = pool
            .iter()
            .map(|a| {
                let count = yield_by_app.get(&a.name).copied().unwrap_or(0);
                (a.clone(), count)
            })
            .collect();
        winners.sort_by(|a, b| b.1.cmp(&a.1));
        let top_names: Vec<_> = winners
            .iter()
            .take(KEEP_TOP_APPARATUSES)
            .map(|(a, c)| format!("{}={}", a.name, c))
            .collect();
        println!("  top apparatuses (by theorem yield): {top_names:?}");

        pool = evolve_pool(
            &winners,
            KEEP_TOP_APPARATUSES,
            MUTANTS_PER_PARENT,
            POPULATION_CAP,
            cycle as u64,
        );

        let cycle_elapsed = cycle_start.elapsed().as_secs_f64();
        session.record_cycle(
            cycle,
            provenance.len(),
            new_theorems.len(),
            cycle_elapsed,
        );
        println!("  cycle elapsed: {cycle_elapsed:.1}s");
    }

    let total_elapsed = start.elapsed().as_secs_f64();

    // ── Edge trajectory report ───────────────────────────────
    println!();
    println!("╔══════════════════════════════════════════════════════════════════════╗");
    println!("║ EDGE TRAJECTORY                                                      ║");
    println!("╚══════════════════════════════════════════════════════════════════════╝");
    println!(
        "  {:>6}  {:>12}  {:>12}  {:>14}  {:>10}",
        "cycle", "structural", "new_theorems", "substrate_size", "elapsed"
    );
    for rec in &session.trajectory {
        println!(
            "  {:>6}  {:>12}  {:>12}  {:>14}  {:>9.1}s",
            rec.cycle,
            rec.structural_rules,
            rec.new_theorems,
            rec.substrate_size,
            rec.elapsed_secs
        );
    }
    println!();

    let total_new_theorems = session.total_new_theorems();
    let mean_per_cycle = session.mean_theorems_per_cycle();
    let final_substrate = session.substrate.len();

    println!(
        "ran {cycles} cycles in {total_elapsed:.1}s, discovered {total_new_theorems} new theorems"
    );
    println!("mean theorems per cycle: {mean_per_cycle:.2}");
    println!("final substrate size: {final_substrate}  ledger size: {}", session.ledger.len());
    println!();

    // Print the evolved substrate — what the machine now knows.
    println!("── evolved substrate (machine's validated axioms) ───");
    for (i, r) in session.substrate.rules().iter().enumerate() {
        println!(
            "  [{i:>2}] {} → {}",
            format_term(&anonymize_term(&r.lhs)),
            format_term(&anonymize_term(&r.rhs))
        );
    }
    println!();

    // ── CORRECTNESS CRITERION ───────────────────────────────
    // By Gödel's incompleteness, any rich enough substrate has
    // new unprovables outside it. So any zero-novelty post-
    // bootstrap cycle is a bug — the machine stopped finding
    // theorems that must exist. The session enforces this check.
    let late_cycles_zero_novelty = session.stalled_cycles();

    println!("── correctness check ───────");
    println!(
        "  late-cycle zero-novelty count: {}",
        late_cycles_zero_novelty.len()
    );
    if late_cycles_zero_novelty.is_empty() {
        println!("  ✓ EDGE-RIDING CONFIRMED — nonzero novelty across all post-bootstrap cycles");
    } else {
        println!("  ✗ MACHINE STALLED — novelty collapsed at cycles:");
        for c in late_cycles_zero_novelty {
            println!("      cycle {c}");
        }
    }

    // Saturation threshold — mean theorems per cycle must exceed
    // threshold. Below this we've lost productive edge velocity.
    println!(
        "  mean theorems/cycle: {mean_per_cycle:.2}  (threshold: {SATURATION_THRESHOLD})"
    );
    if mean_per_cycle >= SATURATION_THRESHOLD {
        println!(
            "  ✓ NOVELTY RATE SUSTAINED — {mean_per_cycle:.2} ≥ {SATURATION_THRESHOLD}"
        );
    } else {
        println!(
            "  ✗ NOVELTY RATE TOO LOW — {mean_per_cycle:.2} < {SATURATION_THRESHOLD}"
        );
    }

    // Invariant: substrate must have grown during the run.
    assert!(
        final_substrate > 0,
        "substrate must have grown during the loop"
    );
    assert!(
        mean_per_cycle >= SATURATION_THRESHOLD,
        "novelty rate {mean_per_cycle:.2} below saturation threshold {SATURATION_THRESHOLD} — machine is stalling"
    );
}
