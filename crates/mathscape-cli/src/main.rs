use clap::{Parser, Subcommand};
use mathscape_axiom_bridge::{run_promotion, BridgeConfig};
use mathscape_compress::{extract::ExtractConfig, CompressionGenerator};
use mathscape_config::Config;
use mathscape_core::{
    control::{Allocator, EpochAction, RealizationPolicy, RegimeDetector, RewardEstimator},
    corpus::{CorpusLog, CorpusSnapshot},
    epoch::{
        AcceptanceCertificate, Artifact, Epoch, EpochTrace, Generator, InMemoryRegistry,
        Registry, RuleEmitter,
    },
    event::Event,
    promotion_gate::{PromotionGate, ThresholdGate},
    term::Term,
    trap::{Trap, TrapDetector},
    value::Value,
};
use mathscape_reward::{compute_reward, StatisticalProver};
use std::io::{self, Write};

#[derive(Parser)]
#[command(name = "mathscape-cli")]
#[command(about = "Interactive REPL for mathscape epoch execution and inspection")]
struct Cli {
    /// Path to YAML config file (default: mathscape.yaml)
    #[arg(long)]
    config: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run N epochs and print summary
    Run {
        /// Number of epochs to run
        #[arg(default_value = "10")]
        epochs: usize,
    },
    /// Run a self-contained promotion demo: build a patterned corpus,
    /// extract rules via CompressionGenerator, fabricate cross-corpus
    /// evidence in CorpusLog, fire ThresholdGate, invoke axiom-forge,
    /// and print the Rust source mathscape discovered.
    PromoteDemo {
        /// Path to write the emitted Rust source (defaults to stdout).
        #[arg(long)]
        output: Option<String>,
    },
    /// Run epochs until the TrapDetector emits its first trap, then
    /// print the trap's registry root + timing. The trap is a
    /// fixed point of the mathscape machine — a registry root stable
    /// across the detector's observation window.
    TrapsFind {
        /// Maximum epochs to run before giving up.
        #[arg(default_value = "30")]
        max_epochs: usize,
    },
}

fn main() {
    let cli = Cli::parse();

    let config = if let Some(path) = &cli.config {
        mathscape_config::load_from(path).unwrap_or_else(|e| {
            eprintln!("configuration error: {e}");
            std::process::exit(1);
        })
    } else {
        mathscape_config::load_or_panic()
    };

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| config.log_level.clone().into()),
        )
        .init();

    match cli.command {
        Some(Commands::Run { epochs }) => run_epochs(&config, epochs),
        Some(Commands::PromoteDemo { output }) => promote_demo(output.as_deref()),
        Some(Commands::TrapsFind { max_epochs }) => traps_find(&config, max_epochs),
        None => repl(&config),
    }
}

/// Run epochs until the first trap is detected or max_epochs is
/// reached. Prints the trap's registry root, entry epoch, stability
/// window.
fn traps_find(config: &Config, max_epochs: usize) {
    let mut rng = rand::thread_rng();
    let mut pop = config.to_population();
    pop.initialize(&mut rng);
    let mut epoch = build_epoch(config);
    let mut control = ControlState::new(RealizationPolicy::default());

    println!("▶ Running up to {max_epochs} epochs looking for the first trap ...\n");
    for epoch_num in 1..=max_epochs {
        let _summary = run_epoch(config, &mut pop, &mut epoch, &mut control, &mut rng);
        if let Some(trap) = control.trap_history.first() {
            println!("✓ Trap found after epoch {epoch_num}");
            println!("  registry_root:    {}", trap.registry_root);
            println!("  epoch_entered:    {}", trap.epoch_id_entered);
            println!("  stability_window: {}", control.traps.window);
            println!("  policy_hash:      {}", trap.policy_hash);
            println!("  trap content_hash: {}", trap.content_hash);
            println!("  |L| at trap:       {}", epoch.registry.len());
            return;
        }
    }
    println!("✗ No trap found in {max_epochs} epochs.");
    println!("  Current registry root: {}", epoch.registry.root());
    println!("  Library size:          {}", epoch.registry.len());
    println!("  Hint: raise max_epochs, or the registry is still evolving.");
}

