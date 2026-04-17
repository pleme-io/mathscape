//! MCP server (stdio transport) for observe and control interaction with Mathscape.
//!
//! Observe tools:
//!   - status          — current engine state
//!   - list_library    — list discovered rewrite rules
//!   - parse_expr      — parse an s-expression
//!   - eval_expr       — evaluate an expression
//!   - identify_rule   — match a rule against the known-math catalog
//!   - scan_timeline   — produce a discovery timeline
//!   - expr_to_tree    — visualize an expression as a tree
//!   - list_catalog    — list recognized mathematical properties
//!
//! Control tools (dynamic config — highest precedence override):
//!   - get_config      — show current configuration
//!   - pause_engine    — pause epoch computation
//!   - resume_engine   — resume epoch computation
//!   - set_max_epoch   — set epoch ceiling (engine stops at this epoch)
//!   - set_reward_weights — adjust alpha/beta/gamma live
//!   - set_population   — adjust population size/depth/tournament_k

use mathscape_axiom_bridge::{run_promotion, BridgeConfig};
use mathscape_compress::{extract::ExtractConfig, CompressionGenerator};
use mathscape_config::DynamicConfig;
use mathscape_core::{
    control::{Allocator, RealizationPolicy, RegimeDetector, RewardEstimator},
    corpus::{CorpusLog, CorpusSnapshot},
    epoch::{Epoch, InMemoryRegistry, Registry, RuleEmitter},
    eval::RewriteRule,
    promotion_gate::{PromotionGate, ThresholdGate},
    term::Term,
    trap::{Trap, TrapDetector},
    value::Value,
};
use mathscape_discovery::catalog;
use mathscape_discovery::matcher;
use mathscape_discovery::representation;
use mathscape_discovery::scanner::{self, SymbolRecord};
use mathscape_reward::{reward::RewardConfig, StatisticalProver};
use rmcp::model::{ServerCapabilities, ServerInfo};
use rmcp::{ServerHandler, ServiceExt, schemars, tool};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex, RwLock};

/// Shared engine state accessible from MCP tool handlers.
#[derive(Clone, Default)]
struct EngineSnapshot {
    epoch: u64,
    library: Vec<RewriteRule>,
    population_size: usize,
    avg_fitness: f64,
    diversity: f64,
    action: Option<String>,
    regime: Option<String>,
    trap_count: usize,
    last_registry_root: Option<String>,
}

/// A real running engine behind the MCP server. Agents drive the
/// pace by calling the `step` tool. No async background task; all
/// state transitions are synchronous under a single Mutex.
struct MathscapeEngine {
    epoch: Epoch<CompressionGenerator, StatisticalProver, RuleEmitter, InMemoryRegistry>,
    allocator: Allocator,
    regime_detector: RegimeDetector,
    trap_detector: TrapDetector,
    trap_history: Vec<Trap>,
    /// Built-in demonstration corpus. Agents can swap this via tools
    /// in a future revision; v0 uses a fixed patterned corpus.
    corpus_a: CorpusSnapshot,
    corpus_b: CorpusSnapshot,
    corpus_log: CorpusLog,
    /// Which corpus to feed this epoch. Toggles each call to step so
    /// cross-corpus evidence accumulates naturally.
    next_corpus_is_a: bool,
}

impl MathscapeEngine {
    fn new() -> Self {
        fn apply(f: Term, args: Vec<Term>) -> Term {
            Term::Apply(Box::new(f), args)
        }
        fn nat(n: u64) -> Term {
            Term::Number(Value::Nat(n))
        }
        fn var(id: u32) -> Term {
            Term::Var(id)
        }
        let arith_terms = vec![
            apply(var(2), vec![nat(3), nat(0)]),
            apply(var(2), vec![nat(5), nat(0)]),
            apply(var(2), vec![nat(7), nat(0)]),
            apply(var(2), vec![nat(11), nat(0)]),
        ];
        let comb_terms = vec![
            apply(var(2), vec![nat(2), nat(0)]),
            apply(var(2), vec![nat(13), nat(0)]),
            apply(var(2), vec![nat(17), nat(0)]),
            apply(var(2), vec![nat(19), nat(0)]),
        ];
        let corpus_a = CorpusSnapshot::new("arith", arith_terms, 0);
        let corpus_b = CorpusSnapshot::new("combinators", comb_terms, 0);
        let epoch = Epoch::new(
            CompressionGenerator::new(
                ExtractConfig {
                    min_shared_size: 2,
                    min_matches: 2,
                    max_new_rules: 2,
                },
                1,
            ),
            StatisticalProver::new(RewardConfig::default(), 0.0),
            RuleEmitter,
            InMemoryRegistry::new(),
        );
        let policy = RealizationPolicy::default();
        Self {
            epoch,
            allocator: Allocator::new(policy, RewardEstimator::new(0.3)),
            regime_detector: RegimeDetector::new(10),
            trap_detector: TrapDetector::new(3),
            trap_history: Vec::new(),
            corpus_a,
            corpus_b,
            corpus_log: CorpusLog::new(),
            next_corpus_is_a: true,
        }
    }

