use axum::{Extension, Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::{get, post}};
use mathscape_api::graphql::{self, EngineProviderDyn, Schema};
use mathscape_api::types::{
    ConfigUpdate, ControlResponse, EpochList, EpochMetrics, EngineConfig as ApiEngineConfig,
    LibraryList, LibrarySymbol, RewardSnapshot, Status,
};
use mathscape_config::DynamicConfig;
use mathscape_core::eval::RewriteRule;
use mathscape_reward::RewardResult;
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::services::ServeDir;

// ---------------------------------------------------------------------------
// Shared engine state
// ---------------------------------------------------------------------------

struct EngineState {
    epoch: u64,
    population: mathscape_evolve::Population,
    latest_reward: Option<RewardResult>,
    library: Vec<RewriteRule>,
    epoch_history: Vec<EpochMetrics>,
}

struct AppState {
    engine: RwLock<EngineState>,
    config: DynamicConfig,
    /// Optional PersistentRegistry — when `MATHSCAPE_PERSISTENT_PATH`
    /// is set at startup, this holds a redb-backed Registry enabling
    /// knowability-criterion-2 across process restarts. The existing
    /// in-memory `engine.library` is unchanged; this field is a
    /// parallel observation surface until the engine_loop migration
    /// to Epoch::step lands. See typescape-binding.md and
    /// persistent_registry.rs in mathscape-store.
    persistent: Option<tokio::sync::Mutex<mathscape_store::PersistentRegistry>>,
}

type SharedState = Arc<AppState>;

// ---------------------------------------------------------------------------
// Builders — shared logic for REST, GraphQL, and gRPC handlers
// ---------------------------------------------------------------------------

async fn build_status(state: &AppState) -> Status {
    let s = state.engine.read().await;
    let config = state.config.get();
    Status {
        epoch: s.epoch,
        running: config.engine.running,
        library_size: s.library.len() as i32,
        population_size: s.population.individuals.len() as i32,
        avg_fitness: s.population.avg_fitness(),
        diversity: s.population.diversity(),
        latest_reward: s.latest_reward.as_ref().map(|r| RewardSnapshot {
            reward: r.reward,
            compression_ratio: r.compression_ratio,
            description_length: r.description_length as i32,
            raw_length: r.raw_length as i32,
            novelty_total: r.novelty_total,
            meta_compression: r.meta_compression,
        }),
    }
}

fn build_config(config: &mathscape_config::Config) -> ApiEngineConfig {
    ApiEngineConfig {
        running: config.engine.running,
        max_epoch: config.engine.max_epoch,
        epoch_delay_ms: config.engine.epoch_delay_ms,
        alpha: config.reward.alpha,
        beta: config.reward.beta,
        gamma: config.reward.gamma,
        population_size: config.population.target_size as u32,
        tournament_k: config.population.tournament_k as u32,
        max_depth: config.population.max_depth as u32,
        elite_fraction: config.population.elite_fraction,
        crossover_rate: config.population.crossover_rate,
        min_shared_size: config.extract.min_shared_size as u32,
        min_matches: config.extract.min_matches as u32,
        max_new_rules: config.extract.max_new_rules as u32,
    }
}

fn apply_config_update(config: &DynamicConfig, update: &ConfigUpdate) -> ControlResponse {
    config.update(|c| {
        if let Some(running) = update.running {
            c.engine.running = running;
        }
        if let Some(max) = update.max_epoch {
            c.engine.max_epoch = Some(max);
        }
        if let Some(delay) = update.epoch_delay_ms {
            c.engine.epoch_delay_ms = delay;
        }
        if let Some(a) = update.alpha {
            c.reward.alpha = a;
        }
        if let Some(b) = update.beta {
            c.reward.beta = b;
        }
        if let Some(g) = update.gamma {
            c.reward.gamma = g;
        }
        if let Some(size) = update.population_size {
            c.population.target_size = size as usize;
        }
        if let Some(k) = update.tournament_k {
            c.population.tournament_k = k as usize;
        }
        if let Some(d) = update.max_depth {
            c.population.max_depth = d as usize;
        }
    });
    ControlResponse {
        success: true,
        message: "config updated".into(),
    }
}

async fn build_epochs(state: &AppState, limit: i32, offset: i32) -> EpochList {
    let s = state.engine.read().await;
    let total = s.epoch_history.len() as i32;
    let epochs: Vec<EpochMetrics> = s
        .epoch_history
        .iter()
        .rev()
        .skip(offset as usize)
        .take(limit as usize)
        .cloned()
        .collect();
    EpochList { epochs, total }
}

