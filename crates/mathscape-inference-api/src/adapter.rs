//! `HandleAdapter` — the default `InferenceService` implementation
//! backed by `mathscape_core::LiveInferenceHandle`.

use crate::service::{InferenceService, ServiceError};
use crate::types::{
    CurriculumReportDto, EvalResult, InferRequest, LinearPolicyDto,
    RewriteRuleDto, TermDto,
};
use mathscape_core::LiveInferenceHandle;

pub struct HandleAdapter {
    handle: LiveInferenceHandle,
}

impl HandleAdapter {
    pub fn new(handle: LiveInferenceHandle) -> Self {
        Self { handle }
    }

    pub fn handle(&self) -> &LiveInferenceHandle {
        &self.handle
    }
}

impl InferenceService for HandleAdapter {
    fn infer(
        &self,
        req: &InferRequest,
    ) -> Result<EvalResult, ServiceError> {
        if req.step_limit == 0 {
            return Err(ServiceError::BadRequest(
                "step_limit must be > 0".into(),
            ));
        }
        let core_term: mathscape_core::term::Term = req.term.clone().into();
        let result = self.handle.infer(&core_term, req.step_limit as usize);
        Ok(match result {
            Ok(term) => EvalResult::Ok {
                value: TermDto::from(&term),
            },
            Err(e) => EvalResult::Err {
                message: e.to_string(),
            },
        })
    }

    fn current_competency(&self) -> Result<CurriculumReportDto, ServiceError> {
        let report = self.handle.current_competency();
        Ok((&report).into())
    }

    fn policy_snapshot(&self) -> Result<LinearPolicyDto, ServiceError> {
        let policy = self.handle.policy_snapshot();
        Ok((&policy).into())
    }

    fn library_snapshot(&self) -> Result<Vec<RewriteRuleDto>, ServiceError> {
        let lib = self.handle.library_snapshot();
        Ok(lib.iter().map(Into::into).collect())
    }

    fn library_size(&self) -> Result<usize, ServiceError> {
        Ok(self.handle.library_size())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mathscape_core::eval::RewriteRule;
    use mathscape_core::streaming_policy::StreamingPolicyTrainer;
    use mathscape_core::term::Term;
    use mathscape_core::value::Value;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn add_id() -> RewriteRule {
        RewriteRule {
            name: "add-id".into(),
            lhs: Term::Apply(
                Box::new(Term::Var(2)),
                vec![
                    Term::Number(Value::Nat(0)),
                    Term::Var(100),
                ],
            ),
            rhs: Term::Var(100),
        }
    }

    fn fresh_adapter() -> (HandleAdapter, Rc<RefCell<Vec<RewriteRule>>>) {
        let library = Rc::new(RefCell::new(Vec::new()));
        let trainer = Rc::new(StreamingPolicyTrainer::new(0.1));
        let handle = LiveInferenceHandle::new(library.clone(), trainer);
        (HandleAdapter::new(handle), library)
    }

    #[test]
    fn infer_reduces_term_via_library() {
        let (adapter, library) = fresh_adapter();
        library.borrow_mut().push(add_id());

        let req = InferRequest {
            term: TermDto::Apply {
                head: Box::new(TermDto::Var { symbol_id: 2 }),
                args: vec![
                    TermDto::Number {
                        value: crate::types::ValueDto::Nat { n: 0 },
                    },
                    TermDto::Var { symbol_id: 100 },
                ],
            },
            step_limit: 20,
        };
        let result = adapter.infer(&req).unwrap();
        match result {
            EvalResult::Ok { value } => {
                assert_eq!(value, TermDto::Var { symbol_id: 100 });
            }
            EvalResult::Err { message } => {
                panic!("expected Ok, got err: {message}")
            }
        }
    }

    #[test]
    fn infer_rejects_zero_step_limit() {
        let (adapter, _) = fresh_adapter();
        let req = InferRequest {
            term: TermDto::Var { symbol_id: 0 },
            step_limit: 0,
        };
        assert!(matches!(
            adapter.infer(&req),
            Err(ServiceError::BadRequest(_))
        ));
    }

    #[test]
    fn library_size_reflects_live_count() {
        let (adapter, library) = fresh_adapter();
        assert_eq!(adapter.library_size().unwrap(), 0);
        library.borrow_mut().push(add_id());
        library.borrow_mut().push(add_id());
        assert_eq!(adapter.library_size().unwrap(), 2);
    }

    #[test]
    fn library_snapshot_returns_all_rules() {
        let (adapter, library) = fresh_adapter();
        library.borrow_mut().push(add_id());
        let snap = adapter.library_snapshot().unwrap();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].name, "add-id");
    }

    #[test]
    fn policy_snapshot_returns_live_policy() {
        let (adapter, _) = fresh_adapter();
        let p = adapter.policy_snapshot().unwrap();
        assert_eq!(p.weights.len(), 9);
        assert_eq!(p.trained_steps, 0);
    }

    #[test]
    fn competency_breaks_down_by_subdomain() {
        let (adapter, library) = fresh_adapter();
        // Empty library → symbolic-nat should score 0.
        let report = adapter.current_competency().unwrap();
        assert!(report.per_subdomain.contains_key("symbolic-nat"));
        assert_eq!(
            report.per_subdomain.get("symbolic-nat").unwrap().solved_count,
            0
        );
        // After adding add-id, symbolic-nat gains some points.
        library.borrow_mut().push(add_id());
        let report2 = adapter.current_competency().unwrap();
        assert!(
            report2.per_subdomain.get("symbolic-nat").unwrap().solved_count > 0
        );
    }

    #[test]
    fn full_roundtrip_via_json_over_wire() {
        // Simulates an HTTP client: request → JSON → server →
        // response JSON → client. Every hop crosses the DTO
        // boundary.
        let (adapter, library) = fresh_adapter();
        library.borrow_mut().push(add_id());

        let req = InferRequest {
            term: TermDto::Apply {
                head: Box::new(TermDto::Var { symbol_id: 2 }),
                args: vec![
                    TermDto::Number {
                        value: crate::types::ValueDto::Nat { n: 0 },
                    },
                    TermDto::Number {
                        value: crate::types::ValueDto::Nat { n: 42 },
                    },
                ],
            },
            step_limit: 50,
        };
        // Client serializes.
        let wire_in = serde_json::to_string(&req).unwrap();
        // Server deserializes + handles.
        let server_req: InferRequest =
            serde_json::from_str(&wire_in).unwrap();
        let server_resp = adapter.infer(&server_req).unwrap();
        // Server serializes response.
        let wire_out = serde_json::to_string(&server_resp).unwrap();
        // Client deserializes.
        let client_resp: EvalResult =
            serde_json::from_str(&wire_out).unwrap();
        match client_resp {
            EvalResult::Ok { value } => {
                // add(0, 42) = 42 via the identity rule.
                assert_eq!(
                    value,
                    TermDto::Number {
                        value: crate::types::ValueDto::Nat { n: 42 },
                    }
                );
            }
            EvalResult::Err { message } => panic!("{message}"),
        }
    }
}
