//! The `(defacquisition …)` authoring surface round-trips: a spec →
//! Sexp → spec is byte-identical, and a malformed form is rejected at
//! the parse boundary (a typed `SpecError::Parse`, never a bad spec).

use mathscape_acquisition::{AcquisitionOp, AcquisitionSpec, Complexity, SpecError};
use mathscape_core::eval::RewriteRule;
use mathscape_core::term::Term;
use mathscape_core::value::Value;

fn sample_spec() -> AcquisitionSpec {
    let id_rule = RewriteRule {
        name: "add-identity".to_string(),
        // (add ?100 0) — Display: (?2 ?100 0), parse round-trips via ?2
        lhs: Term::Apply(
            Box::new(Term::Var(2)),
            vec![Term::Var(100), Term::Number(Value::Nat(0))],
        ),
        rhs: Term::Var(100),
    };
    let const_rule = RewriteRule {
        name: "mul-zero".to_string(),
        lhs: Term::Apply(
            Box::new(Term::Var(3)), // mul
            vec![Term::Var(101), Term::Number(Value::Nat(0))],
        ),
        rhs: Term::Number(Value::Nat(0)),
    };
    AcquisitionSpec::new("peano-ratchet", Complexity(64))
        .seal(id_rule, 16)
        .absorb(const_rule, 16)
        .fold_back(0)
        .advance(Complexity(128))
}

#[test]
fn defacquisition_round_trips_exactly() {
    let spec = sample_spec();
    let sexp = spec.to_sexp();
    let back = AcquisitionSpec::from_sexp(&sexp).expect("round-trip parse");
    assert_eq!(spec, back, "spec → Sexp → spec must be identical");
}

#[test]
fn defacquisition_preserves_every_op_kind() {
    let spec = sample_spec();
    let back = AcquisitionSpec::from_sexp(&spec.to_sexp()).unwrap();
    assert_eq!(back.ops.len(), 4);
    assert!(matches!(back.ops[0], AcquisitionOp::SealLayer { k: 16, .. }));
    assert!(matches!(back.ops[1], AcquisitionOp::Absorb { k: 16, .. }));
    assert!(matches!(
        back.ops[2],
        AcquisitionOp::FoldBack { primitive_index: 0 }
    ));
    assert!(matches!(
        back.ops[3],
        AcquisitionOp::AdvanceFrontier {
            to: Complexity(128)
        }
    ));
}

#[test]
fn malformed_form_is_rejected_at_the_parse_boundary() {
    use tatara_lisp::ast::Sexp;
    // Not a (defacquisition …) head.
    let bad = Sexp::List(vec![Sexp::symbol("defsomethingelse")]);
    let res = AcquisitionSpec::from_sexp(&bad);
    assert!(matches!(res, Err(SpecError::Parse { .. })));
}

#[test]
fn unknown_op_is_a_typed_parse_error() {
    use tatara_lisp::ast::Sexp;
    let form = Sexp::List(vec![
        Sexp::symbol("defacquisition"),
        Sexp::keyword("name"),
        Sexp::string("x"),
        Sexp::keyword("genesis-ceiling"),
        Sexp::int(64),
        Sexp::keyword("ops"),
        Sexp::List(vec![Sexp::List(vec![Sexp::symbol("teleport")])]),
    ]);
    let res = AcquisitionSpec::from_sexp(&form);
    assert!(matches!(res, Err(SpecError::Parse { .. })));
}