async fn build_library(state: &AppState) -> LibraryList {
    let s = state.engine.read().await;
    let symbols = s
        .library
        .iter()
        .enumerate()
        .map(|(i, rule)| LibrarySymbol {
            symbol_id: i as i32,
            name: rule.name.clone(),
            epoch_discovered: 0,
            arity: count_vars(&rule.lhs) as i32,
            lhs_sexpr: format!("{}", rule.lhs),
            rhs_sexpr: format!("{}", rule.rhs),
            generality: None,
            irreducibility: None,
            is_meta: false,
            status: "active".into(),
        })
        .collect();
    LibraryList { symbols }
}

fn count_vars(term: &mathscape_core::Term) -> usize {
    use std::collections::HashSet;
    fn walk(t: &mathscape_core::Term, vars: &mut HashSet<u32>) {
        match t {
            mathscape_core::Term::Var(v) => {
                vars.insert(*v);
            }
            mathscape_core::Term::Apply(f, args) => {
                walk(f, vars);
                for a in args {
                    walk(a, vars);
                }
            }
            mathscape_core::Term::Fn(_, body) => walk(body, vars),
            mathscape_core::Term::Symbol(_, args) => {
                for a in args {
                    walk(a, vars);
                }
            }
            _ => {}
        }
    }
    let mut vars = HashSet::new();
    walk(term, &mut vars);
    vars.len()
}

// ---------------------------------------------------------------------------
// GraphQL provider — bridges AppState to the abstract EngineProviderDyn
// ---------------------------------------------------------------------------

struct GraphQLProvider(SharedState);

#[async_trait::async_trait]
impl EngineProviderDyn for GraphQLProvider {
    async fn status(&self) -> Status {
        build_status(&self.0).await
    }

    async fn epochs(&self, limit: i32, offset: i32) -> EpochList {
        build_epochs(&self.0, limit, offset).await
    }

    async fn library(&self) -> LibraryList {
        build_library(&self.0).await
    }

    fn config(&self) -> ApiEngineConfig {
        build_config(&self.0.config.get())
    }

    fn update_config(&self, update: ConfigUpdate) -> ControlResponse {
        apply_config_update(&self.0.config, &update)
    }

    fn pause(&self) -> ControlResponse {
        self.0.config.pause();
        ControlResponse {
            success: true,
            message: "engine paused".into(),
        }
    }

