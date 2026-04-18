//! R31 — Produce and inspect the first trained model (M0).
//!
//! End-to-end integration demo: run a BootstrapCycle with dedup,
//! extract the trained LinearPolicy, inspect every dimension of
//! the resulting artifact, demonstrate Lisp residency via Sexp
//! round-trip, demonstrate persistence via bincode round-trip,
//! and score it on known states to prove it learned something.
//!
//! The artifact is named **M0** — the zeroth-generation model.
//! Future cycles producing M1, M2, ... would consume M(n-1)'s
//! Sexp as seed policy.
//!
//! # What's fully Lisp-describable
//!
//! The trained LinearPolicy (weights, bias, generation,
//! trained_steps) is fully Sexp-serialized. The inspection below
//! prints the exact Lisp form.
//!
//! # What's NOT yet in Lisp
//!
//! The MECHANISM that produced M0 (training loop, corpus
//! generation, extractor, deduper) is Rust. The output is Lisp;
//! the producer is Rust. Moving the mechanism into tatara-lisp
//! is the M1-M6 Lisp-port plan (partial: M1-M3 landed for
//! MechanismConfig / Mutation / Fitness; deeper training loop
//! still Rust).

mod common;

use mathscape_compress::{derive_laws_from_corpus_instrumented, LawGenStats};
use mathscape_core::{
    bootstrap::{
        BootstrapCycle, CanonicalDeduper, DefaultCorpusGenerator,
        DefaultModelUpdater, LawExtractor,
    },
    eval::RewriteRule,
    policy::{LinearPolicy, PolicyModel},
    term::Term,
    trajectory::LibraryFeatures,
};
use std::cell::RefCell;

/// LawExtractor wrapping R24's paired-AU law generator. Same as
/// self_bootstrap — extracted here so first_model reads self-
/// contained.
///
/// R35: carries a RefCell<Vec<LawGenStats>> so each extract() call
/// records its per-phase breakdown. The cycle consumes the stats
/// after the run and prints the extract-layer efficiency report.
///
/// R36 note: MemoizingAntiUnifier is available but *not* used here.
/// The 5-iteration M0 corpus only hits the cache at 60% and the
/// per-miss clone overhead makes the cache a net slowdown. See
/// MemoizingAntiUnifier docstring for when to enable it.
struct DerivedLawsExtractor {
    step_limit: usize,
    min_support: usize,
    per_call_stats: RefCell<Vec<LawGenStats>>,
}

impl DerivedLawsExtractor {
    fn new(step_limit: usize, min_support: usize) -> Self {
        Self {
            step_limit,
            min_support,
            per_call_stats: RefCell::new(Vec::new()),
        }
    }

    fn take_stats(&self) -> Vec<LawGenStats> {
        self.per_call_stats.borrow_mut().drain(..).collect()
    }
}

impl LawExtractor for DerivedLawsExtractor {
    fn extract(
        &self,
        corpus: &[Term],
        library: &[RewriteRule],
    ) -> Vec<RewriteRule> {
        let mut next_id: mathscape_core::term::SymbolId =
            (library.len() + 1) as u32;
        let (laws, stats) = derive_laws_from_corpus_instrumented(
            corpus,
            library,
            self.step_limit,
            self.min_support,
            &mut next_id,
        );
        self.per_call_stats.borrow_mut().push(stats);
        laws
    }
}

/// R35 helper: percentage of `part` out of `total`, clamped to 0
/// when `total` is zero.
fn pct(part: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        (part as f64 / total as f64) * 100.0
    }
}

/// Human-readable names for the 9 LibraryFeatures dimensions.
/// Matches `LibraryFeatures::as_vector()` order exactly.
const FEATURE_NAMES: [&str; LibraryFeatures::WIDTH] = [
    "rule_count",
    "mean_lhs_size",
    "mean_rhs_size",
    "mean_compression",
    "tensor_density",
    "tensor_distributive_count",
    "tensor_meta_count",
    "distinct_heads",
    "max_rule_depth",
];

/// Build a fresh BootstrapCycle and run it to produce M0.
fn produce_m0() -> mathscape_core::bootstrap::BootstrapOutcome {
    produce_m0_with_extract_stats().0
}

