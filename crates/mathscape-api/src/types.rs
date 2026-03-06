//! Central API types — shared across REST, GraphQL, and gRPC.
//!
//! Each type derives `Serialize`/`Deserialize` (REST JSON) and implements
//! `async_graphql::SimpleObject` (GraphQL). Proto conversions are `From` impls.

use async_graphql::SimpleObject;
use serde::{Deserialize, Serialize};

use crate::proto::mathscape::v1 as pb;

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize, SimpleObject)]
pub struct Status {
    pub epoch: u64,
    pub running: bool,
    pub library_size: i32,
    pub population_size: i32,
    pub avg_fitness: f64,
    pub diversity: f64,
    pub latest_reward: Option<RewardSnapshot>,
}

#[derive(Clone, Debug, Serialize, Deserialize, SimpleObject)]
pub struct RewardSnapshot {
    pub reward: f64,
    pub compression_ratio: f64,
    pub description_length: i32,
    pub raw_length: i32,
    pub novelty_total: f64,
    pub meta_compression: f64,
}

impl From<Status> for pb::StatusResponse {
    fn from(s: Status) -> Self {
        pb::StatusResponse {
            epoch: s.epoch,
            running: s.running,
            library_size: s.library_size,
            population_size: s.population_size,
            avg_fitness: s.avg_fitness,
            diversity: s.diversity,
            latest_reward: s.latest_reward.map(|r| r.into()),
        }
    }
}

impl From<RewardSnapshot> for pb::RewardSnapshot {
    fn from(r: RewardSnapshot) -> Self {
        pb::RewardSnapshot {
            reward: r.reward,
            compression_ratio: r.compression_ratio,
            description_length: r.description_length,
            raw_length: r.raw_length,
            novelty_total: r.novelty_total,
            meta_compression: r.meta_compression,
        }
    }
}

// ---------------------------------------------------------------------------
// Epoch metrics
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize, SimpleObject)]
pub struct EpochMetrics {
    pub epoch: i32,
    pub compression_ratio: f64,
    pub description_length: i32,
    pub novelty_total: f64,
    pub meta_compression: f64,
    pub library_size: i32,
    pub population_diversity: Option<f64>,
    pub phase: Option<String>,
    pub duration_ms: Option<i32>,
    pub alpha: f64,
    pub beta: f64,
    pub gamma: f64,
}

impl From<mathscape_entity::epoch::Model> for EpochMetrics {
    fn from(e: mathscape_entity::epoch::Model) -> Self {
        EpochMetrics {
            epoch: e.epoch,
            compression_ratio: e.compression_ratio,
            description_length: e.description_length,
            novelty_total: e.novelty_total,
            meta_compression: e.meta_compression,
            library_size: e.library_size,
            population_diversity: e.population_diversity,
            phase: e.phase,
            duration_ms: e.duration_ms,
            alpha: e.alpha,
            beta: e.beta,
            gamma: e.gamma,
        }
    }
}

impl From<EpochMetrics> for pb::EpochResponse {
    fn from(e: EpochMetrics) -> Self {
        pb::EpochResponse {
            epoch: e.epoch,
            compression_ratio: e.compression_ratio,
            description_length: e.description_length,
            novelty_total: e.novelty_total,
            meta_compression: e.meta_compression,
            library_size: e.library_size,
            population_diversity: e.population_diversity,
            phase: e.phase,
            duration_ms: e.duration_ms,
            alpha: e.alpha,
            beta: e.beta,
            gamma: e.gamma,
        }
    }
}

// ---------------------------------------------------------------------------
// Library symbol
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize, SimpleObject)]
pub struct LibrarySymbol {
    pub symbol_id: i32,
    pub name: String,
    pub epoch_discovered: i32,
    pub arity: i32,
    pub lhs_sexpr: String,
    pub rhs_sexpr: String,
    pub generality: Option<f64>,
    pub irreducibility: Option<f64>,
    pub is_meta: bool,
    pub status: String,
}