    fn resume(&self) -> ControlResponse {
        self.0.config.resume();
        ControlResponse {
            success: true,
            message: "engine resumed".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// REST handlers
// ---------------------------------------------------------------------------

async fn healthz() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok"}))
}

async fn readyz(State(state): State<SharedState>) -> Json<serde_json::Value> {
    let s = state.engine.read().await;
    let status = if s.epoch > 0 { "ready" } else { "starting" };
    Json(serde_json::json!({"status": status}))
}

async fn rest_status(State(state): State<SharedState>) -> Json<Status> {
    Json(build_status(&state).await)
}

async fn rest_epochs(
    State(state): State<SharedState>,
    axum::extract::Query(params): axum::extract::Query<PaginationParams>,
) -> Json<EpochList> {
    Json(build_epochs(&state, params.limit.unwrap_or(50), params.offset.unwrap_or(0)).await)
}

async fn rest_library(State(state): State<SharedState>) -> Json<LibraryList> {
    Json(build_library(&state).await)
}

async fn rest_config(State(state): State<SharedState>) -> Json<ApiEngineConfig> {
    Json(build_config(&state.config.get()))
}

async fn rest_update_config(
    State(state): State<SharedState>,
    Json(update): Json<ConfigUpdate>,
) -> Json<ControlResponse> {
    Json(apply_config_update(&state.config, &update))
}

async fn rest_pause(State(state): State<SharedState>) -> Json<ControlResponse> {
    state.config.pause();
    Json(ControlResponse {
        success: true,
        message: "engine paused".into(),
    })
}

async fn rest_resume(State(state): State<SharedState>) -> Json<ControlResponse> {
    state.config.resume();
    Json(ControlResponse {
        success: true,
        message: "engine resumed".into(),
    })
}

/// GET /api/registry-root — return the Merkle root of the
/// PersistentRegistry (when enabled via MATHSCAPE_PERSISTENT_PATH).
///
/// This endpoint demonstrates knowability criterion 2 across
/// process lifetimes: the root is byte-stable across restarts, so
/// clients can observe that shutting down the service + restarting
/// produces the same root value (provided no new artifacts were
/// inserted). Returns `{"root": null}` when persistence is disabled.
async fn rest_registry_root(State(state): State<SharedState>) -> Json<serde_json::Value> {
    use mathscape_core::epoch::Registry;
    match &state.persistent {
        Some(reg_mutex) => {
            let reg = reg_mutex.lock().await;
            Json(serde_json::json!({
                "enabled": true,
                "root": format!("{}", reg.root()),
                "library_size": reg.len(),
            }))
        }
        None => Json(serde_json::json!({
            "enabled": false,
            "root": null,
            "library_size": 0,
        })),
    }
}

#[derive(Deserialize)]
struct PaginationParams {
    limit: Option<i32>,
    offset: Option<i32>,
}

// ---------------------------------------------------------------------------
// GraphQL Axum handlers
// ---------------------------------------------------------------------------

async fn graphql_handler(
    schema: Extension<Schema>,
    req: async_graphql_axum::GraphQLRequest,
) -> async_graphql_axum::GraphQLResponse {
    schema.execute(req.into_inner()).await.into()
}

async fn graphql_playground() -> axum::response::Html<String> {
    axum::response::Html(async_graphql::http::playground_source(
        async_graphql::http::GraphQLPlaygroundConfig::new("/graphql"),
    ))
}

// ---------------------------------------------------------------------------
// SPA fallback — serves index.html for client-side routing (like hanabi)
// ---------------------------------------------------------------------------

async fn spa_fallback(static_dir: String) -> impl IntoResponse {
    let index_path = std::path::Path::new(&static_dir).join("index.html");
    match tokio::fs::read_to_string(&index_path).await {
        Ok(content) => (
            StatusCode::OK,
            [
                (axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8"),
                (
                    axum::http::header::CACHE_CONTROL,
                    "no-cache, no-store, must-revalidate, max-age=0",
                ),
            ],
            content,
        )
            .into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "frontend not available").into_response(),
    }
}

// ---------------------------------------------------------------------------
// gRPC service implementation
// ---------------------------------------------------------------------------

mod grpc {
    use super::*;
    use mathscape_api::proto::mathscape::v1 as pb;
    use mathscape_api::proto::mathscape::v1::engine_service_server::EngineService;
    use tonic::{Request, Response};

    pub struct GrpcEngine {
        pub state: SharedState,
    }

    #[tonic::async_trait]
    impl EngineService for GrpcEngine {
        async fn get_status(
            &self,
            _req: Request<pb::GetStatusRequest>,
        ) -> Result<Response<pb::StatusResponse>, tonic::Status> {
            let status = build_status(&self.state).await;
            Ok(Response::new(status.into()))
        }

        type StreamEpochsStream =
            tokio_stream::wrappers::ReceiverStream<Result<pb::EpochResponse, tonic::Status>>;

        async fn stream_epochs(
            &self,
            _req: Request<pb::StreamEpochsRequest>,
        ) -> Result<Response<Self::StreamEpochsStream>, tonic::Status> {
            let state = self.state.clone();
            let (tx, rx) = tokio::sync::mpsc::channel(32);

            tokio::spawn(async move {
                let mut last_epoch = 0u64;
                loop {
                    let current;
                    let metrics;
                    {
                        let s = state.engine.read().await;
                        current = s.epoch;
                        metrics = s.epoch_history.last().cloned();
                    }
                    if current > last_epoch {
                        if let Some(m) = metrics {
                            let proto: pb::EpochResponse = m.into();
                            if tx.send(Ok(proto)).await.is_err() {
                                break;
                            }
                        }
                        last_epoch = current;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            });

            Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
                rx,
            )))
        }

        async fn pause(
            &self,
            _req: Request<pb::PauseRequest>,
        ) -> Result<Response<pb::ControlResponse>, tonic::Status> {
            self.state.config.pause();
            Ok(Response::new(pb::ControlResponse {
                success: true,
                message: "engine paused".into(),
            }))
        }

        async fn resume(
            &self,
            _req: Request<pb::ResumeRequest>,
        ) -> Result<Response<pb::ControlResponse>, tonic::Status> {
            self.state.config.resume();
            Ok(Response::new(pb::ControlResponse {
                success: true,
                message: "engine resumed".into(),
            }))
        }

        async fn set_max_epoch(
            &self,
            req: Request<pb::SetMaxEpochRequest>,
        ) -> Result<Response<pb::ControlResponse>, tonic::Status> {
            let max = req.into_inner().max_epoch;
            self.state.config.set_max_epoch(max);
            Ok(Response::new(pb::ControlResponse {
                success: true,
                message: format!("max_epoch set to {max:?}"),
            }))
        }

        async fn set_reward_weights(
            &self,
            req: Request<pb::SetRewardWeightsRequest>,
        ) -> Result<Response<pb::ControlResponse>, tonic::Status> {
            let inner = req.into_inner();
            self.state
                .config
                .set_reward_weights(inner.alpha, inner.beta, inner.gamma);
            Ok(Response::new(pb::ControlResponse {
                success: true,
                message: format!(
                    "reward weights: alpha={}, beta={}, gamma={}",
                    inner.alpha, inner.beta, inner.gamma
                ),
            }))
        }

        async fn list_epochs(
            &self,
            req: Request<pb::ListEpochsRequest>,
        ) -> Result<Response<pb::ListEpochsResponse>, tonic::Status> {
            let inner = req.into_inner();
            let list = build_epochs(&self.state, inner.limit, inner.offset).await;
            Ok(Response::new(pb::ListEpochsResponse {
                epochs: list.epochs.into_iter().map(|e| e.into()).collect(),
                total: list.total,
            }))
        }

        async fn list_library(
            &self,
            _req: Request<pb::ListLibraryRequest>,
        ) -> Result<Response<pb::ListLibraryResponse>, tonic::Status> {
            let list = build_library(&self.state).await;
            Ok(Response::new(pb::ListLibraryResponse {
                symbols: list.symbols.into_iter().map(|s| s.into()).collect(),
            }))
        }

        async fn get_config(
            &self,
            _req: Request<pb::GetConfigRequest>,
        ) -> Result<Response<pb::ConfigResponse>, tonic::Status> {
            let config = build_config(&self.state.config.get());
            Ok(Response::new(config.into()))
        }

        async fn update_config(
            &self,
            req: Request<pb::UpdateConfigRequest>,
        ) -> Result<Response<pb::ControlResponse>, tonic::Status> {
            let inner = req.into_inner();
            let update = ConfigUpdate {
                running: inner.running,
                max_epoch: inner.max_epoch,
                epoch_delay_ms: inner.epoch_delay_ms,
                alpha: inner.alpha,
                beta: inner.beta,
                gamma: inner.gamma,
                population_size: inner.population_size,
                tournament_k: inner.tournament_k,
                max_depth: inner.max_depth,
            };
            let result = apply_config_update(&self.state.config, &update);
            Ok(Response::new(result.into()))
        }
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    let config = mathscape_config::load_or_panic();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| config.log_level.clone().into()),
        )
        .init();