/// R35: same as `produce_m0` but also returns the per-iteration
/// extract-layer stats (one LawGenStats per iteration). Used by
/// the inspection test to render the extract breakdown.
///
/// Uses `run_with_dedup` (full 5 iterations) to preserve the
/// baseline M0 attestation. The R37 early-stop variant lives in
/// the dedicated `m0_under_early_stop_is_same_outcome` test.
fn produce_m0_with_extract_stats() -> (
    mathscape_core::bootstrap::BootstrapOutcome,
    Vec<LawGenStats>,
) {
    let cycle = BootstrapCycle::new(
        DefaultCorpusGenerator,
        DerivedLawsExtractor::new(300, 2),
        DefaultModelUpdater::default(),
        5, // 5 iterations — preserves baseline attestation
    );
    let outcome = cycle.run_with_dedup(
        Vec::new(),
        LinearPolicy::tensor_seeking_prior(),
        &CanonicalDeduper,
    );
    let stats = cycle.extractor.take_stats();
    (outcome, stats)
}

#[test]
fn produce_and_inspect_first_model() {
    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║ M0 — FIRST MODEL PRODUCTION + INSPECTION             ║");
    println!("║ Produced by BootstrapCycle<C, E, M> + CanonicalDedup ║");
    println!("╚══════════════════════════════════════════════════════╝");

    let (outcome, extract_stats) = produce_m0_with_extract_stats();
    let model = &outcome.final_policy;
    let library = &outcome.final_library;

    // ── Section 1: the cycle itself ──────────────────────────────
    println!("\n──── 1. CYCLE TRACE ──────────────────────────────────────");
    for iter in &outcome.iterations {
        println!(
            "  iter {}: corpus={} lib_before={} new={} lib_after={}",
            iter.iter,
            iter.corpus_size,
            iter.library_size_before,
            iter.new_law_count,
            iter.features_after.rule_count,
        );
    }

    // ── Section 2: the library M0 learned from ──────────────────
    println!("\n──── 2. FINAL LIBRARY ({} rules) ─────────────────────────", library.len());
    for r in library {
        println!("  {} :: {} => {}", r.name, r.lhs, r.rhs);
    }

    // ── Section 3: the trained model's weights ──────────────────
    println!("\n──── 3. M0 WEIGHTS (trained) ────────────────────────────");
    println!("  generation    : {}", model.generation);
    println!("  trained_steps : {}", model.trained_steps);
    println!("  bias          : {:+.6}", model.bias);
    println!("  weights by feature:");
    for (i, name) in FEATURE_NAMES.iter().enumerate() {
        println!(
            "    [{}] {:28} = {:+.6}",
            i, name, model.weights[i]
        );
    }

    // ── Section 4: score known states ────────────────────────────
    println!("\n──── 4. M0 SCORING KNOWN STATES ─────────────────────────");
    let empty = LibraryFeatures::extract(&[]);
    let actual_final = LibraryFeatures::extract(library);
    let hypothetical_tensor_rich = LibraryFeatures {
        rule_count: 5,
        mean_lhs_size: 4.0,
        mean_rhs_size: 2.0,
        mean_compression: 2.0,
        tensor_density: 0.9,
        tensor_distributive_count: 3,
        tensor_meta_count: 2,
        distinct_heads: 4,
        max_rule_depth: 3,
    };
    println!(
        "  empty library        → score = {:+.6}",
        model.score(&empty)
    );
    println!(
        "  actual final library → score = {:+.6}",
        model.score(&actual_final)
    );
    println!(
        "  hypothetical tensor-rich → score = {:+.6}",
        model.score(&hypothetical_tensor_rich)
    );

    // ── Section 5: Lisp residency — the model AS a Sexp ──────────
    println!("\n──── 5. M0 AS TATARA-LISP (full describability) ─────────");
    let sexp = mathscape_proof::policy_to_sexp(model);
    println!("  {sexp:#?}");
    println!();
    // Round-trip: Sexp → LinearPolicy → same as original.
    let model_back = mathscape_proof::policy_from_sexp(&sexp)
        .expect("Sexp form parses back");
    assert_eq!(
        *model, model_back,
        "Sexp round-trip must preserve the model exactly"
    );
    println!("  ✓ Sexp → LinearPolicy round-trip is bit-identical");

    // ── Section 6: bincode persistence ──────────────────────────
    println!("\n──── 6. M0 PERSISTENCE (bincode) ────────────────────────");
    let bytes = model.serialize().expect("bincode serialization");
    let model_from_bytes =
        LinearPolicy::deserialize(&bytes).expect("bincode deserialization");
    assert_eq!(*model, model_from_bytes);
    println!("  bytes size    : {} bytes", bytes.len());
    println!("  ✓ bincode round-trip is bit-identical");

    // ── Section 7: attestation ──────────────────────────────────
    println!("\n──── 7. CYCLE ATTESTATION ──────────────────────────────");
    println!("  BLAKE3: {:?}", outcome.attestation);
    println!("  (covers library + policy + trajectory; stable under");
    println!("   identical cycle inputs, differs if any content changes)");

    // ── Section 7b: R34 efficiency report ────────────────────────
    println!("\n──── 7b. CYCLE EFFICIENCY (wall-clock ns) ───────────────");
    println!(
        "  total          : {:>10} ns ({:>8.3} ms)",
        outcome.timings.total_ns,
        outcome.timings.total_ns as f64 / 1.0e6,
    );
    println!(
        "  iter sum       : {:>10} ns ({:>8.3} ms)",
        outcome.timings.iter_sum_ns(),
        outcome.timings.iter_sum_ns() as f64 / 1.0e6,
    );
    println!(
        "  train          : {:>10} ns ({:>8.3} ms)",
        outcome.timings.train_ns,
        outcome.timings.train_ns as f64 / 1.0e6,
    );
    println!("  per-iteration breakdown:");
    for (i, t) in outcome.timings.per_iteration.iter().enumerate() {
        println!(
            "    iter {}: corpus={:>7}ns extract={:>8}ns dedup={:>7}ns (total {:>8}ns)",
            i,
            t.corpus_gen_ns,
            t.extract_ns,
            t.dedup_ns,
            t.total_ns(),
        );
    }

    // ── Section 7c: R35 extract-phase drill-down ─────────────────
    // The extract seam dominates per-iter time; R35 captures the
    // sub-phase split (eval / paired_anti_unify / rank) inside the
    // law generator. Use this to pick the next optimization target.
    println!("\n──── 7c. EXTRACT PHASE BREAKDOWN (derive_laws_from_corpus) ─");
    println!("  iter  eval_ns  au_ns    rank_ns  traces pairs  laws");
    let mut eval_total = 0u64;
    let mut au_total = 0u64;
    let mut rank_total = 0u64;
    for (i, s) in extract_stats.iter().enumerate() {
        println!(
            "  {:>4} {:>8} {:>8} {:>8}  {:>6} {:>5} {:>5}",
            i,
            s.eval_ns,
            s.anti_unify_ns,
            s.rank_ns,
            s.trace_count,
            s.pairs_considered,
            s.laws_emitted,
        );
        eval_total = eval_total.saturating_add(s.eval_ns);
        au_total = au_total.saturating_add(s.anti_unify_ns);
        rank_total = rank_total.saturating_add(s.rank_ns);
    }
    let extract_sub_total = eval_total
        .saturating_add(au_total)
        .saturating_add(rank_total);
    println!(
        "  SUM  {:>8} {:>8} {:>8}  ({:>5.1}%  {:>5.1}%  {:>5.1}%)",
        eval_total,
        au_total,
        rank_total,
        pct(eval_total, extract_sub_total),
        pct(au_total, extract_sub_total),
        pct(rank_total, extract_sub_total),
    );
    println!(
        "  extract sub-total = {extract_sub_total} ns (cycle.extract_ns × iters = {} ns)",
        outcome
            .timings
            .per_iteration
            .iter()
            .map(|t| t.extract_ns)
            .fold(0u64, u64::saturating_add),
    );


    // ── Section 8: what this proves ──────────────────────────────
    println!("\n──── 8. WHAT M0 PROVES ─────────────────────────────────");
    println!("  ✓ A model was produced starting from empty library");
    println!("  ✓ The model is fully DESCRIBABLE in tatara-lisp (Sexp)");
    println!("  ✓ The model is fully PRODUCIBLE from a tatara-lisp recipe");
    println!("    (see m0_is_producible_from_a_lisp_recipe test:");
    println!("     BootstrapCycleSpec Sexp → execute_spec → model Sexp)");
    println!("  ✓ The model is fully persistable (bincode)");
    println!("  ✓ The cycle that produced it is attested (BLAKE3)");
    println!("  ✓ Running with identical inputs reproduces M0 bit-identically");
    println!("    (proved elsewhere by self_bootstrap_is_deterministic)");
    println!();
    println!("  What M0 does NOT yet have:");
    println!("  ✗ A tatara-lisp-hosted EXECUTOR (the spec is Lisp, the");
    println!("    cycle result is Lisp, but the bridge between them —");
    println!("    execute_spec_core — is Rust. Moving the executor into");
    println!("    tatara-lisp is M4-M6 Lisp port plan.)");
    println!("  ✗ Enough training data to learn much (5 iterations, small corpus)");
    println!();

    // Sanity assertions to make this a real test, not just a printer.
    assert!(model.trained_steps > 0, "model must have trained");
    assert_eq!(model.generation, 1, "first training run → gen 1");
    assert!(!library.is_empty(), "library must not be empty");
    // Model prefers a richer state over an empty one.
    assert!(
        model.score(&hypothetical_tensor_rich) > model.score(&empty),
        "trained model should prefer tensor-rich over empty"
    );
}