impl From<LibrarySymbol> for pb::LibrarySymbol {
    fn from(s: LibrarySymbol) -> Self {
        pb::LibrarySymbol {
            symbol_id: s.symbol_id,
            name: s.name,
            epoch_discovered: s.epoch_discovered,
            arity: s.arity,
            lhs_sexpr: s.lhs_sexpr,
            rhs_sexpr: s.rhs_sexpr,
            generality: s.generality,
            irreducibility: s.irreducibility,
            is_meta: s.is_meta,
            status: s.status,
        }
    }
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize, SimpleObject)]
pub struct EngineConfig {
    pub running: bool,
    pub max_epoch: Option<u64>,
    pub epoch_delay_ms: u64,
    pub alpha: f64,
    pub beta: f64,
    pub gamma: f64,
    pub population_size: u32,
    pub tournament_k: u32,
    pub max_depth: u32,
    pub elite_fraction: f64,
    pub crossover_rate: f64,
    pub min_shared_size: u32,
    pub min_matches: u32,
    pub max_new_rules: u32,
}

impl From<EngineConfig> for pb::ConfigResponse {
    fn from(c: EngineConfig) -> Self {
        pb::ConfigResponse {
            running: c.running,
            max_epoch: c.max_epoch,
            epoch_delay_ms: c.epoch_delay_ms,
            alpha: c.alpha,
            beta: c.beta,
            gamma: c.gamma,
            population_size: c.population_size,
            tournament_k: c.tournament_k,
            max_depth: c.max_depth,
            elite_fraction: c.elite_fraction,
            crossover_rate: c.crossover_rate,
            min_shared_size: c.min_shared_size,
            min_matches: c.min_matches,
            max_new_rules: c.max_new_rules,
        }
    }
}

// ---------------------------------------------------------------------------
// Control responses
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize, SimpleObject)]
pub struct ControlResponse {
    pub success: bool,
    pub message: String,
}

impl From<ControlResponse> for pb::ControlResponse {
    fn from(c: ControlResponse) -> Self {
        pb::ControlResponse {
            success: c.success,
            message: c.message,
        }
    }
}

// ---------------------------------------------------------------------------
// Config update input
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize, async_graphql::InputObject)]
pub struct ConfigUpdate {
    pub running: Option<bool>,
    pub max_epoch: Option<u64>,
    pub epoch_delay_ms: Option<u64>,
    pub alpha: Option<f64>,
    pub beta: Option<f64>,
    pub gamma: Option<f64>,
    pub population_size: Option<u32>,
    pub tournament_k: Option<u32>,
    pub max_depth: Option<u32>,
}

// ---------------------------------------------------------------------------
// Paginated response wrapper
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize, SimpleObject)]
pub struct EpochList {
    pub epochs: Vec<EpochMetrics>,
    pub total: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize, SimpleObject)]
pub struct LibraryList {
    pub symbols: Vec<LibrarySymbol>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn sample_reward_snapshot() -> RewardSnapshot {
        RewardSnapshot {
            reward: 1.5,
            compression_ratio: 0.42,
            description_length: 100,
            raw_length: 238,
            novelty_total: 0.87,
            meta_compression: 0.33,
        }
    }

    fn sample_status() -> Status {
        Status {
            epoch: 42,
            running: true,
            library_size: 15,
            population_size: 256,
            avg_fitness: 0.78,
            diversity: 0.65,
            latest_reward: Some(sample_reward_snapshot()),
        }
    }

    fn sample_epoch_metrics() -> EpochMetrics {
        EpochMetrics {
            epoch: 7,
            compression_ratio: 0.55,
            description_length: 120,
            novelty_total: 0.91,
            meta_compression: 0.44,
            library_size: 10,
            population_diversity: Some(0.73),
            phase: Some("explore".into()),
            duration_ms: Some(340),
            alpha: 1.0,
            beta: 0.5,
            gamma: 0.25,
        }
    }

    fn sample_library_symbol() -> LibrarySymbol {
        LibrarySymbol {
            symbol_id: 3,
            name: "add".into(),
            epoch_discovered: 2,
            arity: 2,
            lhs_sexpr: "(+ ?a ?b)".into(),
            rhs_sexpr: "(add ?a ?b)".into(),
            generality: Some(0.9),
            irreducibility: Some(0.8),
            is_meta: false,
            status: "active".into(),
        }
    }

    fn sample_engine_config() -> EngineConfig {
        EngineConfig {
            running: true,
            max_epoch: Some(1000),
            epoch_delay_ms: 50,
            alpha: 1.0,
            beta: 0.5,
            gamma: 0.25,
            population_size: 128,
            tournament_k: 5,
            max_depth: 12,
            elite_fraction: 0.1,
            crossover_rate: 0.7,
            min_shared_size: 3,
            min_matches: 2,
            max_new_rules: 10,
        }
    }

    fn sample_control_response() -> ControlResponse {
        ControlResponse {
            success: true,
            message: "Engine paused".into(),
        }
    }