    let dyn_config = DynamicConfig::new(config.clone());

    // Optional persistent registry — enabled when
    // MATHSCAPE_PERSISTENT_PATH is set.
    let persistent = std::env::var("MATHSCAPE_PERSISTENT_PATH").ok().and_then(|path| {
        match mathscape_store::PersistentRegistry::open(&path) {
            Ok(reg) => {
                tracing::info!(persistent_path = %path, "PersistentRegistry opened");
                Some(tokio::sync::Mutex::new(reg))
            }
            Err(e) => {
                tracing::warn!(persistent_path = %path, error = %e, "PersistentRegistry open failed; continuing without persistence");
                None
            }
        }
    });

    let state: SharedState = Arc::new(AppState {
        engine: RwLock::new(EngineState {
            epoch: 0,
            population: config.to_population(),
            latest_reward: None,
            library: Vec::new(),
            epoch_history: Vec::new(),
        }),
        config: dyn_config,
        persistent,
    });

    // Initialize population
    {
        use rand::SeedableRng;
        let mut s = state.engine.write().await;
        let mut rng = rand::rngs::StdRng::from_entropy();
        s.population.initialize(&mut rng);
        tracing::info!(pop_size = config.population.target_size, "population initialized");
    }

    // Spawn engine loop
    let engine_state = state.clone();
    tokio::spawn(async move {
        engine_loop(engine_state).await;
    });

    // Build GraphQL schema
    let gql_schema = graphql::build_schema(Box::new(GraphQLProvider(state.clone())));

    // Static file serving — STATIC_DIR env var or /app/static (container default)
    let static_dir = std::env::var("STATIC_DIR").unwrap_or_else(|_| "/app/static".into());

    // REST + GraphQL router (schema passed via Extension)
    let api_routes = Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/api/status", get(rest_status))
        .route("/api/epochs", get(rest_epochs))
        .route("/api/library", get(rest_library))
        .route("/api/config", get(rest_config).put(rest_update_config))
        .route("/api/engine/pause", post(rest_pause))
        .route("/api/engine/resume", post(rest_resume))
        .route("/api/registry-root", get(rest_registry_root))
        .route("/graphql", get(graphql_playground).post(graphql_handler))
        .with_state(state.clone())
        .layer(Extension(gql_schema))
        .layer(axum::extract::DefaultBodyLimit::max(1024 * 1024));