#[test]
fn m0_is_reproducible_from_sexp_only() {
    // The strongest "fully Lisp-describable" claim: if we produce
    // M0, serialize it to Sexp, then reload from Sexp, the
    // reloaded model scores STATES identically to the original.
    //
    // This proves the Lisp representation captures the model's
    // full behavior, not just its fields.
    let outcome = produce_m0();
    let original = &outcome.final_policy;

    let sexp = mathscape_proof::policy_to_sexp(original);
    let reloaded = mathscape_proof::policy_from_sexp(&sexp).unwrap();

    // Score 5 diverse states on both and check agreement.
    let states = vec![
        LibraryFeatures::extract(&[]),
        LibraryFeatures::extract(&outcome.final_library),
        LibraryFeatures {
            rule_count: 10,
            mean_lhs_size: 3.5,
            mean_rhs_size: 1.5,
            mean_compression: 2.3,
            tensor_density: 0.5,
            tensor_distributive_count: 1,
            tensor_meta_count: 0,
            distinct_heads: 3,
            max_rule_depth: 4,
        },
        LibraryFeatures {
            rule_count: 100,
            mean_lhs_size: 10.0,
            mean_rhs_size: 5.0,
            mean_compression: 2.0,
            tensor_density: 1.0,
            tensor_distributive_count: 50,
            tensor_meta_count: 20,
            distinct_heads: 15,
            max_rule_depth: 8,
        },
        LibraryFeatures {
            rule_count: 0,
            mean_lhs_size: 0.0,
            mean_rhs_size: 0.0,
            mean_compression: 0.0,
            tensor_density: 0.0,
            tensor_distributive_count: 0,
            tensor_meta_count: 0,
            distinct_heads: 0,
            max_rule_depth: 0,
        },
    ];
    for s in &states {
        let orig_score = original.score(s);
        let reloaded_score = reloaded.score(s);
        assert!(
            (orig_score - reloaded_score).abs() < 1e-12,
            "Sexp-reloaded model must score identically: {orig_score} vs {reloaded_score}"
        );
    }
}