/// Operator-visible end-to-end promotion demo.
///
/// Uses a small hand-crafted corpus with a clear pattern, runs the
/// real CompressionGenerator to extract a candidate rule, fabricates
/// the cross-corpus evidence that a multi-epoch run would accumulate
/// naturally, fires the PromotionGate + bridge, and prints (or
/// writes) the Rust source axiom-forge emitted.
fn promote_demo(output_path: Option<&str>) {
    fn var(id: u32) -> Term {
        Term::Var(id)
    }
    fn nat(n: u64) -> Term {
        Term::Number(Value::Nat(n))
    }
    fn apply(f: Term, args: Vec<Term>) -> Term {
        Term::Apply(Box::new(f), args)
    }

    let corpus = vec![
        apply(var(2), vec![nat(3), nat(0)]),
        apply(var(2), vec![nat(5), nat(0)]),
        apply(var(2), vec![nat(7), nat(0)]),
        apply(var(2), vec![nat(11), nat(0)]),
        apply(var(2), vec![nat(13), nat(0)]),
    ];
    let corpus_a = CorpusSnapshot::new("arith", corpus.clone(), 0);
    let corpus_b = CorpusSnapshot::new("combinators", corpus.clone(), 1);

    println!("═══ mathscape promote-demo ═══\n");
    println!("Corpus A ({}): {} terms", corpus_a.id, corpus_a.terms.len());
    println!("Corpus B ({}): {} terms", corpus_b.id, corpus_b.terms.len());

    let mut g = CompressionGenerator::new(
        ExtractConfig {
            min_shared_size: 2,
            min_matches: 2,
            max_new_rules: 1,
        },
        1,
    );
    let candidates = g.propose(0, &corpus, &[]);
    let Some(candidate) = candidates.into_iter().next() else {
        eprintln!("error: generator produced no candidates for this corpus");
        std::process::exit(1);
    };
    println!(
        "\n▶ CompressionGenerator proposed: {} :: {} => {}",
        candidate.rule.name, candidate.rule.lhs, candidate.rule.rhs
    );

    let artifact = Artifact::seal(
        candidate.rule.clone(),
        0,
        AcceptanceCertificate::trivial_conjecture(1.0),
        vec![],
    );
    println!(
        "▶ Sealed as Artifact with content_hash: {}",
        artifact.content_hash
    );

    // Accumulate cross-corpus evidence via CorpusLog — what a multi-
    // epoch run would produce naturally.
    let mut log = CorpusLog::new();
    log.scan_corpus(
        &corpus_a,
        [(artifact.content_hash, artifact.rule.lhs.clone())],
        0,
    );
    log.scan_corpus(
        &corpus_b,
        [(artifact.content_hash, artifact.rule.lhs.clone())],
        1,
    );
    let history = log.history_for(artifact.content_hash, 2, 100);
    println!(
        "▶ CorpusLog evidence: corpus_matches={}, epochs_alive={}, usage_in_window={}",
        history.corpus_matches.len(),
        history.epochs_alive,
        history.usage_in_window,
    );

    let gate = ThresholdGate::new(0, 2);
    let signal = match gate.evaluate(&artifact, &[artifact.clone()], &history, 2) {
        Some(s) => s,
        None => {
            eprintln!("error: PromotionGate did not fire. Check thresholds + history.");
            std::process::exit(1);
        }
    };
    println!("▶ PromotionGate fired: {}", signal.rationale);
    println!("  signal content_hash: {}", signal.content_hash());

    let receipt = match run_promotion(&signal, &artifact, &BridgeConfig::default()) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: bridge rejected the promotion: {e}");
            std::process::exit(1);
        }
    };
    println!(
        "\n✓ axiom-forge accepted the proposal (gate 6)",
    );
    println!("  axiom_identity: {}::{} ({})",
        receipt.axiom_identity.target,
        receipt.axiom_identity.name,
        receipt.axiom_identity.proposal_hash,
    );
    println!(
        "  frozen_vector.b3sum_hex: {}",
        receipt.frozen_vector.b3sum_hex
    );

    let emitted_source = format!(
        "// Mathscape-discovered primitive\n\
         // axiom_identity: {target}::{name}\n\
         // proposal_hash:  {ph}\n\
         // canonical_text: {ct}\n\
         // b3sum:          {b3}\n\
         //\n\
         // Declaration:\n\
         {decl}\n\
         \n\
         // Documentation:\n\
         {doc}\n\
         \n\
         // to_sexpr arm:\n\
         {to_arm}\n\
         \n\
         // from_sexpr arm:\n\
         {from_arm}\n",
        target = receipt.axiom_identity.target,
        name = receipt.axiom_identity.name,
        ph = receipt.axiom_identity.proposal_hash,
        ct = receipt.frozen_vector.canonical_text,
        b3 = receipt.frozen_vector.b3sum_hex,
        decl = receipt.emission.declaration,
        doc = receipt.emission.doc_block,
        to_arm = receipt.emission.to_sexpr_arm,
        from_arm = receipt.emission.from_sexpr_arm,
    );

    match output_path {
        Some(path) => {
            if let Err(e) = std::fs::write(path, &emitted_source) {
                eprintln!("error: failed to write {path}: {e}");
                std::process::exit(1);
            }
            println!("\n✓ Emitted Rust source written to {path}");
        }
        None => {
            println!("\n───── emitted Rust source ─────");
            print!("{emitted_source}");
            println!("──────────────────────────────");
        }
    }
    println!("\n═══ promote-demo complete — gate 7 (rustc) left to the caller ═══");
}