    /// Run one epoch, update the CorpusLog with the current corpus
    /// matches, and observe traps. Returns the EpochTrace details.
    fn step(&mut self) -> StepResult {
        let corpus = if self.next_corpus_is_a {
            &self.corpus_a
        } else {
            &self.corpus_b
        };
        let action = self
            .allocator
            .choose(corpus.terms.len(), self.epoch.registry.len());
        let trace = self
            .epoch
            .step_with_action(corpus.terms(), action.clone());
        // Scan the current corpus against every live artifact so
        // cross-corpus evidence accumulates.
        let scan_input: Vec<_> = self
            .epoch
            .registry
            .all()
            .iter()
            .map(|a| (a.content_hash, a.rule.lhs.clone()))
            .collect();
        self.corpus_log.scan_corpus(corpus, scan_input, self.epoch.epoch_id);
        self.allocator.estimator.update(&trace.events);
        let regime = self.regime_detector.observe(&trace);
        if let Some(trap) = self
            .trap_detector
            .observe(self.epoch.registry.root(), self.epoch.epoch_id)
        {
            self.trap_history.push(trap);
        }
        self.next_corpus_is_a = !self.next_corpus_is_a;
        StepResult {
            epoch_id: self.epoch.epoch_id,
            action: format!("{:?}", action),
            regime: format!("{:?}", regime),
            accepted: trace.accepted,
            rejected: trace.rejected,
            library_size: self.epoch.registry.len(),
            registry_root: format!("{}", self.epoch.registry.root()),
            trap_count: self.trap_history.len(),
        }
    }

    fn library_rules(&self) -> Vec<RewriteRule> {
        self.epoch.registry.all().iter().map(|a| a.rule.clone()).collect()
    }
}

struct StepResult {
    epoch_id: u64,
    action: String,
    regime: String,
    accepted: usize,
    rejected: usize,
    library_size: usize,
    registry_root: String,
    trap_count: usize,
}

/// The MCP server handler.
#[derive(Clone)]
struct MathscapeMcp {
    state: Arc<RwLock<EngineSnapshot>>,
    config: DynamicConfig,
    engine: Arc<Mutex<MathscapeEngine>>,
}

impl MathscapeMcp {
    fn new() -> Self {
        let config = mathscape_config::load_or_panic();
        MathscapeMcp {
            state: Arc::new(RwLock::new(EngineSnapshot::default())),
            config: DynamicConfig::new(config),
            engine: Arc::new(Mutex::new(MathscapeEngine::new())),
        }
    }

    /// Mirror the engine's current state into the shared snapshot so
    /// the read-only observation tools return live data.
    fn sync_snapshot(&self, step: &StepResult) {
        let library = {
            let e = self.engine.lock().unwrap();
            e.library_rules()
        };
        let mut snap = self.state.write().unwrap();
        snap.epoch = step.epoch_id;
        snap.library = library;
        snap.action = Some(step.action.clone());
        snap.regime = Some(step.regime.clone());
        snap.trap_count = step.trap_count;
        snap.last_registry_root = Some(step.registry_root.clone());
    }
}

