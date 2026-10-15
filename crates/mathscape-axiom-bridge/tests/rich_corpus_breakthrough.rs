//! Phase Z.8 (2026-04-19): rich corpus → right-identity frontier test.
//!
//! Z.6 and Z.7 identified `right-identity` as the ONE remaining
//! frontier: the motor's default corpus never shows
//! `add(?x, 0)` shapes, so anti-unification never produces a
//! right-identity rule.
//!
//! This test runs the SAME motor with the NEW RichCorpusGenerator
//! (which explicitly emits both left- and right-oriented
//! identity positions) and measures whether the frontier closes.

use mathscape_compress::derive_laws_from_corpus_instrumented;
use mathscape_core::bootstrap::{
    BootstrapCycle, CanonicalDeduper, DefaultCorpusGenerator,
    DefaultModelUpdater, LawExtractor, NoDedup,
};
use mathscape_core::eval::RewriteRule;
use mathscape_core::math_problem::{mathematician_curriculum, run_curriculum};
use mathscape_core::policy::LinearPolicy;
use mathscape_core::term::Term;
use mathscape_core::RichCorpusGenerator;

struct DerivedLawsExtractor {
    step_limit: usize,
    min_support: usize,
}
impl LawExtractor for DerivedLawsExtractor {
    fn extract(
        &self,
        corpus: &[Term],
        library: &[RewriteRule],
    ) -> Vec<RewriteRule> {
        let mut next_id: mathscape_core::term::SymbolId =
            (library.len() + 1) as u32;
        let (laws, _) = derive_laws_from_corpus_instrumented(
            corpus,
            library,
            self.step_limit,
            self.min_support,
            &mut next_id,
        );
        laws
    }
}