    // -----------------------------------------------------------------------
    // 1. JSON round-trip tests
    // -----------------------------------------------------------------------

    #[test]
    fn status_json_round_trip() {
        let original = sample_status();
        let json = serde_json::to_string(&original).unwrap();
        let restored: Status = serde_json::from_str(&json).unwrap();
        assert_eq!(format!("{:?}", original), format!("{:?}", restored));
    }

    #[test]
    fn epoch_metrics_json_round_trip() {
        let original = sample_epoch_metrics();
        let json = serde_json::to_string(&original).unwrap();
        let restored: EpochMetrics = serde_json::from_str(&json).unwrap();
        assert_eq!(format!("{:?}", original), format!("{:?}", restored));
    }

    #[test]
    fn config_update_json_round_trip() {
        let original = ConfigUpdate {
            running: Some(false),
            max_epoch: Some(500),
            epoch_delay_ms: None,
            alpha: Some(2.0),
            beta: None,
            gamma: None,
            population_size: None,
            tournament_k: Some(7),
            max_depth: None,
        };
        let json = serde_json::to_string(&original).unwrap();
        let restored: ConfigUpdate = serde_json::from_str(&json).unwrap();
        assert_eq!(format!("{:?}", original), format!("{:?}", restored));
    }

    // -----------------------------------------------------------------------
    // 2. Entity → API conversion
    // -----------------------------------------------------------------------

    #[test]
    fn entity_model_to_epoch_metrics() {
        let entity = mathscape_entity::epoch::Model {
            epoch: 5,
            compression_ratio: 0.6,
            description_length: 80,
            raw_length: 200,
            novelty_total: 0.75,
            meta_compression: 0.38,
            library_size: 12,
            population_diversity: Some(0.55),
            expression_count: Some(400),
            alpha: 1.0,
            beta: 0.5,
            gamma: 0.25,
            phase: Some("compress".into()),
            duration_ms: Some(210),
            started_at: None,
            completed_at: None,
        };

        let metrics: EpochMetrics = entity.into();

        assert_eq!(metrics.epoch, 5);
        assert_eq!(metrics.compression_ratio, 0.6);
        assert_eq!(metrics.description_length, 80);
        assert_eq!(metrics.novelty_total, 0.75);
        assert_eq!(metrics.meta_compression, 0.38);
        assert_eq!(metrics.library_size, 12);
        assert_eq!(metrics.population_diversity, Some(0.55));
        assert_eq!(metrics.phase.as_deref(), Some("compress"));
        assert_eq!(metrics.duration_ms, Some(210));
        assert_eq!(metrics.alpha, 1.0);
        assert_eq!(metrics.beta, 0.5);
        assert_eq!(metrics.gamma, 0.25);
    }

    // -----------------------------------------------------------------------
    // 3. API → Proto conversions
    // -----------------------------------------------------------------------

    #[test]
    fn status_to_proto() {
        let status = sample_status();
        let proto: pb::StatusResponse = status.into();

        assert_eq!(proto.epoch, 42);
        assert!(proto.running);
        assert_eq!(proto.library_size, 15);
        assert_eq!(proto.population_size, 256);
        assert_eq!(proto.avg_fitness, 0.78);
        assert_eq!(proto.diversity, 0.65);

        let reward = proto.latest_reward.expect("latest_reward should be Some");
        assert_eq!(reward.reward, 1.5);
        assert_eq!(reward.compression_ratio, 0.42);
        assert_eq!(reward.description_length, 100);
        assert_eq!(reward.raw_length, 238);
        assert_eq!(reward.novelty_total, 0.87);
        assert_eq!(reward.meta_compression, 0.33);
    }

    #[test]
    fn status_to_proto_without_reward() {
        let status = Status {
            latest_reward: None,
            ..sample_status()
        };
        let proto: pb::StatusResponse = status.into();
        assert!(proto.latest_reward.is_none());
    }

    #[test]
    fn epoch_metrics_to_proto() {
        let metrics = sample_epoch_metrics();
        let proto: pb::EpochResponse = metrics.into();

        assert_eq!(proto.epoch, 7);
        assert_eq!(proto.compression_ratio, 0.55);
        assert_eq!(proto.description_length, 120);
        assert_eq!(proto.novelty_total, 0.91);
        assert_eq!(proto.meta_compression, 0.44);
        assert_eq!(proto.library_size, 10);
        assert_eq!(proto.population_diversity, Some(0.73));
        assert_eq!(proto.phase, Some("explore".into()));
        assert_eq!(proto.duration_ms, Some(340));
        assert_eq!(proto.alpha, 1.0);
        assert_eq!(proto.beta, 0.5);
        assert_eq!(proto.gamma, 0.25);
    }