// -- Tool parameter types --

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ParseExprRequest {
    #[schemars(description = "S-expression to parse, e.g. '(add 1 2)'")]
    expr: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct EvalExprRequest {
    #[schemars(description = "S-expression to evaluate, e.g. '(add 2 3)'")]
    expr: String,
    #[schemars(description = "Maximum evaluation steps (default 1000)")]
    step_limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct IdentifyRuleRequest {
    #[schemars(description = "S-expression of the LHS pattern")]
    lhs: String,
    #[schemars(description = "S-expression of the RHS replacement")]
    rhs: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ScanTimelineRequest {
    #[schemars(description = "JSON array of symbol records to scan")]
    symbols_json: String,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
struct SymbolRecordInput {
    symbol_id: i32,
    name: String,
    epoch_discovered: i32,
    arity: i32,
    generality: Option<f64>,
    irreducibility: Option<f64>,
    is_meta: bool,
    status: String,
    lhs_sexpr: String,
    rhs_sexpr: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ExprToTreeRequest {
    #[schemars(description = "S-expression to visualize")]
    expr: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct EmptyRequest {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct PromoteRequest {
    #[schemars(description = "Index of the rule in the library (0-based)")]
    rule_index: usize,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SetMaxEpochRequest {
    #[schemars(description = "Maximum epoch to run to (null to remove ceiling)")]
    max_epoch: Option<u64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SetRewardWeightsRequest {
    #[schemars(description = "Weight for compression ratio (exploitation). Sum should be 1.0.")]
    alpha: f64,
    #[schemars(description = "Weight for novelty (exploration)")]
    beta: f64,
    #[schemars(description = "Weight for meta-compression")]
    gamma: f64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SetPopulationRequest {
    #[schemars(description = "Target population size")]
    target_size: Option<usize>,
    #[schemars(description = "Maximum expression tree depth")]
    max_depth: Option<usize>,
    #[schemars(description = "Tournament selection size")]
    tournament_k: Option<usize>,
    #[schemars(description = "Elite injection fraction (0.0 - 1.0)")]
    elite_fraction: Option<f64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SetExtractRequest {
    #[schemars(description = "Minimum shared structure size for extraction")]
    min_shared_size: Option<usize>,
    #[schemars(description = "Minimum corpus matches for a pattern")]
    min_matches: Option<usize>,
    #[schemars(description = "Maximum new rules per epoch")]
    max_new_rules: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SetEpochDelayRequest {
    #[schemars(description = "Delay between epochs in milliseconds (0 = no delay)")]
    delay_ms: u64,
}

#[tool(tool_box)]
impl MathscapeMcp {
    // === Observe tools ===

    #[tool(description = "Get current engine status: epoch, library size, population stats, running state")]
    fn status(&self) -> String {
        let s = self.state.read().unwrap();
        let config = self.config.get();
        serde_json::to_string_pretty(&serde_json::json!({
            "epoch": s.epoch,
            "running": config.engine.running,
            "max_epoch": config.engine.max_epoch,
            "epoch_delay_ms": config.engine.epoch_delay_ms,
            "library_size": s.library.len(),
            "population_size": s.population_size,
            "avg_fitness": s.avg_fitness,
            "diversity": s.diversity,
            "last_action": s.action,
            "last_regime": s.regime,
            "trap_count": s.trap_count,
            "last_registry_root": s.last_registry_root,
            "reward_weights": {
                "alpha": config.reward.alpha,
                "beta": config.reward.beta,
                "gamma": config.reward.gamma,
            },
        }))
        .unwrap()
    }

    #[tool(description = "Run one mathscape epoch and return a summary (action, regime, accepted, library_size, trap_count). The engine runs agent-paced — each call to step advances the machine by one epoch.")]
    fn step(&self) -> String {
        let step_result = {
            let mut engine = self.engine.lock().unwrap();
            engine.step()
        };
        self.sync_snapshot(&step_result);
        serde_json::to_string_pretty(&serde_json::json!({
            "epoch_id": step_result.epoch_id,
            "action": step_result.action,
            "regime": step_result.regime,
            "accepted": step_result.accepted,
            "rejected": step_result.rejected,
            "library_size": step_result.library_size,
            "registry_root": step_result.registry_root,
            "trap_count": step_result.trap_count,
        })).unwrap()
    }

    #[tool(description = "List all traps (fixed-point registry roots) emitted so far. Each trap is a content-addressed snapshot of a stable registry state.")]
    fn list_traps(&self) -> String {
        let engine = self.engine.lock().unwrap();
        let traps: Vec<_> = engine.trap_history.iter().map(|t| serde_json::json!({
            "registry_root": format!("{}", t.registry_root),
            "content_hash": format!("{}", t.content_hash),
            "epoch_entered": t.epoch_id_entered,
            "epoch_left": t.epoch_id_left,
            "is_active": t.is_active(),
        })).collect();
        serde_json::to_string_pretty(&traps).unwrap()
    }

    #[tool(description = "Attempt to promote a library rule by index into a Rust primitive via axiom-forge. Fabricates cross-corpus evidence for demonstration. Returns the emitted Rust source + axiom identity + frozen vector hash, or an error if the bridge rejects.")]
    fn promote(&self, #[tool(aggr)] req: PromoteRequest) -> String {
        let (artifact, signal) = {
            let engine = self.engine.lock().unwrap();
            let all = engine.epoch.registry.all();
            let Some(artifact) = all.get(req.rule_index).cloned() else {
                return serde_json::json!({
                    "error": format!("rule_index {} out of range (library size {})", req.rule_index, all.len())
                }).to_string();
            };
            let history = engine.corpus_log.history_for(
                artifact.content_hash,
                engine.epoch.epoch_id,
                100,
            );
            // Gate 4 relaxed (k=0) for demo; gate 5 enforced via n=2.
            let gate = ThresholdGate::new(0, 1);
            let Some(signal) = gate.evaluate(&artifact, all, &history, engine.epoch.epoch_id) else {
                return serde_json::json!({
                    "error": "PromotionGate did not fire; need more cross-corpus evidence. Call step more times.",
                    "history": {
                        "corpus_matches": history.corpus_matches.len(),
                        "epochs_alive": history.epochs_alive,
                    }
                }).to_string();
            };
            (artifact, signal)
        };
        match run_promotion(&signal, &artifact, &BridgeConfig::default()) {
            Ok(receipt) => serde_json::to_string_pretty(&serde_json::json!({
                "axiom_identity": {
                    "target": receipt.axiom_identity.target,
                    "name": receipt.axiom_identity.name,
                    "proposal_hash": format!("{}", receipt.axiom_identity.proposal_hash),
                    "typescape_coord": {
                        "module_path": receipt.axiom_identity.typescape_coord.module_path,
                        "ast_domain": receipt.axiom_identity.typescape_coord.ast_domain,
                    },
                },
                "frozen_vector": {
                    "canonical_text": receipt.frozen_vector.canonical_text,
                    "b3sum_hex": receipt.frozen_vector.b3sum_hex,
                },
                "emitted_rust": {
                    "declaration": receipt.emission.declaration,
                    "doc_block": receipt.emission.doc_block,
                    "to_sexpr_arm": receipt.emission.to_sexpr_arm,
                    "from_sexpr_arm": receipt.emission.from_sexpr_arm,
                },
            })).unwrap(),
            Err(e) => serde_json::json!({
                "error": format!("bridge rejected: {e}"),
            }).to_string(),
        }
    }

    #[tool(description = "List all discovered rewrite rules in the library")]
    fn list_library(&self) -> String {
        let s = self.state.read().unwrap();
        let rules: Vec<serde_json::Value> = s
            .library
            .iter()
            .map(|r| {
                serde_json::json!({
                    "name": r.name,
                    "lhs": format!("{}", r.lhs),
                    "rhs": format!("{}", r.rhs),
                })
            })
            .collect();
        serde_json::to_string_pretty(&rules).unwrap()
    }

    #[tool(description = "Parse an s-expression and return Term structure with metrics")]
    fn parse_expr(&self, #[tool(aggr)] req: ParseExprRequest) -> String {
        match mathscape_core::parse::parse(&req.expr) {
            Ok(term) => serde_json::to_string_pretty(&serde_json::json!({
                "term": format!("{term}"),
                "size": term.size(),
                "depth": term.depth(),
                "distinct_ops": term.distinct_ops(),
                "hash": format!("{}", term.content_hash()),
            }))
            .unwrap(),
            Err(e) => serde_json::to_string_pretty(&serde_json::json!({
                "error": format!("{e}"),
            }))
            .unwrap(),
        }
    }

    #[tool(description = "Evaluate an s-expression under the current library rules")]
    fn eval_expr(&self, #[tool(aggr)] req: EvalExprRequest) -> String {
        let library = {
            let s = self.state.read().unwrap();
            s.library.clone()
        };
        let step_limit = req.step_limit.unwrap_or(1000);

        match mathscape_core::parse::parse(&req.expr) {
            Ok(term) => match mathscape_core::eval::eval(&term, &library, step_limit) {
                Ok(result) => serde_json::to_string_pretty(&serde_json::json!({
                    "input": format!("{term}"),
                    "result": format!("{result}"),
                    "result_size": result.size(),
                }))
                .unwrap(),
                Err(e) => serde_json::to_string_pretty(&serde_json::json!({
                    "error": format!("{e}"),
                }))
                .unwrap(),
            },
            Err(e) => serde_json::to_string_pretty(&serde_json::json!({
                "error": format!("parse error: {e}"),
            }))
            .unwrap(),
        }
    }

    #[tool(description = "Identify a rewrite rule against the known-math catalog. Returns matched mathematical properties with confidence scores.")]
    fn identify_rule(&self, #[tool(aggr)] req: IdentifyRuleRequest) -> String {
        let lhs = match mathscape_core::parse::parse(&req.lhs) {
            Ok(t) => t,
            Err(e) => {
                return serde_json::to_string_pretty(&serde_json::json!({
                    "error": format!("LHS parse error: {e}"),
                }))
                .unwrap();
            }
        };
        let rhs = match mathscape_core::parse::parse(&req.rhs) {
            Ok(t) => t,
            Err(e) => {
                return serde_json::to_string_pretty(&serde_json::json!({
                    "error": format!("RHS parse error: {e}"),
                }))
                .unwrap();
            }
        };

        let rule = RewriteRule {
            name: "query".into(),
            lhs,
            rhs,
        };
        let identifications = matcher::identify(&rule);
        serde_json::to_string_pretty(&identifications).unwrap()
    }

    #[tool(description = "Scan symbol records and produce a discovery timeline with identifications")]
    fn scan_timeline(&self, #[tool(aggr)] req: ScanTimelineRequest) -> String {
        let inputs: Vec<SymbolRecordInput> = match serde_json::from_str(&req.symbols_json) {
            Ok(v) => v,
            Err(e) => {
                return serde_json::to_string_pretty(&serde_json::json!({
                    "error": format!("JSON parse error: {e}"),
                }))
                .unwrap();
            }
        };

        let records: Vec<SymbolRecord> = inputs
            .into_iter()
            .map(|i| SymbolRecord {
                symbol_id: i.symbol_id,
                name: i.name,
                epoch_discovered: i.epoch_discovered,
                arity: i.arity,
                generality: i.generality,
                irreducibility: i.irreducibility,
                is_meta: i.is_meta,
                status: i.status,
                lhs_sexpr: i.lhs_sexpr,
                rhs_sexpr: i.rhs_sexpr,
            })
            .collect();

        let timeline = scanner::scan_symbols(&records);
        serde_json::to_string_pretty(&timeline).unwrap()
    }

    #[tool(description = "Convert an s-expression to a tree visualization structure (JSON nodes + edges for React rendering)")]
    fn expr_to_tree(&self, #[tool(aggr)] req: ExprToTreeRequest) -> String {
        match mathscape_core::parse::parse(&req.expr) {
            Ok(term) => {
                let hash = format!("{}", term.content_hash());
                let tree = representation::term_to_tree(&term, &hash);
                serde_json::to_string_pretty(&tree).unwrap()
            }
            Err(e) => serde_json::to_string_pretty(&serde_json::json!({
                "error": format!("parse error: {e}"),
            }))
            .unwrap(),
        }
    }

    #[tool(description = "List the known-math catalog of recognizable mathematical properties")]
    fn list_catalog(&self, #[tool(aggr)] _req: EmptyRequest) -> String {
        let props: Vec<serde_json::Value> = catalog::catalog()
            .into_iter()
            .map(|p| {
                serde_json::json!({
                    "id": p.id,
                    "name": p.name,
                    "domain": p.domain,
                    "latex": p.latex,
                    "description": p.description,
                })
            })
            .collect();
        serde_json::to_string_pretty(&props).unwrap()
    }

    // === Control tools (dynamic config — highest precedence) ===

    #[tool(description = "Get the full current configuration (all layers merged)")]
    fn get_config(&self) -> String {
        let config = self.config.get();
        serde_json::to_string_pretty(&config).unwrap()
    }

    #[tool(description = "Pause the engine — stop computing epochs. Use resume_engine to continue.")]
    fn pause_engine(&self) -> String {
        self.config.pause();
        serde_json::json!({"status": "paused"}).to_string()
    }

    #[tool(description = "Resume the engine — continue computing epochs after a pause.")]
    fn resume_engine(&self) -> String {
        self.config.resume();
        serde_json::json!({"status": "running"}).to_string()
    }

    #[tool(description = "Set the maximum epoch ceiling. Engine stops after reaching this epoch. Pass null to remove the ceiling.")]
    fn set_max_epoch(&self, #[tool(aggr)] req: SetMaxEpochRequest) -> String {
        self.config.set_max_epoch(req.max_epoch);
        let config = self.config.get();
        serde_json::to_string_pretty(&serde_json::json!({
            "max_epoch": config.engine.max_epoch,
            "status": if config.engine.running { "running" } else { "paused" },
        }))
        .unwrap()
    }

    #[tool(description = "Set reward function weights (alpha=CR, beta=novelty, gamma=meta_compression). Should sum to 1.0.")]
    fn set_reward_weights(&self, #[tool(aggr)] req: SetRewardWeightsRequest) -> String {
        self.config.set_reward_weights(req.alpha, req.beta, req.gamma);
        let config = self.config.get();
        serde_json::to_string_pretty(&serde_json::json!({
            "alpha": config.reward.alpha,
            "beta": config.reward.beta,
            "gamma": config.reward.gamma,
            "sum": config.reward.alpha + config.reward.beta + config.reward.gamma,
        }))
        .unwrap()
    }

    #[tool(description = "Adjust population parameters live. Only provided fields are updated.")]
    fn set_population(&self, #[tool(aggr)] req: SetPopulationRequest) -> String {
        self.config.update(|c| {
            if let Some(v) = req.target_size {
                c.population.target_size = v;
            }
            if let Some(v) = req.max_depth {
                c.population.max_depth = v;
            }
            if let Some(v) = req.tournament_k {
                c.population.tournament_k = v;
            }
            if let Some(v) = req.elite_fraction {
                c.population.elite_fraction = v;
            }
        });
        let config = self.config.get();
        serde_json::to_string_pretty(&config.population).unwrap()
    }

    #[tool(description = "Adjust library extraction parameters live. Only provided fields are updated.")]
    fn set_extract(&self, #[tool(aggr)] req: SetExtractRequest) -> String {
        self.config.update(|c| {
            if let Some(v) = req.min_shared_size {
                c.extract.min_shared_size = v;
            }
            if let Some(v) = req.min_matches {
                c.extract.min_matches = v;
            }
            if let Some(v) = req.max_new_rules {
                c.extract.max_new_rules = v;
            }
        });
        let config = self.config.get();
        serde_json::to_string_pretty(&config.extract).unwrap()
    }

    #[tool(description = "Set the delay between epochs in milliseconds. Use 0 for maximum speed.")]
    fn set_epoch_delay(&self, #[tool(aggr)] req: SetEpochDelayRequest) -> String {
        self.config.update(|c| {
            c.engine.epoch_delay_ms = req.delay_ms;
        });
        serde_json::json!({"epoch_delay_ms": req.delay_ms}).to_string()
    }
}

#[tool(tool_box)]
impl ServerHandler for MathscapeMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "Mathscape: evolutionary symbolic compression engine. \
                 Observe and control the engine — view discovered abstractions, \
                 evaluate expressions, identify patterns, and drive the engine \
                 by pausing/resuming, setting epoch ceilings, and tuning parameters live."
                    .into(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    tracing::info!("starting mathscape-mcp server");

    let server = MathscapeMcp::new();
    let transport = rmcp::transport::io::stdio();

    let service = server
        .serve(transport)
        .await
        .expect("failed to start MCP server");
    service.waiting().await.expect("MCP server error");
}
