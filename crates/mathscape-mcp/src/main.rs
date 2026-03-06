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

use mathscape_config::DynamicConfig;
use mathscape_core::eval::RewriteRule;
use mathscape_discovery::catalog;
use mathscape_discovery::matcher;
use mathscape_discovery::representation;
use mathscape_discovery::scanner::{self, SymbolRecord};
use rmcp::model::{ServerCapabilities, ServerInfo};
use rmcp::{ServerHandler, ServiceExt, schemars, tool};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};

/// Shared engine state accessible from MCP tool handlers.
#[derive(Clone, Default)]
struct EngineSnapshot {
    epoch: u64,
    library: Vec<RewriteRule>,
    population_size: usize,
    avg_fitness: f64,
    diversity: f64,
}

/// The MCP server handler.
#[derive(Clone)]
struct MathscapeMcp {
    state: Arc<RwLock<EngineSnapshot>>,
    config: DynamicConfig,
}

impl MathscapeMcp {
    fn new() -> Self {
        let config = mathscape_config::load_or_panic();
        MathscapeMcp {
            state: Arc::new(RwLock::new(EngineSnapshot::default())),
            config: DynamicConfig::new(config),
        }
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
            "reward_weights": {
                "alpha": config.reward.alpha,
                "beta": config.reward.beta,
                "gamma": config.reward.gamma,
            },
        }))
        .unwrap()
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