/// Concrete type of the v0 mathscape epoch — fixed quad of
/// adapters. Later phases swap the roles via `EpochAction` dispatch.
type MathscapeEpoch =
    Epoch<CompressionGenerator, StatisticalProver, RuleEmitter, InMemoryRegistry>;

/// Build the v0 epoch from config.
fn build_epoch(config: &Config) -> MathscapeEpoch {
    let extract_cfg = config.to_extract_config();
    let reward_cfg = config.to_reward_config();
    Epoch::new(
        CompressionGenerator::new(extract_cfg, 1),
        StatisticalProver::new(reward_cfg, 0.0),
        RuleEmitter,
        InMemoryRegistry::new(),
    )
}

/// Per-epoch summary derived from the trace + a fresh reward
/// computation for population feedback. The trace drives the
/// cryptographic story; `compute_reward` over the post-epoch library
/// keeps the fitness signal exactly comparable to the pre-refactor
/// CLI.
struct EpochSummary {
    trace: EpochTrace,
    compression_ratio: f64,
    description_length: usize,
    novelty_total: f64,
    regime: mathscape_core::control::Regime,
    action: EpochAction,
}

/// Control-plane state that persists across epochs. Consolidated so
/// the CLI can print regime + ΔDL trajectory + trap emissions without
/// threading multiple args through run_epoch.
struct ControlState {
    allocator: Allocator,
    detector: RegimeDetector,
    traps: TrapDetector,
    /// Traps the detector has emitted so far, in order.
    trap_history: Vec<Trap>,
}

impl ControlState {
    fn new(policy: RealizationPolicy) -> Self {
        let estimator = RewardEstimator::new(0.3);
        Self {
            allocator: Allocator::new(policy, estimator),
            detector: RegimeDetector::new(10),
            traps: TrapDetector::new(3),
            trap_history: Vec::new(),
        }
    }
}

