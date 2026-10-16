//! Shared configuration with layered precedence:
//!
//!   defaults → YAML → env vars → dynamic (runtime)
//!
//! Precedence (highest wins):
//!   1. Dynamic overrides (set at runtime via MCP or API — stored in-memory)
//!   2. Environment variables (`MATHSCAPE_` prefix, `__` for nesting)
//!   3. YAML config file (`mathscape.yaml` or `MATHSCAPE_CONFIG` path)
//!   4. Compiled defaults
//!
//! Dynamic config is the most respected layer: MCP tools can set any field
//! at runtime without restarting the engine. This enables "driving" the
//! engine — pausing epochs, adjusting reward weights, changing population
//! parameters — all live.
//!
//! Example env vars:
//!   MATHSCAPE_HTTP__PORT=9090
//!   MATHSCAPE_POPULATION__TARGET_SIZE=5000
//!   MATHSCAPE_DATABASE__URL=postgres://...

// Config loading goes through shikumi's ProviderChain — the pleme-io
// standard. shikumi wraps figment behind a fluent API and owns the
// figment dep on this crate's behalf.
pub mod chart;

use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};

/// Top-level configuration for all Mathscape binaries.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    /// HTTP server settings.
    pub http: HttpConfig,
    /// Evolution engine settings.
    pub population: PopulationConfig,
    /// Reward function weights.
    pub reward: RewardConfig,
    /// Library extraction settings.
    pub extract: ExtractConfig,
    /// Engine control (runtime-adjustable).
    pub engine: EngineConfig,
    /// Database settings.
    pub database: DatabaseConfig,
    /// Expression store settings.
    pub store: StoreConfig,
    /// Logging level.
    pub log_level: String,
    #[serde(default)]
    pub stream: Option<StreamConfig>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum StreamClock {
    #[default]
    Logical,
    Wall,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StreamConfig {
    pub seed: u64,
    #[serde(default = "default_stream_data_dir")]
    pub data_dir: String,
    #[serde(default = "default_snapshot_every")]
    pub snapshot_every: u64,
    #[serde(default = "default_checkpoint_epochs")]
    pub checkpoint_epochs: u64,
    #[serde(default)]
    pub until_epoch: Option<u64>,
    #[serde(default = "default_phases_per_epoch")]
    pub phases_per_epoch: usize,
    #[serde(default = "default_iterations")]
    pub iterations: usize,
    #[serde(default = "default_adaptive_every")]
    pub adaptive_every: u64,
    #[serde(default = "default_learning_rate")]
    pub learning_rate: f64,
    #[serde(default = "default_extractor")]
    pub extractor: mathscape_compress::Extractor,
    #[serde(default = "default_corpus_max_nodes")]
    pub corpus_max_nodes: usize,
    #[serde(default)]
    pub clock: StreamClock,
    #[serde(default)]
    pub novelty: StreamNovelty,
    #[serde(default)]
    pub frontier: StreamFrontier,
    #[serde(default)]
    pub mechanism: StreamMechanism,
    #[serde(default)]
    pub health_addr: Option<String>,
    #[serde(default)]
    pub substrate: StreamSubstrate,
    #[serde(default)]
    pub emergence: mathscape_emergence::EmergenceConfig,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum StreamSubstrate {
    #[default]
    Emergent,
    Builtin,
}

impl StreamSubstrate {
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Emergent => "emergent",
            Self::Builtin => "builtin",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum StreamNovelty {
    #[default]
    Entailment,
    Off,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct StreamFrontier {
    pub enabled: bool,
    pub per_epoch: usize,
    pub variables: u32,
}

impl Default for StreamFrontier {
    fn default() -> Self {
        Self {
            enabled: false,
            per_epoch: 64,
            variables: 3,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum StreamGraveyard {
    #[default]
    Context,
    Mutation,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum StreamTrialScale {
    #[default]
    Opened,
    Fixed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct StreamMechanism {
    pub respond: bool,
    pub plateau_epochs: u64,
    pub mutations_per_round: usize,
    pub escalation_budget: usize,
    pub trial_budget: usize,
    pub graveyard: StreamGraveyard,
    pub trial_scale: StreamTrialScale,
    pub trial_ceiling: usize,
    pub trial_stop_after: usize,
    pub drive_motor: bool,
}

impl Default for StreamMechanism {
    fn default() -> Self {
        Self {
            respond: true,
            plateau_epochs: 4,
            mutations_per_round: 6,
            escalation_budget: 2,
            trial_budget: 256,
            graveyard: StreamGraveyard::Context,
            trial_scale: StreamTrialScale::Opened,
            trial_ceiling: 32_768,
            trial_stop_after: 1,
            drive_motor: true,
        }
    }
}

impl StreamMechanism {
    #[must_use]
    pub fn v0_1_8() -> Self {
        Self {
            graveyard: StreamGraveyard::Mutation,
            trial_scale: StreamTrialScale::Fixed,
            trial_stop_after: 0,
            drive_motor: false,
            ..Self::default()
        }
    }
}

impl StreamConfig {
    #[must_use]
    pub fn seeded(seed: u64, data_dir: impl Into<String>) -> Self {
        Self {
            seed,
            data_dir: data_dir.into(),
            snapshot_every: default_snapshot_every(),
            checkpoint_epochs: default_checkpoint_epochs(),
            until_epoch: None,
            phases_per_epoch: default_phases_per_epoch(),
            iterations: default_iterations(),
            adaptive_every: default_adaptive_every(),
            learning_rate: default_learning_rate(),
            extractor: default_extractor(),
            corpus_max_nodes: default_corpus_max_nodes(),
            clock: StreamClock::default(),
            novelty: StreamNovelty::default(),
            frontier: StreamFrontier::default(),
            mechanism: StreamMechanism::default(),
            health_addr: None,
            substrate: StreamSubstrate::default(),
            emergence: mathscape_emergence::EmergenceConfig::default(),
        }
    }

    #[must_use]
    pub fn builtin(seed: u64, data_dir: impl Into<String>) -> Self {
        Self {
            substrate: StreamSubstrate::Builtin,
            frontier: StreamFrontier {
                enabled: true,
                ..StreamFrontier::default()
            },
            ..Self::seeded(seed, data_dir)
        }
    }

    #[must_use]
    pub fn emergent(&self) -> bool {
        self.substrate == StreamSubstrate::Emergent
    }
}

fn default_corpus_max_nodes() -> usize {
    mathscape_compress::DEFAULT_CORPUS_MAX_NODES
}

fn default_stream_data_dir() -> String {
    "./data/stream".into()
}

fn default_snapshot_every() -> u64 {
    4
}

fn default_checkpoint_epochs() -> u64 {
    64
}

fn default_phases_per_epoch() -> usize {
    2
}

fn default_iterations() -> usize {
    5
}

fn default_adaptive_every() -> u64 {
    1
}

fn default_learning_rate() -> f64 {
    0.1
}

fn default_extractor() -> mathscape_compress::Extractor {
    mathscape_compress::Extractor::derived_laws()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HttpConfig {
    pub port: u16,
    pub host: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PopulationConfig {
    pub target_size: usize,
    pub tournament_k: usize,
    pub max_depth: usize,
    pub elite_fraction: f64,
    pub crossover_rate: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RewardConfig {
    pub alpha: f64,
    pub beta: f64,
    pub gamma: f64,
    /// Library-LHS subsumption weight. A candidate rule that would
    /// make K existing library rules redundant earns `K * delta` bits
    /// — this is the signal that drives dimensional discovery
    /// (meta-rules) past ΔCR gatekeeping.
    #[serde(default = "default_delta")]
    pub delta: f64,
}

fn default_delta() -> f64 {
    0.5
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExtractConfig {
    pub min_shared_size: usize,
    pub min_matches: usize,
    pub max_new_rules: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EngineConfig {
    /// Whether the engine is running (false = paused).
    pub running: bool,
    /// Maximum epoch to run to (None = unlimited).
    pub max_epoch: Option<u64>,
    /// Delay between epochs in milliseconds (0 = no delay).
    pub epoch_delay_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoreConfig {
    pub path: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            http: HttpConfig {
                port: 8080,
                host: "0.0.0.0".into(),
            },
            population: PopulationConfig {
                target_size: 10_000,
                tournament_k: 5,
                max_depth: 10,
                elite_fraction: 0.1,
                crossover_rate: 0.3,
            },
            reward: RewardConfig {
                alpha: 0.6,
                beta: 0.3,
                gamma: 0.1,
                delta: 0.5,
            },
            extract: ExtractConfig {
                min_shared_size: 3,
                min_matches: 2,
                max_new_rules: 5,
            },
            engine: EngineConfig {
                running: true,
                max_epoch: None,
                epoch_delay_ms: 0,
            },
            database: DatabaseConfig {
                url: "postgres://localhost/mathscape".into(),
            },
            store: StoreConfig {
                path: "./data/expressions.redb".into(),
            },
            log_level: "info,mathscape=debug".into(),
            stream: None,
        }
    }
}

impl Config {
    /// Convert to the reward crate's RewardConfig.
    pub fn to_reward_config(&self) -> mathscape_reward::RewardConfig {
        mathscape_reward::RewardConfig {
            alpha: self.reward.alpha,
            beta: self.reward.beta,
            gamma: self.reward.gamma,
            delta: self.reward.delta,
        }
    }

    /// Convert to the compress crate's ExtractConfig.
    pub fn to_extract_config(&self) -> mathscape_compress::extract::ExtractConfig {
        mathscape_compress::extract::ExtractConfig {
            min_shared_size: self.extract.min_shared_size,
            min_matches: self.extract.min_matches,
            max_new_rules: self.extract.max_new_rules,
        }
    }

    /// Create a Population from the population config.
    pub fn to_population(&self) -> mathscape_evolve::Population {
        let mut pop = mathscape_evolve::Population::new(self.population.target_size);
        pop.tournament_k = self.population.tournament_k;
        pop.max_depth = self.population.max_depth;
        pop
    }
}

/// Load configuration with layered precedence:
///   defaults → YAML file → environment variables
///
/// # Errors
///
/// Returns a shikumi error if the YAML file exists but is malformed
/// or env var coercion fails.
pub fn load() -> Result<Config, shikumi::ShikumiError> {
    let config_path =
        std::env::var("MATHSCAPE_CONFIG").unwrap_or_else(|_| "mathscape.yaml".into());
    load_from(&config_path)
}

/// Load configuration from a specific YAML path.
///
/// # Errors
///
/// Returns a shikumi error if the YAML file exists but is malformed
/// or env var coercion fails. A missing YAML file is tolerated —
/// defaults are returned.
pub fn load_from(yaml_path: &str) -> Result<Config, shikumi::ShikumiError> {
    shikumi::ProviderChain::new()
        .with_defaults(&Config::default())
        .with_file(std::path::Path::new(yaml_path))
        .with_env("MATHSCAPE_")
        .extract()
}

/// Load configuration, panicking on error (for binary startup).
pub fn load_or_panic() -> Config {
    load().unwrap_or_else(|e| {
        eprintln!("configuration error: {e}");
        std::process::exit(1);
    })
}

/// Thread-safe dynamic configuration store.
///
/// Wraps a Config in an Arc<RwLock> so MCP tools and API endpoints
/// can read/write config at runtime. Dynamic changes are the highest
/// precedence layer — they override everything.
#[derive(Clone)]
pub struct DynamicConfig {
    inner: Arc<RwLock<Config>>,
}

impl DynamicConfig {
    /// Create a new dynamic config from a loaded static config.
    pub fn new(config: Config) -> Self {
        DynamicConfig {
            inner: Arc::new(RwLock::new(config)),
        }
    }

    /// Get a snapshot of the current config.
    pub fn get(&self) -> Config {
        self.inner.read().unwrap().clone()
    }

    /// Replace the entire config.
    pub fn set(&self, config: Config) {
        *self.inner.write().unwrap() = config;
    }

    /// Mutate the config in-place with a closure.
    pub fn update(&self, f: impl FnOnce(&mut Config)) {
        let mut config = self.inner.write().unwrap();
        f(&mut config);
    }

    /// Pause the engine.
    pub fn pause(&self) {
        self.update(|c| c.engine.running = false);
    }

    /// Resume the engine.
    pub fn resume(&self) {
        self.update(|c| c.engine.running = true);
    }

    /// Set a max epoch ceiling. Engine stops after reaching this epoch.
    pub fn set_max_epoch(&self, max: Option<u64>) {
        self.update(|c| c.engine.max_epoch = max);
    }

    /// Update reward weights.
    pub fn set_reward_weights(&self, alpha: f64, beta: f64, gamma: f64) {
        self.update(|c| {
            c.reward.alpha = alpha;
            c.reward.beta = beta;
            c.reward.gamma = gamma;
        });
    }

    /// Check if the engine should run the next epoch.
    pub fn should_run(&self, current_epoch: u64) -> bool {
        let c = self.inner.read().unwrap();
        if !c.engine.running {
            return false;
        }
        if let Some(max) = c.engine.max_epoch {
            if current_epoch >= max {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_loads() {
        let config = Config::default();
        assert_eq!(config.http.port, 8080);
        assert_eq!(config.population.target_size, 10_000);
        assert!((config.reward.alpha - 0.6).abs() < f64::EPSILON);
    }

    #[test]
    fn defaults_roundtrip_through_provider_chain() {
        // Confirms shikumi's ProviderChain serializes + extracts defaults
        // equivalently to a raw figment round-trip. No YAML file, no env
        // vars — just the defaults layer.
        let config: Config = shikumi::ProviderChain::new()
            .with_defaults(&Config::default())
            .extract()
            .unwrap();
        assert_eq!(config.http.port, 8080);
        assert_eq!(config.database.url, "postgres://localhost/mathscape");
    }

    fn yaml(name: &str, body: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("mathscape-config-{name}-{}.yaml", std::process::id()));
        std::fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn a_stream_section_loads_with_its_defaults_and_absent_means_none() {
        assert!(Config::default().stream.is_none());
        let p = yaml("stream-ok", "stream:\n  seed: 42\n  data_dir: /data/stream\n");
        let c = load_from(p.to_str().unwrap()).unwrap();
        let s = c.stream.unwrap();
        assert_eq!(s.seed, 42);
        assert_eq!(s.data_dir, "/data/stream");
        assert_eq!(s.snapshot_every, 4);
        assert_eq!(s.clock, StreamClock::Logical);
        assert_eq!(s.health_addr, None);
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn a_stream_section_without_a_seed_or_with_an_unknown_key_is_refused() {
        let p = yaml("stream-noseed", "stream:\n  data_dir: /data/stream\n");
        assert!(load_from(p.to_str().unwrap()).is_err());
        let q = yaml("stream-typo", "stream:\n  seed: 1\n  snapshot_evry: 3\n");
        assert!(load_from(q.to_str().unwrap()).is_err());
        let _ = std::fs::remove_file(p);
        let _ = std::fs::remove_file(q);
    }

    #[test]
    fn the_stream_section_is_reachable_from_the_environment() {
        unsafe {
            std::env::set_var("MSCFGSTREAMTEST_STREAM__SEED", "9");
            std::env::set_var("MSCFGSTREAMTEST_STREAM__DATA_DIR", "/data/stream");
            std::env::set_var("MSCFGSTREAMTEST_STREAM__HEALTH_ADDR", "127.0.0.1:9473");
        }
        let c: Config = shikumi::ProviderChain::new()
            .with_defaults(&Config::default())
            .with_env("MSCFGSTREAMTEST_")
            .extract()
            .unwrap();
        unsafe {
            std::env::remove_var("MSCFGSTREAMTEST_STREAM__SEED");
            std::env::remove_var("MSCFGSTREAMTEST_STREAM__DATA_DIR");
            std::env::remove_var("MSCFGSTREAMTEST_STREAM__HEALTH_ADDR");
        }
        let s = c.stream.unwrap();
        assert_eq!((s.seed, s.data_dir.as_str()), (9, "/data/stream"));
        assert_eq!(s.health_addr.as_deref(), Some("127.0.0.1:9473"));
    }

    #[test]
    fn nested_stream_keys_flattened_as_the_chart_does_reach_the_typed_config() {
        let vars = [
            ("MSCFGNESTED_STREAM__SEED", "9"),
            ("MSCFGNESTED_STREAM__MECHANISM__GRAVEYARD", "mutation"),
            ("MSCFGNESTED_STREAM__MECHANISM__DRIVE_MOTOR", "false"),
            ("MSCFGNESTED_STREAM__EXTRACTOR__KIND", "validated"),
            ("MSCFGNESTED_STREAM__EXTRACTOR__STEP_LIMIT", "300"),
            ("MSCFGNESTED_STREAM__EXTRACTOR__MIN_SUPPORT", "2"),
            ("MSCFGNESTED_STREAM__EXTRACTOR__SUBTERM_DEPTH", "3"),
            ("MSCFGNESTED_STREAM__EXTRACTOR__K_SAMPLES", "8"),
            ("MSCFGNESTED_STREAM__EXTRACTOR__SEED", "7"),
            ("MSCFGNESTED_STREAM__EMERGENCE__BASIS", "combinators"),
            ("MSCFGNESTED_STREAM__LEARNING_RATE", "0.25"),
        ];
        unsafe {
            for (k, v) in vars {
                std::env::set_var(k, v);
            }
        }
        let c: Config = shikumi::ProviderChain::new()
            .with_defaults(&Config::default())
            .with_env("MSCFGNESTED_")
            .extract()
            .unwrap();
        unsafe {
            for (k, _) in vars {
                std::env::remove_var(k);
            }
        }
        let s = c.stream.unwrap();
        assert_eq!(s.mechanism.graveyard, StreamGraveyard::Mutation);
        assert!(!s.mechanism.drive_motor);
        assert_eq!(
            s.extractor,
            mathscape_compress::Extractor::Validated {
                step_limit: 300,
                min_support: 2,
                subterm_depth: 3,
                k_samples: 8,
                seed: 7,
            }
        );
        assert_eq!(s.emergence.basis, mathscape_emergence::Basis::Combinators);
        assert!((s.learning_rate - 0.25).abs() < f64::EPSILON);
    }

    #[test]
    fn reward_weights_sum_to_one() {
        let config = Config::default();
        let sum = config.reward.alpha + config.reward.beta + config.reward.gamma;
        assert!((sum - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn dynamic_config_pause_resume() {
        let dc = DynamicConfig::new(Config::default());
        assert!(dc.should_run(0));

        dc.pause();
        assert!(!dc.should_run(0));

        dc.resume();
        assert!(dc.should_run(0));
    }

    #[test]
    fn dynamic_config_max_epoch() {
        let dc = DynamicConfig::new(Config::default());
        dc.set_max_epoch(Some(100));

        assert!(dc.should_run(50));
        assert!(dc.should_run(99));
        assert!(!dc.should_run(100));
        assert!(!dc.should_run(200));

        dc.set_max_epoch(None);
        assert!(dc.should_run(1000));
    }

    #[test]
    fn dynamic_config_update_weights() {
        let dc = DynamicConfig::new(Config::default());
        dc.set_reward_weights(0.5, 0.4, 0.1);

        let c = dc.get();
        assert!((c.reward.alpha - 0.5).abs() < f64::EPSILON);
        assert!((c.reward.beta - 0.4).abs() < f64::EPSILON);
    }

    #[test]
    fn engine_default_is_running() {
        let config = Config::default();
        assert!(config.engine.running);
        assert!(config.engine.max_epoch.is_none());
        assert_eq!(config.engine.epoch_delay_ms, 0);
    }
}

#[cfg(test)]
mod substrate {
    use super::*;

    #[test]
    fn the_default_stream_grows_mathematics_from_the_primitives_and_keeps_builtins_and_the_frontier_off() {
        let seeded = StreamConfig::seeded(7, "unused");
        let from_yaml: StreamConfig = serde_json::from_str(r#"{"seed": 7}"#).unwrap();
        for cfg in [seeded, from_yaml] {
            assert_eq!(cfg.substrate, StreamSubstrate::Emergent);
            assert!(!cfg.frontier.enabled);
            assert_eq!(cfg.emergence, mathscape_emergence::EmergenceConfig::default());
        }
    }

    #[test]
    fn the_builtin_profile_is_kept_as_a_selectable_value() {
        let cfg = StreamConfig::builtin(7, "unused");
        assert_eq!(cfg.substrate, StreamSubstrate::Builtin);
        assert!(cfg.frontier.enabled);
        assert_eq!(cfg.mechanism, StreamMechanism::default());
    }
}
