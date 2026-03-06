use clap::{Parser, Subcommand};
use mathscape_config::Config;
use mathscape_reward::compute_reward;
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

fn run_epoch(
    config: &Config,
    pop: &mut mathscape_evolve::Population,
    library: &mut Vec<mathscape_core::eval::RewriteRule>,
    next_symbol_id: &mut u32,
    rng: &mut impl rand::Rng,
) -> mathscape_reward::RewardResult {
    let corpus: Vec<_> = pop.individuals.iter().map(|i| i.term.clone()).collect();
    let extract_cfg = config.to_extract_config();
    let reward_cfg = config.to_reward_config();

    let new_rules =
        mathscape_compress::extract::extract_rules(&corpus, library, next_symbol_id, &extract_cfg);
    library.extend(new_rules.iter().cloned());

    let result = compute_reward(&corpus, library, &new_rules, &reward_cfg);

    for ind in &mut pop.individuals {
        ind.fitness = result.compression_ratio + result.novelty_total * 0.1;
    }
    pop.update_archive();
    pop.inject_elites(config.population.elite_fraction);
    pop.evolve(rng);

    result
}

fn run_epochs(config: &Config, n_epochs: usize) {
    let mut rng = rand::thread_rng();
    let mut pop = config.to_population();
    pop.initialize(&mut rng);

    let mut library = Vec::new();
    let mut next_symbol_id = 1u32;

    println!(
        "Running {n_epochs} epochs with population size {}...\n",
        config.population.target_size
    );
    println!(
        "{:>6} {:>8} {:>6} {:>8} {:>5} {:>8}",
        "Epoch", "CR", "DL", "Novelty", "|L|", "Diversity"
    );
    println!("{}", "-".repeat(50));

    for epoch in 1..=n_epochs {
        let result = run_epoch(config, &mut pop, &mut library, &mut next_symbol_id, &mut rng);

        println!(
            "{epoch:>6} {cr:>8.4} {dl:>6} {nov:>8.4} {lib:>5} {div:>8.4}",
            cr = result.compression_ratio,
            dl = result.description_length,
            nov = result.novelty_total,
            lib = library.len(),
            div = pop.diversity(),
        );
    }

    println!("\nFinal library ({} symbols):", library.len());
    for rule in &library {
        println!("  {}: {} => {}", rule.name, rule.lhs, rule.rhs);
    }
}

fn repl(config: &Config) {
    let mut rng = rand::thread_rng();
    let mut pop = config.to_population();
    pop.initialize(&mut rng);

    let mut library = Vec::new();
    let mut next_symbol_id = 1u32;
    let mut epoch = 0u64;

    println!("Mathscape REPL — type 'help' for commands\n");

    loop {
        print!("mathscape[{epoch}]> ");
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
                println!("  best       — show best individual");
                println!("  parse EXPR — parse and display an s-expression");
                println!("  eval EXPR  — evaluate an expression");
                println!("  config     — show current configuration");
                println!("  quit       — exit");
            }
            "step" | "s" => {
                epoch += 1;
                let result =
                    run_epoch(config, &mut pop, &mut library, &mut next_symbol_id, &mut rng);

                println!(
                    "  epoch={epoch} CR={:.4} DL={} novelty={:.4} |L|={}",
                    result.compression_ratio, result.description_length, result.novelty_total, library.len()
                );
            }
            "pop" => {
                println!("  size: {}", pop.individuals.len());
                println!("  avg fitness: {:.4}", pop.avg_fitness());
                println!("  diversity: {:.4}", pop.diversity());
                println!("  archive cells: {}", pop.archive.len());
            }
            "lib" => {
                if library.is_empty() {
                    println!("  (empty)");
                } else {
                    for rule in &library {
                        println!("  {}: {} => {}", rule.name, rule.lhs, rule.rhs);
                    }
                }
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
                    config.extract.min_shared_size, config.extract.min_matches, config.extract.max_new_rules
                );
                println!("  database.url: {}", config.database.url);
                println!("  store.path: {}", config.store.path);
            }
            "quit" | "q" | "exit" => break,
            cmd if cmd.starts_with("run ") => {
                if let Ok(n) = cmd[4..].trim().parse::<usize>() {
                    for _ in 0..n {
                        epoch += 1;
                        run_epoch(config, &mut pop, &mut library, &mut next_symbol_id, &mut rng);
                    }
                    println!("  ran {n} epochs (now at epoch {epoch})");
                    println!("  |L|={} diversity={:.4}", library.len(), pop.diversity());
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
                Ok(term) => match mathscape_core::eval::eval(&term, &library, 1000) {
                    Ok(result) => println!("  => {result}"),
                    Err(e) => println!("  error: {e}"),
                },
                Err(e) => println!("  parse error: {e}"),
            },
            "" => {}
            _ => println!("  unknown command: '{input}' — type 'help'"),
        }
    }
}