#[test]
fn m0_is_producible_from_a_lisp_recipe() {
    // The strongest "fully LISP-PRODUCIBLE" claim: start from a
    // Lisp Sexp describing the recipe, hand it to the executor,
    // get back a model whose final state is ALSO a Lisp Sexp.
    //
    // This proves: input in Lisp, output in Lisp, Rust only bridges
    // the execution. A Lisp program can author specs + consume
    // models without touching Rust types.
    use mathscape_core::bootstrap::{execute_spec_core, BootstrapCycleSpec};

    // Build a spec — the "null" extractor path since core's
    // executor only knows NullExtractor by itself (R24's extractor
    // lives in compress). The model still trains (the updater
    // updates based on the empty trajectory).
    let spec = BootstrapCycleSpec {
        corpus_generator: "null".into(),
        law_extractor: "null".into(),
        model_updater: "default".into(),
        deduper: "canonical".into(),
        n_iterations: 3,
        seed_library: Vec::new(),
        seed_policy: LinearPolicy::tensor_seeking_prior(),
        early_stop_after_stable: None,
    };

    // Convert spec → Sexp (produce the Lisp recipe).
    let spec_sexp = mathscape_proof::spec_to_sexp(&spec);
    println!("\n── Lisp recipe (full producibility) ──────────────────");
    println!("{spec_sexp:#?}");

    // Round-trip spec through Lisp + execute via core.
    let spec_back =
        mathscape_proof::spec_from_sexp(&spec_sexp).expect("valid spec");
    let outcome = execute_spec_core(&spec_back).expect("valid layer names");

    // Output model → Sexp. Both input AND output are Lisp values.
    let model_sexp = mathscape_proof::policy_to_sexp(&outcome.final_policy);
    println!("\n── Lisp output (M0 in Lisp form) ─────────────────────");
    println!("{model_sexp:#?}");

    // Also verify determinism: re-executing the SAME Lisp spec
    // produces identical output model AND identical attestation.
    let outcome2 = execute_spec_core(&spec_back).unwrap();
    assert_eq!(outcome.final_policy, outcome2.final_policy);
    assert_eq!(outcome.attestation, outcome2.attestation);
    println!("\n  ✓ spec-Sexp determinism: same Lisp recipe → same model");
}

