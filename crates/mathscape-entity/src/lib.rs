//! SeaORM entities — canonical data model for all mathscape tables.
//!
//! These entities are the single source of truth. API types (REST, GraphQL, gRPC)
//! all derive from these via conversion traits in `mathscape-api`.

pub mod epoch;
pub mod eval_trace;
pub mod library;
pub mod lineage_event;
pub mod population;
pub mod proof;
pub mod proof_dep;
pub mod symbol_dep;

#[cfg(test)]
mod tests {
    use serde_json;

    #[test]
    fn epoch_model_json_round_trip() {
        let model = super::epoch::Model {
            epoch: 42,
            compression_ratio: 0.85,
            description_length: 120,
            raw_length: 800,
            novelty_total: 0.6,
            meta_compression: 0.3,
            library_size: 15,
            population_diversity: Some(0.72),
            expression_count: Some(500),
            alpha: 0.4,
            beta: 0.3,
            gamma: 0.3,
            phase: Some("explore".into()),
            duration_ms: Some(1234),
            started_at: None,
            completed_at: None,
        };
        let json = serde_json::to_string(&model).unwrap();
        let restored: super::epoch::Model = serde_json::from_str(&json).unwrap();
        assert_eq!(model, restored);
    }

    #[test]
    fn library_model_json_round_trip() {
        let model = super::library::Model {
            symbol_id: 7,
            name: "add-identity".into(),
            epoch_discovered: 3,
            lhs_hash: vec![0xAB, 0xCD],
            rhs_hash: vec![0xEF, 0x01],
            arity: 2,
            generality: Some(0.95),
            irreducibility: Some(0.8),
            is_meta: Some(false),
            status: "active".into(),
        };
        let json = serde_json::to_string(&model).unwrap();
        let restored: super::library::Model = serde_json::from_str(&json).unwrap();
        assert_eq!(model, restored);
    }

    #[test]
    fn population_model_json_round_trip() {
        let model = super::population::Model {
            epoch: 10,
            individual: 42,
            root_hash: vec![1, 2, 3, 4],
            fitness: 0.92,
            cr_contrib: Some(0.5),
            novelty: Some(0.3),
            depth_bin: Some(3),
            op_diversity: Some(5),
            cr_bin: Some(2),
        };
        let json = serde_json::to_string(&model).unwrap();
        let restored: super::population::Model = serde_json::from_str(&json).unwrap();
        assert_eq!(model, restored);
    }

    #[test]
    fn eval_trace_model_json_round_trip() {
        let model = super::eval_trace::Model {
            trace_id: 1,
            expr_hash: vec![0xDE, 0xAD],
            step_index: 3,
            rule_applied: "add-identity".into(),
            before_hash: vec![0xBE, 0xEF],
            after_hash: vec![0xCA, 0xFE],
            epoch: 5,
        };
        let json = serde_json::to_string(&model).unwrap();
        let restored: super::eval_trace::Model = serde_json::from_str(&json).unwrap();
        assert_eq!(model, restored);
    }

    #[test]
    fn proof_model_json_round_trip() {
        let model = super::proof::Model {
            proof_id: 1,
            symbol_id: 7,
            proof_type: "equational".into(),
            status: "verified".into(),
            lhs_hash: vec![1, 2],
            rhs_hash: vec![3, 4],
            trace_ids: vec![5, 6],
            epoch_found: 10,
            epoch_verified: Some(12),
            lean_export: Some("theorem add_zero ...".into()),
        };
        let json = serde_json::to_string(&model).unwrap();
        let restored: super::proof::Model = serde_json::from_str(&json).unwrap();
        assert_eq!(model, restored);
    }

    #[test]
    fn lineage_event_model_json_round_trip() {
        let model = super::lineage_event::Model {
            event_id: 1,
            child_hash: vec![0x01],
            parent1_hash: Some(vec![0x02]),
            parent2_hash: None,
            mutation_type: "point".into(),
            operator: Some("swap-leaf".into()),
            epoch: 8,
        };
        let json = serde_json::to_string(&model).unwrap();
        let restored: super::lineage_event::Model = serde_json::from_str(&json).unwrap();
        assert_eq!(model, restored);
    }

    #[test]
    fn symbol_dep_model_json_round_trip() {
        let model = super::symbol_dep::Model {
            symbol_id: 3,
            depends_on: 1,
            depth: 2,
        };
        let json = serde_json::to_string(&model).unwrap();
        let restored: super::symbol_dep::Model = serde_json::from_str(&json).unwrap();
        assert_eq!(model, restored);
    }

    #[test]
    fn proof_dep_model_json_round_trip() {
        let model = super::proof_dep::Model {
            proof_id: 5,
            depends_on: 2,
        };
        let json = serde_json::to_string(&model).unwrap();
        let restored: super::proof_dep::Model = serde_json::from_str(&json).unwrap();
        assert_eq!(model, restored);
    }

    #[test]
    fn epoch_model_clone_and_eq() {
        let model = super::epoch::Model {
            epoch: 1,
            compression_ratio: 0.5,
            description_length: 100,
            raw_length: 200,
            novelty_total: 0.3,
            meta_compression: 0.1,
            library_size: 5,
            population_diversity: None,
            expression_count: None,
            alpha: 0.4,
            beta: 0.3,
            gamma: 0.3,
            phase: None,
            duration_ms: None,
            started_at: None,
            completed_at: None,
        };
        let cloned = model.clone();
        assert_eq!(model, cloned);
    }

    #[test]
    fn epoch_optional_fields_serialize_as_null() {
        let model = super::epoch::Model {
            epoch: 1,
            compression_ratio: 0.0,
            description_length: 0,
            raw_length: 0,
            novelty_total: 0.0,
            meta_compression: 0.0,
            library_size: 0,
            population_diversity: None,
            expression_count: None,
            alpha: 1.0,
            beta: 0.0,
            gamma: 0.0,
            phase: None,
            duration_ms: None,
            started_at: None,
            completed_at: None,
        };
        let json = serde_json::to_string(&model).unwrap();
        assert!(json.contains("null"));
    }
}