    #[test]
    fn library_symbol_to_proto() {
        let sym = sample_library_symbol();
        let proto: pb::LibrarySymbol = sym.into();

        assert_eq!(proto.symbol_id, 3);
        assert_eq!(proto.name, "add");
        assert_eq!(proto.epoch_discovered, 2);
        assert_eq!(proto.arity, 2);
        assert_eq!(proto.lhs_sexpr, "(+ ?a ?b)");
        assert_eq!(proto.rhs_sexpr, "(add ?a ?b)");
        assert_eq!(proto.generality, Some(0.9));
        assert_eq!(proto.irreducibility, Some(0.8));
        assert!(!proto.is_meta);
        assert_eq!(proto.status, "active");
    }

    #[test]
    fn engine_config_to_proto() {
        let cfg = sample_engine_config();
        let proto: pb::ConfigResponse = cfg.into();

        assert!(proto.running);
        assert_eq!(proto.max_epoch, Some(1000));
        assert_eq!(proto.epoch_delay_ms, 50);
        assert_eq!(proto.alpha, 1.0);
        assert_eq!(proto.beta, 0.5);
        assert_eq!(proto.gamma, 0.25);
        assert_eq!(proto.population_size, 128);
        assert_eq!(proto.tournament_k, 5);
        assert_eq!(proto.max_depth, 12);
        assert_eq!(proto.elite_fraction, 0.1);
        assert_eq!(proto.crossover_rate, 0.7);
        assert_eq!(proto.min_shared_size, 3);
        assert_eq!(proto.min_matches, 2);
        assert_eq!(proto.max_new_rules, 10);
    }

    #[test]
    fn control_response_to_proto() {
        let resp = sample_control_response();
        let proto: pb::ControlResponse = resp.into();

        assert!(proto.success);
        assert_eq!(proto.message, "Engine paused");
    }

    // -----------------------------------------------------------------------
    // 4. Default / empty cases
    // -----------------------------------------------------------------------

    #[test]
    fn empty_epoch_list_serializes() {
        let list = EpochList {
            epochs: vec![],
            total: 0,
        };
        let json = serde_json::to_string(&list).unwrap();
        let restored: EpochList = serde_json::from_str(&json).unwrap();
        assert!(restored.epochs.is_empty());
        assert_eq!(restored.total, 0);
    }

    #[test]
    fn empty_library_list_serializes() {
        let list = LibraryList { symbols: vec![] };
        let json = serde_json::to_string(&list).unwrap();
        let restored: LibraryList = serde_json::from_str(&json).unwrap();
        assert!(restored.symbols.is_empty());
    }

    // -----------------------------------------------------------------------
    // 5. ConfigUpdate partial fields
    // -----------------------------------------------------------------------

    #[test]
    fn config_update_all_none() {
        let update = ConfigUpdate {
            running: None,
            max_epoch: None,
            epoch_delay_ms: None,
            alpha: None,
            beta: None,
            gamma: None,
            population_size: None,
            tournament_k: None,
            max_depth: None,
        };
        let json = serde_json::to_string(&update).unwrap();
        let restored: ConfigUpdate = serde_json::from_str(&json).unwrap();
        assert!(restored.running.is_none());
        assert!(restored.max_epoch.is_none());
        assert!(restored.epoch_delay_ms.is_none());
        assert!(restored.alpha.is_none());
        assert!(restored.beta.is_none());
        assert!(restored.gamma.is_none());
        assert!(restored.population_size.is_none());
        assert!(restored.tournament_k.is_none());
        assert!(restored.max_depth.is_none());
    }

    #[test]
    fn config_update_partial_fields_only() {
        let json = r#"{"running":true,"alpha":2.5}"#;
        let update: ConfigUpdate = serde_json::from_str(json).unwrap();
        assert_eq!(update.running, Some(true));
        assert_eq!(update.alpha, Some(2.5));
        // All other fields should be None when absent from JSON.
        assert!(update.max_epoch.is_none());
        assert!(update.epoch_delay_ms.is_none());
        assert!(update.beta.is_none());
        assert!(update.gamma.is_none());
        assert!(update.population_size.is_none());
        assert!(update.tournament_k.is_none());
        assert!(update.max_depth.is_none());
    }
}