fn run_epoch(
    config: &Config,
    pop: &mut mathscape_evolve::Population,
    epoch: &mut MathscapeEpoch,
    control: &mut ControlState,
    rng: &mut impl rand::Rng,
) -> EpochSummary {
    let corpus: Vec<_> = pop.individuals.iter().map(|i| i.term.clone()).collect();

    // Allocator chooses the action for this epoch based on estimator
    // state. Promote/Migrate are epoch-level no-ops (executed out-of-
    // crate by the bridge); for now the CLI only sees Reinforce and
    // Discover, which is the common case.
    let action = control
        .allocator
        .choose(corpus.len(), epoch.registry.len());

    // Gates 1–3 (discovery pass) or the reinforce scaffold, depending
    // on action.
    let trace = epoch.step_with_action(&corpus, action.clone());

    // Feed the trace back into the estimator + detector.
    control.allocator.estimator.update(&trace.events);
    let regime = control.detector.observe(&trace);

    // Trap detection: observe the post-epoch registry root. If the
    // detector emits a Trap, stash it.
    if let Some(trap) = control.traps.observe(epoch.registry.root(), epoch.epoch_id) {
        control.trap_history.push(trap);
    }

    // Population feedback: re-compute an aggregate reward view over the
    // post-epoch library so fitness signal stays comparable.
    let new_rules: Vec<_> = trace
        .events
        .iter()
        .filter_map(|e| match e {
            Event::Accept { artifact, .. } => Some(artifact.rule.clone()),
            _ => None,
        })
        .collect();
    let library: Vec<_> = epoch.registry.all().iter().map(|a| a.rule.clone()).collect();
    let reward_cfg = config.to_reward_config();
    let result = compute_reward(&corpus, &library, &new_rules, &reward_cfg);

    for ind in &mut pop.individuals {
        ind.fitness = result.compression_ratio + result.novelty_total * 0.1;
    }
    pop.update_archive();
    pop.inject_elites(config.population.elite_fraction);
    pop.evolve(rng);

    EpochSummary {
        trace,
        compression_ratio: result.compression_ratio,
        description_length: result.description_length,
        novelty_total: result.novelty_total,
        regime,
        action,
    }
}

fn run_epochs(config: &Config, n_epochs: usize) {
    let mut rng = rand::thread_rng();
    let mut pop = config.to_population();
    pop.initialize(&mut rng);
    let mut epoch = build_epoch(config);
    let mut control = ControlState::new(RealizationPolicy::default());

    println!(
        "Running {n_epochs} epochs with population size {}...\n",
        config.population.target_size
    );
    println!(
        "{:>6} {:>10} {:>9} {:>8} {:>6} {:>8} {:>5} {:>5} {:>5}",
        "Epoch", "Action", "Regime", "CR", "DL", "Novelty", "|L|", "Acc", "Traps"
    );
    println!("{}", "-".repeat(76));

    for epoch_num in 1..=n_epochs {
        let summary = run_epoch(config, &mut pop, &mut epoch, &mut control, &mut rng);

        let action_str = match summary.action {
            EpochAction::Discover => "Discover",
            EpochAction::Reinforce => "Reinforce",
            EpochAction::Promote(_) => "Promote",
            EpochAction::Migrate(_) => "Migrate",
        };
        let regime_str = format!("{:?}", summary.regime);

        println!(
            "{:>6} {:>10} {:>9} {:>8.4} {:>6} {:>8.4} {:>5} {:>5} {:>5}",
            epoch_num,
            action_str,
            regime_str,
            summary.compression_ratio,
            summary.description_length,
            summary.novelty_total,
            epoch.registry.len(),
            summary.trace.accepted,
            control.trap_history.len(),
        );
    }

    println!("\nFinal library ({} artifacts):", epoch.registry.len());
    for artifact in epoch.registry.all() {
        println!(
            "  {}: {} => {}",
            artifact.rule.name, artifact.rule.lhs, artifact.rule.rhs
        );
    }
}

