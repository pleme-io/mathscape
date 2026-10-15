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
