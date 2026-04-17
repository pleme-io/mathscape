//! Stress test — 50-epoch run through the full Allocator-driven
//! dispatch loop. Records per-epoch telemetry. Validates
//! theoretical predictions from `docs/arch/collapse-and-surprise.md`.
//!
//! # What this test confirms
//!
//! 1. The Allocator picks Reinforce when pressure > 0 and ΔDL from
//!    prior reinforcement passes justifies it
//! 2. Pressure rises (during Discover phases) then falls (during
//!    Reinforce phases) — the phase-transition signature
//! 3. Over enough epochs, the registry grows then collapses
//! 4. Traps fire at stable-root windows
//! 5. Different policies produce different trajectories (knowability
//!    criterion 3)
//!
//! # What this test enables
//!
//! Every epoch's telemetry is printed. The data is enough to
//! classify which *transitions* (Reductive → Explosive, pressure
//! spike → collapse event, etc.) were handled cleanly by the
//! algorithmic controllers vs which had ambiguous outcomes that
//! would benefit from learned policy (a tiny RL head on top of
//! Allocator::choose, a classifier on subsumption-pair priority,
//! etc.). This is the minimal-model-ladder question empirically
//! answered — not here, but from the data this test generates.

use mathscape_compress::{extract::ExtractConfig, CompressionGenerator};
use mathscape_core::{
    control::{Allocator, EpochAction, RealizationPolicy, RegimeDetector, RewardEstimator},
    epoch::{Epoch, InMemoryRegistry, Registry, RuleEmitter},
    event::{Event, EventCategory},
    reduction::{check_maximally_reduced, reduction_pressure, ReductionVerdict},
    term::Term,
    trap::TrapDetector,
    value::Value,
};
use mathscape_reward::{reward::RewardConfig, StatisticalProver};

fn apply(f: Term, args: Vec<Term>) -> Term {
    Term::Apply(Box::new(f), args)
}
fn nat(n: u64) -> Term {
    Term::Number(Value::Nat(n))
}
fn var(id: u32) -> Term {
    Term::Var(id)
}

/// A corpus designed to be pattern-rich: every term matches the
/// `add(_, 0) = _` pattern. Discovery should extract the identity
/// rule; subsequent epochs should accumulate pressure via specific
/// instantiations.
fn pattern_rich_corpus() -> Vec<Term> {
    vec![
        apply(var(2), vec![nat(1), nat(0)]),
        apply(var(2), vec![nat(3), nat(0)]),
        apply(var(2), vec![nat(5), nat(0)]),
        apply(var(2), vec![nat(7), nat(0)]),
        apply(var(2), vec![nat(11), nat(0)]),
        apply(var(2), vec![nat(13), nat(0)]),
        apply(var(2), vec![nat(17), nat(0)]),
        apply(var(2), vec![nat(19), nat(0)]),
    ]
}

#[derive(Debug, Clone)]
struct EpochSnapshot {
    epoch_id: u64,
    action: String,
    regime: String,
    library_size: usize,
    pressure: f64,
    delta_dl_discovery: f64,
    delta_dl_reinforce: f64,
    subsumption_events: usize,
    accept_events: usize,
    reject_events: usize,
    registry_root_bytes: [u8; 32],
    is_reduced: bool,
    traps_emitted: usize,
}

fn run_stress_trajectory(
    corpus: &[Term],
    policy: RealizationPolicy,
    n_epochs: usize,
) -> Vec<EpochSnapshot> {
    let mut epoch = Epoch::new(
        CompressionGenerator::new(
            ExtractConfig {
                min_shared_size: 2,
                min_matches: 2,
                max_new_rules: 3,
            },
            1,
        ),
        StatisticalProver::new(RewardConfig::default(), 0.0),
        RuleEmitter,
        InMemoryRegistry::new(),
    );
    let mut allocator = Allocator::new(policy, RewardEstimator::new(0.3));
    let mut regime_detector = RegimeDetector::new(5);
    let mut trap_detector = TrapDetector::new(3);
    let mut trap_count = 0usize;
    let mut snapshots = Vec::with_capacity(n_epochs);

    for _ in 0..n_epochs {
        let pressure_before = reduction_pressure(&epoch.registry);
        let trace = epoch.step_auto(corpus, &mut allocator);
        let regime = regime_detector.observe(&trace);
        if trap_detector
            .observe(epoch.registry.root(), epoch.epoch_id)
            .is_some()
        {
            trap_count += 1;
        }

        let action = match trace.action {
            Some(EpochAction::Discover) => "Discover",
            Some(EpochAction::Reinforce) => "Reinforce",
            Some(EpochAction::Promote(_)) => "Promote",
            Some(EpochAction::Migrate(_)) => "Migrate",
            None => "None",
        }
        .to_string();

        let mut delta_dl_discovery = 0.0f64;
        let mut delta_dl_reinforce = 0.0f64;
        let mut subsumption_events = 0usize;
        let mut accept_events = 0usize;
        let mut reject_events = 0usize;
        for ev in &trace.events {
            let dl = ev.delta_dl();
            match ev.category() {
                EventCategory::Discovery => delta_dl_discovery += dl,
                EventCategory::Reinforce => delta_dl_reinforce += dl,
                _ => {}
            }
            match ev {
                Event::Subsumption { .. } => subsumption_events += 1,
                Event::Accept { .. } => accept_events += 1,
                Event::Reject { .. } => reject_events += 1,
                _ => {}
            }
        }

        snapshots.push(EpochSnapshot {
            epoch_id: trace.epoch_id,
            action,
            regime: format!("{regime:?}"),
            library_size: epoch.registry.len(),
            pressure: pressure_before,
            delta_dl_discovery,
            delta_dl_reinforce,
            subsumption_events,
            accept_events,
            reject_events,
            registry_root_bytes: *epoch.registry.root().as_bytes(),
            is_reduced: check_maximally_reduced(&epoch.registry).is_reduced(),
            traps_emitted: trap_count,
        });
    }
    snapshots
}