fn repl(config: &Config) {
    let mut rng = rand::thread_rng();
    let mut pop = config.to_population();
    pop.initialize(&mut rng);
    let mut epoch = build_epoch(config);
    let mut control = ControlState::new(RealizationPolicy::default());
    let mut epoch_ctr = 0u64;

    println!("Mathscape REPL — type 'help' for commands\n");

    loop {
        print!("mathscape[{epoch_ctr}]> ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() {
            break;
        }
        let input = input.trim();

        match input {
            "help" | "h" => {
                println!("  step       — run one epoch");
                println!("  run N      — run N epochs");
                println!("  pop        — show population stats");
                println!("  lib        — show library");
                println!("  trace      — show last epoch's event stream");
                println!("  best       — show best individual");
                println!("  parse EXPR — parse and display an s-expression");
                println!("  eval EXPR  — evaluate an expression");
                println!("  config     — show current configuration");
                println!("  quit       — exit");
            }
            "step" | "s" => {
                epoch_ctr += 1;
                let summary = run_epoch(config, &mut pop, &mut epoch, &mut control, &mut rng);
                println!(
                    "  epoch={epoch_ctr} CR={:.4} DL={} novelty={:.4} |L|={} accepted={}",
                    summary.compression_ratio,
                    summary.description_length,
                    summary.novelty_total,
                    epoch.registry.len(),
                    summary.trace.accepted,
                );
            }
            "pop" => {
                println!("  size: {}", pop.individuals.len());
                println!("  avg fitness: {:.4}", pop.avg_fitness());
                println!("  diversity: {:.4}", pop.diversity());
                println!("  archive cells: {}", pop.archive.len());
            }
            "lib" => {
                if epoch.registry.is_empty() {
                    println!("  (empty)");
                } else {
                    for a in epoch.registry.all() {
                        println!("  {}: {} => {}", a.rule.name, a.rule.lhs, a.rule.rhs);
                    }
                }
            }
            "trace" => {
                println!(
                    "  epoch {} action={:?} events={} accepted={} rejected={}",
                    epoch_ctr.saturating_sub(1),
                    "Discover",
                    epoch.registry.len(),
                    0, // last trace not retained in v0 REPL
                    0,
                );
            }
            "best" => {
                if let Some(best) = pop.best() {
                    println!("  fitness: {:.4}", best.fitness);
                    println!("  term: {}", best.term);
                    println!("  size: {}", best.term.size());
                    println!("  depth: {}", best.term.depth());
                }
            }
            "config" => {
                println!("  population.target_size: {}", config.population.target_size);
                println!("  population.max_depth: {}", config.population.max_depth);
                println!("  population.tournament_k: {}", config.population.tournament_k);
                println!(
                    "  reward: alpha={}, beta={}, gamma={}",
                    config.reward.alpha, config.reward.beta, config.reward.gamma
                );
                println!(
                    "  extract: min_shared={}, min_matches={}, max_new={}",
                    config.extract.min_shared_size,
                    config.extract.min_matches,
                    config.extract.max_new_rules,
                );
                println!("  database.url: {}", config.database.url);
                println!("  store.path: {}", config.store.path);
            }
            "quit" | "q" | "exit" => break,
            cmd if cmd.starts_with("run ") => {
                if let Ok(n) = cmd[4..].trim().parse::<usize>() {
                    for _ in 0..n {
                        epoch_ctr += 1;
                        run_epoch(config, &mut pop, &mut epoch, &mut control, &mut rng);
                    }
                    println!("  ran {n} epochs (now at epoch {epoch_ctr})");
                    println!(
                        "  |L|={} diversity={:.4}",
                        epoch.registry.len(),
                        pop.diversity()
                    );
                }
            }
            cmd if cmd.starts_with("parse ") => match mathscape_core::parse::parse(&cmd[6..]) {
                Ok(term) => {
                    println!("  parsed: {term}");
                    println!("  size: {} depth: {}", term.size(), term.depth());
                }
                Err(e) => println!("  error: {e}"),
            },
            cmd if cmd.starts_with("eval ") => match mathscape_core::parse::parse(&cmd[5..]) {
                Ok(term) => {
                    let library: Vec<_> =
                        epoch.registry.all().iter().map(|a| a.rule.clone()).collect();
                    match mathscape_core::eval::eval(&term, &library, 1000) {
                        Ok(result) => println!("  => {result}"),
                        Err(e) => println!("  error: {e}"),
                    }
                }
                Err(e) => println!("  parse error: {e}"),
            },
            "" => {}
            _ => println!("  unknown command: '{input}' — type 'help'"),
        }
    }
}
