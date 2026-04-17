use clap::{Parser, Subcommand};
use mathscape_compress::CompressionGenerator;
use mathscape_config::Config;
use mathscape_core::{
    epoch::{Epoch, EpochTrace, InMemoryRegistry, Registry, RuleEmitter},
    event::Event,
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
        None => repl(&config),
    }
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
}

fn run_epoch(
    config: &Config,
    pop: &mut mathscape_evolve::Population,
    epoch: &mut MathscapeEpoch,
    rng: &mut impl rand::Rng,
) -> EpochSummary {
    let corpus: Vec<_> = pop.individuals.iter().map(|i| i.term.clone()).collect();

    // Gates 1–3 (discovery pass) happen here.
    let trace = epoch.step(&corpus);

    // Population feedback: re-compute an aggregate reward view over the
    // post-epoch library so fitness signal stays comparable. The NEW
    // rules added this epoch come from the trace's Accept events.
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
    }
}

fn run_epochs(config: &Config, n_epochs: usize) {
    let mut rng = rand::thread_rng();
    let mut pop = config.to_population();
    pop.initialize(&mut rng);
    let mut epoch = build_epoch(config);

    println!(
        "Running {n_epochs} epochs with population size {}...\n",
        config.population.target_size
    );
    println!(
        "{:>6} {:>8} {:>6} {:>8} {:>5} {:>8} {:>9}",
        "Epoch", "CR", "DL", "Novelty", "|L|", "Diversity", "Accepted"
    );
    println!("{}", "-".repeat(60));

    for epoch_num in 1..=n_epochs {
        let summary = run_epoch(config, &mut pop, &mut epoch, &mut rng);

        println!(
            "{:>6} {:>8.4} {:>6} {:>8.4} {:>5} {:>8.4} {:>9}",
            epoch_num,
            summary.compression_ratio,
            summary.description_length,
            summary.novelty_total,
            epoch.registry.len(),
            pop.diversity(),
            summary.trace.accepted,
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
                let summary = run_epoch(config, &mut pop, &mut epoch, &mut rng);
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
                        run_epoch(config, &mut pop, &mut epoch, &mut rng);
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
