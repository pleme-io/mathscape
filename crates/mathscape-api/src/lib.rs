//! Shared API types with REST (serde), GraphQL (async-graphql), and gRPC (prost)
//! representations.
//!
//! This crate is the **central** API layer. All three protocols serve the same
//! data structures. Entity ↔ API ↔ Proto conversions are defined here.
//!
//! Architecture:
//!   SeaORM entities (DB) → API types → REST (JSON) / GraphQL / gRPC (proto)

pub mod graphql;
pub mod types;

/// Generated protobuf types from `proto/mathscape/v1/engine.proto`.
pub mod proto {
    pub mod mathscape {
        pub mod v1 {
            tonic::include_proto!("mathscape.v1");
        }
    }
}
