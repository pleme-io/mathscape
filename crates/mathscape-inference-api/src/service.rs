//! The service trait — one interface all transport backends
//! (REST, gRPC, GraphQL, MCP) route through.

use crate::types::{
    CurriculumReportDto, EvalResult, InferRequest, LinearPolicyDto,
    RewriteRuleDto,
};

/// Errors that can be surfaced across any transport.
#[derive(Debug)]
pub enum ServiceError {
    /// A required input was malformed or out of range.
    BadRequest(String),
    /// Internal error unrelated to the request shape.
    Internal(String),
}

impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServiceError::BadRequest(m) => write!(f, "bad request: {m}"),
            ServiceError::Internal(m) => write!(f, "internal: {m}"),
        }
    }
}

impl std::error::Error for ServiceError {}

/// The five operations from `apis/mathscape-inference/openapi.yaml`.
/// Any backend (REST/gRPC/GraphQL/MCP) calls these; any
/// implementation (LiveInferenceHandle, mock, distributed) can
/// stand behind them.
pub trait InferenceService {
    /// POST /infer → reduce term with current library
    fn infer(
        &self,
        req: &InferRequest,
    ) -> Result<EvalResult, ServiceError>;

    /// GET /competency → current per-subdomain breakdown
    fn current_competency(&self) -> Result<CurriculumReportDto, ServiceError>;

    /// GET /model/policy → clone of current policy
    fn policy_snapshot(&self) -> Result<LinearPolicyDto, ServiceError>;

    /// GET /model/library → clone of current rule set
    fn library_snapshot(&self) -> Result<Vec<RewriteRuleDto>, ServiceError>;

    /// GET /model/library/size → count of rules
    fn library_size(&self) -> Result<usize, ServiceError>;
}