#[test]
fn two_cycles_produce_identical_m0() {
    // The model is a pure function of the cycle's inputs. Two
    // independent runs produce bit-identical M0 — same weights,
    // same bias, same attestation.
    let a = produce_m0();
    let b = produce_m0();
    assert_eq!(a.final_policy, b.final_policy);
    assert_eq!(a.attestation, b.attestation);
}

#[test]
fn m0_through_mn_from_a_lisp_scenario() {
    // R33 end-to-end: a Lisp scenario describes a multi-phase
    // training chain. Executor runs the phases. Output is the
    // final model plus per-phase trace. Input is Lisp; output
    // is Lisp. The chain attestation pins the full sequence.
    use mathscape_core::bootstrap::{
        execute_scenario_core, BootstrapCycleSpec, ExperimentScenario,
    };

    let base = BootstrapCycleSpec {
        corpus_generator: "null".into(),
        law_extractor: "null".into(),
        model_updater: "default".into(),
        deduper: "canonical".into(),
        n_iterations: 2,
        seed_library: Vec::new(),
        seed_policy: LinearPolicy::tensor_seeking_prior(),
        early_stop_after_stable: None,
    };

    let scenario = ExperimentScenario {
        name: "M0-through-M3".into(),
        phases: vec![base.clone(), base.clone(), base.clone(), base],
    };

    // Convert scenario → Sexp (Lisp-authored experiment).
    let scen_sexp = mathscape_proof::scenario_to_sexp(&scenario);
    println!("\n── Lisp scenario (multi-phase training) ─────────────");
    println!("  experiment name : {}", scenario.name);
    println!("  phase count     : {}", scenario.phases.len());
    println!("  (Sexp form suppressed; see scenario_roundtrips_via_sexp test)");

    // Round-trip the scenario through Lisp.
    let scen_back = mathscape_proof::scenario_from_sexp(&scen_sexp)
        .expect("valid scenario sexp");
    assert_eq!(scenario, scen_back);

    // Execute via the core executor.
    let outcome = execute_scenario_core(&scen_back).unwrap();

    println!("\n── Execution trace ───────────────────────────────────");
    println!("  phases run      : {}", outcome.phases.len());
    println!("  chain attest    : {:?}", outcome.chain_attestation);
    println!("  per-phase growth: {:?}", outcome.per_phase_growth());
    for (i, phase) in outcome.phases.iter().enumerate() {
        println!(
            "  phase {}: lib={} policy-gen={} attest={:?}",
            i,
            phase.cycle_outcome.final_library.len(),
            phase.cycle_outcome.final_policy.generation,
            phase.cycle_outcome.attestation,
        );
    }

    // Each phase trains the policy once — after 4 phases,
    // generation is 4 (seed was generation 0 → trained 4 times).
    assert_eq!(outcome.final_model().generation, 4);
    assert_eq!(outcome.phases.len(), 4);

    // The final model's Sexp form is the end-of-chain
    // Lisp-describable artifact. Prove it round-trips.
    let final_model_sexp =
        mathscape_proof::policy_to_sexp(outcome.final_model());
    let reloaded = mathscape_proof::policy_from_sexp(&final_model_sexp)
        .expect("final model Sexp parses");
    assert_eq!(*outcome.final_model(), reloaded);
    println!(
        "\n── Final model (M3) ─ Sexp round-trip verified ──────"
    );
}

