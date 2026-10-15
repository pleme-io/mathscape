//! Typed API surface for mathscape live inference.
//!
//! Mirrors `apis/mathscape-inference/openapi.yaml` field-for-
//! field. The hot-path types in `mathscape-core` use serde's
//! default (externally-tagged) enum representation for
//! performance; this crate exposes `kind`-tagged DTOs that
//! match the discriminator shape in the OpenAPI spec.
//!
//! # Relationship to forge-gen
//!
//! This crate is *what forge-gen would generate* from
//! `openapi.yaml`. It's hand-written for now so the
//! implementation can live ahead of the forge-gen invocation;
//! when the generator runs, it will (by convention) land
//! compatible code here. Both paths are valid, both produce the
//! same types.
//!
//! # Shape summary
//!
//! - `TermDto` / `ValueDto` / `RewriteRuleDto` — core substrate
//!   DTOs matching OpenAPI schemas exactly.
//! - `InferRequest` / `EvalResult` — endpoint request/response.
//! - `LinearPolicyDto` / `CurriculumReportDto` — snapshot DTOs.
//! - `InferenceService` — trait that any backend implements to
//!   serve the API. REST, gRPC, GraphQL, MCP servers all route
//!   through `InferenceService`.
//! - `HandleAdapter` — default implementation backed by
//!   `mathscape_core::LiveInferenceHandle`.

pub mod types;
pub mod service;
pub mod adapter;

pub use adapter::HandleAdapter;
pub use service::{InferenceService, ServiceError};
pub use types::{
    BenchmarkReportDto, CurriculumReportDto, EvalResult, InferRequest,
    LinearPolicyDto, ProblemResultDto, RewriteRuleDto, TermDto, ValueDto,
};