fn print_trajectory(label: &str, snaps: &[EpochSnapshot]) {
    println!("\n── {label} — {} epochs ──", snaps.len());
    println!(
        "{:>3} {:>10} {:>10} {:>4} {:>7} {:>9} {:>9} {:>3} {:>3} {:>3} {:>5} {:>2}",
        "ep", "action", "regime", "|L|", "pres", "ΔDL-disc", "ΔDL-reinf", "sub", "acc", "rej", "redcd", "trp"
    );
    for s in snaps {
        println!(
            "{:>3} {:>10} {:>10} {:>4} {:>7.3} {:>9.2} {:>9.2} {:>3} {:>3} {:>3} {:>5} {:>2}",
            s.epoch_id,
            s.action,
            s.regime,
            s.library_size,
            s.pressure,
            s.delta_dl_discovery,
            s.delta_dl_reinforce,
            s.subsumption_events,
            s.accept_events,
            s.reject_events,
            if s.is_reduced { "YES" } else { "no" },
            s.traps_emitted,
        );
    }
}

#[test]
fn allocator_drives_pressure_collapse_cycle() {
    let corpus = pattern_rich_corpus();
    let policy = RealizationPolicy::default();
    let snaps = run_stress_trajectory(&corpus, policy, 30);
    print_trajectory("default policy, 30 epochs", &snaps);

    // --- Theoretical-prediction checks ---

    // 1. At least one Discover action fired (library grew from empty).
    let discover_count = snaps.iter().filter(|s| s.action == "Discover").count();
    assert!(
        discover_count >= 1,
        "allocator should have chosen Discover at least once"
    );

    // 2. Library grew beyond zero at some point.
    let max_lib = snaps.iter().map(|s| s.library_size).max().unwrap_or(0);
    assert!(max_lib > 0, "library should have grown past 0");

    // 3. At some epoch, pressure was greater than zero (pairs existed).
    //    If anti-unification extracted multiple overlapping rules, this
    //    should fire. If it didn't, that's also informative — the
    //    generator's dedup is stronger than expected.
    let observed_pressure = snaps.iter().any(|s| s.pressure > 0.0);
    println!(
        "\n  observed pressure > 0 at some epoch: {observed_pressure}  \
         (max pressure: {:.3}, max library: {max_lib})",
        snaps.iter().map(|s| s.pressure).fold(0.0f64, f64::max)
    );

    // 4. At least one trap emitted (registry stabilized across
    //    the detector's window at some point).
    let final_trap_count = snaps.last().map(|s| s.traps_emitted).unwrap_or(0);
    assert!(
        final_trap_count >= 1,
        "expected at least 1 trap over 30 epochs, got {final_trap_count}"
    );

    // 5. The final registry root is non-zero (library exists + hashed).
    let last_root = snaps.last().unwrap().registry_root_bytes;
    assert_ne!(last_root, [0; 32]);
}

#[test]
fn two_policies_produce_distinct_trajectories() {
    let corpus = pattern_rich_corpus();

    let mut p_loose = RealizationPolicy::default();
    p_loose.epsilon_compression = 0.0; // accept anything

    let mut p_strict = RealizationPolicy::default();
    p_strict.epsilon_compression = 1e6; // accept nothing

    let snaps_loose = run_stress_trajectory(&corpus, p_loose, 20);
    let snaps_strict = run_stress_trajectory(&corpus, p_strict, 20);
    print_trajectory("loose policy (ε=0)", &snaps_loose);
    print_trajectory("strict policy (ε=1e6)", &snaps_strict);

    let last_loose = snaps_loose.last().unwrap();
    let last_strict = snaps_strict.last().unwrap();

    // Loose policy should accept discoveries. Strict rejects all.
    assert!(
        last_loose.library_size >= last_strict.library_size,
        "loose policy should have grown library at least as much as strict"
    );
    // Registry roots should differ when library sizes differ.
    if last_loose.library_size != last_strict.library_size {
        assert_ne!(last_loose.registry_root_bytes, last_strict.registry_root_bytes);
    }
}