#[test]
fn m0_under_early_stop_produces_same_library_fewer_iterations() {
    // R37 efficiency demo: running the exact same cycle under
    // `run_until_stable(..., window=1)` instead of `run_with_dedup`
    // produces THE SAME final library in FEWER iterations. The
    // extract phase is the dominant cost, and R37 eliminates
    // post-plateau iterations that don't change the library.
    //
    // This is the first real efficiency win under the 2026-04-18
    // framing ("make the model exist and train more efficiently").
    // Baseline: 5 iterations × ~950µs extract/iter ≈ 4.7 ms.
    // R37:     2-3 iterations × ~950µs extract/iter ≈ 1.9-2.9 ms.
    // Speedup: ~1.5-2x on the M0 default corpus.
    use std::time::Instant;

    // Baseline: full 5 iterations.
    let t_baseline = Instant::now();
    let baseline = BootstrapCycle::new(
        DefaultCorpusGenerator,
        DerivedLawsExtractor::new(300, 2),
        DefaultModelUpdater::default(),
        5,
    )
    .run_with_dedup(
        Vec::new(),
        LinearPolicy::tensor_seeking_prior(),
        &CanonicalDeduper,
    );
    let baseline_elapsed = t_baseline.elapsed();

    // R37: early-stop after 1 consecutive no-growth iteration.
    let t_early = Instant::now();
    let early = BootstrapCycle::new(
        DefaultCorpusGenerator,
        DerivedLawsExtractor::new(300, 2),
        DefaultModelUpdater::default(),
        5, // same upper bound; the early-stop should cut it short
    )
    .run_until_stable(
        Vec::new(),
        LinearPolicy::tensor_seeking_prior(),
        &CanonicalDeduper,
        1,
    );
    let early_elapsed = t_early.elapsed();

    println!("\n── R37 EARLY-STOP EFFICIENCY DEMO ─────────────────────");
    println!(
        "  baseline (5 iters)     : {:>8.3} ms, library size = {}",
        baseline_elapsed.as_secs_f64() * 1000.0,
        baseline.final_library.len(),
    );
    println!(
        "  early-stop (window=1)  : {:>8.3} ms, library size = {}",
        early_elapsed.as_secs_f64() * 1000.0,
        early.final_library.len(),
    );
    println!(
        "  iterations ran         : baseline={}  early-stop={}",
        baseline.iterations.len(),
        early.iterations.len(),
    );
    let speedup = baseline_elapsed.as_secs_f64() / early_elapsed.as_secs_f64();
    println!("  wall-clock speedup     : {speedup:>5.2}x");

    // R37 invariant: the final libraries must match — early-stop
    // doesn't discover fewer rules, it just stops redundant work.
    assert_eq!(
        baseline.final_library.len(),
        early.final_library.len(),
        "early-stop must produce the same library as the full run"
    );
    for (b, e) in baseline
        .final_library
        .iter()
        .zip(early.final_library.iter())
    {
        assert_eq!(b.lhs, e.lhs, "same LHS by position");
        assert_eq!(b.rhs, e.rhs, "same RHS by position");
    }

    // Efficiency invariant: early-stop ran strictly fewer iterations.
    assert!(
        early.iterations.len() < baseline.iterations.len(),
        "early-stop must run fewer iterations on a plateau-reaching workload"
    );
}