/// Head-to-head: default corpus vs rich corpus on the full
/// 67-problem curriculum. Which library closes right-identity?
#[test]
fn rich_corpus_head_to_head_vs_default_on_right_identity() {
    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║ PHASE Z.8 — RICH CORPUS: CLOSING THE RIGHT-IDENTITY FRONTIER  ║");
    println!("╚══════════════════════════════════════════════════════════════╝");

    let curriculum = mathematician_curriculum();

    // ── Run A: DEFAULT corpus, 10 iterations ────────────────────
    let default_cycle = BootstrapCycle::new(
        DefaultCorpusGenerator,
        DerivedLawsExtractor {
            step_limit: 300,
            min_support: 2,
        },
        DefaultModelUpdater::default(),
        10,
    );
    let default_result = default_cycle.run_until_stable(
        Vec::new(),
        LinearPolicy::tensor_seeking_prior(),
        &CanonicalDeduper,
        1,
    );
    let default_library = default_result.final_library;
    let default_report = run_curriculum(&curriculum, &default_library);
    let default_right = default_report
        .per_subdomain
        .get("right-identity")
        .unwrap()
        .solved_count;

    // ── Run B: RICH corpus + CanonicalDeduper, 10 iters ────────
    let rich_cycle = BootstrapCycle::new(
        RichCorpusGenerator,
        DerivedLawsExtractor {
            step_limit: 300,
            min_support: 2,
        },
        DefaultModelUpdater::default(),
        10,
    );
    let rich_result = rich_cycle.run_until_stable(
        Vec::new(),
        LinearPolicy::tensor_seeking_prior(),
        &CanonicalDeduper,
        1,
    );
    let rich_library = rich_result.final_library;
    let rich_report = run_curriculum(&curriculum, &rich_library);
    let rich_right = rich_report
        .per_subdomain
        .get("right-identity")
        .unwrap()
        .solved_count;

    // ── Run C: RICH corpus + NoDedup, 10 iters ─────────────────
    //
    // CanonicalDeduper collapses add(?x, 0) into add(0, ?x) as
    // "canonically equal" and rejects the second. NoDedup lets
    // both orientations coexist in the library → pattern match
    // can then succeed on either orientation.
    let rich_nodedup_cycle = BootstrapCycle::new(
        RichCorpusGenerator,
        DerivedLawsExtractor {
            step_limit: 300,
            min_support: 2,
        },
        DefaultModelUpdater::default(),
        10,
    );
    let rich_nd_result = rich_nodedup_cycle.run_until_stable(
        Vec::new(),
        LinearPolicy::tensor_seeking_prior(),
        &NoDedup,
        1,
    );
    let rich_nd_library = rich_nd_result.final_library;
    let rich_nd_report = run_curriculum(&curriculum, &rich_nd_library);
    let rich_nd_right = rich_nd_report
        .per_subdomain
        .get("right-identity")
        .unwrap()
        .solved_count;

    // ── Report ──────────────────────────────────────────────────
    println!("\n  DEFAULT corpus ({} iterations):", 10);
    println!("    rules discovered:    {}", default_library.len());
    println!(
        "    curriculum total:    {}/{} ({:.1}%)",
        default_report.total.solved_count,
        default_report.total.problem_set_size,
        default_report.total.solved_fraction() * 100.0,
    );
    println!(
        "    right-identity:      {}/5",
        default_right
    );
    print_rules("default", &default_library);

    println!("\n  RICH corpus + CanonicalDeduper ({} iterations):", 10);
    println!("    rules discovered:    {}", rich_library.len());
    println!(
        "    curriculum total:    {}/{} ({:.1}%)",
        rich_report.total.solved_count,
        rich_report.total.problem_set_size,
        rich_report.total.solved_fraction() * 100.0,
    );
    println!(
        "    right-identity:      {}/5",
        rich_right
    );
    print_rules("rich+canonical", &rich_library);

    println!("\n  RICH corpus + NoDedup ({} iterations):", 10);
    println!("    rules discovered:    {}", rich_nd_library.len());
    println!(
        "    curriculum total:    {}/{} ({:.1}%)",
        rich_nd_report.total.solved_count,
        rich_nd_report.total.problem_set_size,
        rich_nd_report.total.solved_fraction() * 100.0,
    );
    println!(
        "    right-identity:      {}/5",
        rich_nd_right
    );
    print_rules("rich+nodedup", &rich_nd_library);

    // ── Head-to-head per-subdomain ──────────────────────────────
    println!("\n  PER-SUBDOMAIN COMPARISON (default → rich):");
    let mut subdomains: std::collections::BTreeSet<&&'static str> =
        std::collections::BTreeSet::new();
    for sd in default_report.per_subdomain.keys() {
        subdomains.insert(sd);
    }
    for sd in rich_report.per_subdomain.keys() {
        subdomains.insert(sd);
    }
    for sd in &subdomains {
        let d = default_report
            .per_subdomain
            .get(*sd)
            .map(|r| (r.solved_count, r.problem_set_size))
            .unwrap_or((0, 0));
        let r = rich_report
            .per_subdomain
            .get(*sd)
            .map(|r| (r.solved_count, r.problem_set_size))
            .unwrap_or((0, 0));
        let arrow =
            if r.0 > d.0 { "↑" } else if r.0 < d.0 { "↓" } else { "=" };
        println!(
            "    {:<25}  default {}/{}   rich {}/{}   {}",
            sd, d.0, d.1, r.0, r.1, arrow
        );
    }

    // ── Assertions ──────────────────────────────────────────────
    //
    // The rich corpus MUST do at least as well on right-identity
    // as the default. If we made it strictly better, that's a
    // breakthrough. If it's equal, we've at least not regressed.
    assert!(
        rich_right >= default_right,
        "rich corpus must not regress right-identity \
         (default={default_right}, rich={rich_right})"
    );

    // Total score non-regression.
    assert!(
        rich_report.total.solved_count
            >= default_report.total.solved_count,
        "rich corpus total ({}) >= default total ({})",
        rich_report.total.solved_count,
        default_report.total.solved_count,
    );

    // ── Three-way verdict (right-identity + zero-absorber) ────
    let default_zero = default_report
        .per_subdomain
        .get("zero-absorber")
        .map(|r| r.solved_count)
        .unwrap_or(0);
    let rich_zero = rich_report
        .per_subdomain
        .get("zero-absorber")
        .map(|r| r.solved_count)
        .unwrap_or(0);
    let rich_nd_zero = rich_nd_report
        .per_subdomain
        .get("zero-absorber")
        .map(|r| r.solved_count)
        .unwrap_or(0);

    println!("\n  ╔═══════════════════ VERDICT ════════════════════╗");
    println!(
        "    right-identity:  default={}/5  rich+canonical={}/5  rich+nodedup={}/5",
        default_right, rich_right, rich_nd_right
    );
    println!(
        "    zero-absorber:   default={}/5  rich+canonical={}/5  rich+nodedup={}/5",
        default_zero, rich_zero, rich_nd_zero
    );
    if rich_nd_right > rich_right {
        let gained = rich_nd_right - rich_right;
        println!(
            "    🎯 BREAKTHROUGH: NoDedup unlocks {gained} right-id problem(s)"
        );
        println!(
            "    Mechanism confirmed: CanonicalDeduper was collapsing"
        );
        println!(
            "    right-oriented rules back into left-oriented ones"
        );
        println!(
            "    because add(?x, 0) canonicalizes to add(0, ?x)."
        );
    } else if rich_nd_right == rich_right && rich_right == default_right {
        println!("    ⚖  All three tied. The gap lives deeper —");
        println!("       likely in the extractor's pattern enumeration");
        println!("       rather than in the deduper or corpus alone.");
    } else {
        println!("    Mixed result — inspect libraries above.");
    }
    println!("  ╚═════════════════════════════════════════════════╝\n");
}

fn print_rules(label: &str, lib: &[RewriteRule]) {
    println!("    {label} library ({} rules):", lib.len());
    for (i, r) in lib.iter().enumerate() {
        println!("      [{i}] {}", r.name);
    }
}