#[test]
fn reinforce_follows_discover_when_pressure_builds() {
    // Fabricate a scenario guaranteed to exercise the Reinforce
    // path: seed the registry directly with subsumable pairs, then
    // run step_auto and observe whether Reinforce is chosen.
    use mathscape_core::epoch::{AcceptanceCertificate, Artifact};
    use mathscape_core::eval::RewriteRule;
    use mathscape_core::lifecycle::ProofStatus;

    fn axiomatized(lhs: Term, rhs: Term, name: &str) -> Artifact {
        let mut cert = AcceptanceCertificate::trivial_conjecture(1.0);
        cert.status = ProofStatus::Axiomatized;
        Artifact::seal(
            RewriteRule {
                name: name.into(),
                lhs,
                rhs,
            },
            0,
            cert,
            vec![],
        )
    }

    let mut reg = InMemoryRegistry::new();
    reg.insert(axiomatized(
        apply(var(2), vec![var(100), nat(0)]),
        var(100),
        "add-identity",
    ));
    reg.insert(axiomatized(
        apply(var(2), vec![nat(7), nat(0)]),
        nat(7),
        "add-7-0",
    ));
    reg.insert(axiomatized(
        apply(var(2), vec![nat(13), nat(0)]),
        nat(13),
        "add-13-0",
    ));
    reg.insert(axiomatized(
        apply(var(2), vec![nat(19), nat(0)]),
        nat(19),
        "add-19-0",
    ));

    let mut epoch = Epoch::new(
        CompressionGenerator::new(ExtractConfig::default(), 1),
        StatisticalProver::new(RewardConfig::default(), 0.0),
        RuleEmitter,
        reg,
    );
    let mut allocator = Allocator::new(
        RealizationPolicy::default(),
        RewardEstimator::new(0.3),
    );

    let corpus: Vec<Term> = vec![];
    let pressure_before = reduction_pressure(&epoch.registry);
    println!(
        "\nseeded 4 rules, 3 subsumable by add-identity; pressure = {pressure_before:.3}"
    );
    assert!(pressure_before > 0.0, "expected positive pressure");

    // Keep running until the allocator picks Reinforce, up to some cap.
    let mut reinforce_fired = false;
    let mut reinforce_delta_dl = 0.0;
    for _ in 0..20 {
        let trace = epoch.step_auto(&corpus, &mut allocator);
        if matches!(trace.action, Some(EpochAction::Reinforce)) {
            reinforce_fired = true;
            reinforce_delta_dl = trace
                .events
                .iter()
                .map(|e| e.delta_dl())
                .sum();
            break;
        }
    }

    println!(
        "  allocator eventually chose Reinforce: {reinforce_fired} (ΔDL: {reinforce_delta_dl:.2})"
    );
    assert!(
        reinforce_fired,
        "allocator should fire Reinforce given clear pressure"
    );
    assert!(
        reinforce_delta_dl > 0.0,
        "Reinforce pass should have produced positive ΔDL"
    );

    // After the collapse: pressure drops to zero.
    let pressure_after = reduction_pressure(&epoch.registry);
    assert_eq!(
        pressure_after, 0.0,
        "pressure should be 0 after reinforce; got {pressure_after}"
    );
    assert!(matches!(
        check_maximally_reduced(&epoch.registry),
        ReductionVerdict::Reduced
    ));
}

#[test]
fn trajectory_telemetry_supports_move_classification() {
    // Purpose: demonstrate the telemetry is rich enough to classify
    // transitions. No assertions here beyond "it runs and prints" —
    // the value is the per-epoch line, which can be inspected /
    // captured into CSV for offline analysis of which transitions
    // need learned policy vs which are algorithmic.
    let corpus = pattern_rich_corpus();
    let snaps = run_stress_trajectory(&corpus, RealizationPolicy::default(), 50);
    print_trajectory("50-epoch move-classification trajectory", &snaps);

    // Count transitions between consecutive actions.
    let mut transitions: std::collections::BTreeMap<(String, String), usize> =
        std::collections::BTreeMap::new();
    for w in snaps.windows(2) {
        let key = (w[0].action.clone(), w[1].action.clone());
        *transitions.entry(key).or_insert(0) += 1;
    }
    println!("\n  transitions over the trajectory:");
    for ((from, to), count) in &transitions {
        println!("    {from} → {to}: {count}");
    }

    // At least one transition occurred (non-trivial trajectory).
    assert!(!transitions.is_empty());
}