    // SPA fallback: serve static files, fall back to index.html for client-side routing
    let spa_dir = static_dir.clone();
    let app = api_routes.fallback_service(
        ServeDir::new(&static_dir).not_found_service(tower::service_fn(move |_req| {
            let dir = spa_dir.clone();
            async move { Ok::<_, std::convert::Infallible>(spa_fallback(dir).await.into_response()) }
        })),
    );

    // gRPC service
    let grpc_engine = grpc::GrpcEngine {
        state: state.clone(),
    };

    let http_addr = format!("{}:{}", config.http.host, config.http.port);
    let grpc_port = config.http.port + 1;
    let grpc_addr = format!("{}:{}", config.http.host, grpc_port);

    tracing::info!(http = %http_addr, grpc = %grpc_addr, "starting servers");

    let http_listener = tokio::net::TcpListener::bind(&http_addr).await.unwrap();

    let grpc_handle = tokio::spawn(async move {
        use mathscape_api::proto::mathscape::v1::engine_service_server::EngineServiceServer;
        tonic::transport::Server::builder()
            .add_service(EngineServiceServer::new(grpc_engine))
            .serve(grpc_addr.parse().unwrap())
            .await
            .unwrap();
    });

    let http_handle = tokio::spawn(async move {
        axum::serve(http_listener, app.into_make_service())
            .await
            .unwrap();
    });

    tokio::select! {
        _ = http_handle => tracing::error!("HTTP server exited"),
        _ = grpc_handle => tracing::error!("gRPC server exited"),
    }
}

// ---------------------------------------------------------------------------
// Engine loop
// ---------------------------------------------------------------------------

async fn engine_loop(state: SharedState) {
    use rand::SeedableRng;
    let mut rng = rand::rngs::StdRng::from_entropy();
    let mut next_symbol_id = 1u32;

    loop {
        let epoch;
        {
            let s = state.engine.read().await;
            epoch = s.epoch;
        }

        if !state.config.should_run(epoch) {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            continue;
        }

        let config = state.config.get();
        let extract_cfg = config.to_extract_config();
        let reward_config = config.to_reward_config();

        let corpus: Vec<mathscape_core::Term>;
        let library_snapshot: Vec<RewriteRule>;
        {
            let s = state.engine.read().await;
            corpus = s.population.individuals.iter().map(|i| i.term.clone()).collect();
            library_snapshot = s.library.clone();
        }

        let start = std::time::Instant::now();

        let new_rules = mathscape_compress::extract::extract_rules(
            &corpus,
            &library_snapshot,
            &mut next_symbol_id,
            &extract_cfg,
        );

        let mut full_library = library_snapshot;
        full_library.extend(new_rules.iter().cloned());

        let reward_result =
            mathscape_reward::compute_reward(&corpus, &full_library, &new_rules, &reward_config);

        let duration_ms = start.elapsed().as_millis() as i32;
        let lib_len = full_library.len();

        let epoch_metrics = EpochMetrics {
            epoch: (epoch + 1) as i32,
            compression_ratio: reward_result.compression_ratio,
            description_length: reward_result.description_length as i32,
            novelty_total: reward_result.novelty_total,
            meta_compression: reward_result.meta_compression,
            library_size: lib_len as i32,
            population_diversity: None,
            phase: None,
            duration_ms: Some(duration_ms),
            alpha: config.reward.alpha,
            beta: config.reward.beta,
            gamma: config.reward.gamma,
        };

        {
            let mut s = state.engine.write().await;
            s.library = full_library;

            for ind in &mut s.population.individuals {
                ind.fitness =
                    reward_result.compression_ratio + reward_result.novelty_total * 0.1;
            }
            s.population.update_archive();
            s.population.inject_elites(config.population.elite_fraction);
            s.population.evolve(&mut rng);

            let diversity = s.population.diversity();
            let mut metrics = epoch_metrics;
            metrics.population_diversity = Some(diversity);

            s.epoch += 1;
            s.latest_reward = Some(reward_result.clone());
            s.epoch_history.push(metrics);
        }

        tracing::info!(
            epoch = epoch + 1,
            cr = %format!("{:.4}", reward_result.compression_ratio),
            dl = reward_result.description_length,
            novelty = %format!("{:.4}", reward_result.novelty_total),
            library = lib_len,
            duration_ms,
            "epoch complete"
        );

        if config.engine.epoch_delay_ms > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(config.engine.epoch_delay_ms))
                .await;
        } else {
            tokio::task::yield_now().await;
        }
    }
}
