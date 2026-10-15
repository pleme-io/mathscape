use super::*;
use crate::builtin::{ADD, BUILTINS, MUL, NEG, INT_ADD};
use crate::hash::TermRef;
use crate::mathscape_map::MapEvent;
use crate::value::Value;
use std::collections::BTreeMap;

fn v(id: u32) -> Term {
    Term::Var(id)
}
fn ap(h: Term, args: Vec<Term>) -> Term {
    Term::Apply(Box::new(h), args)
}
fn nat(n: u64) -> Term {
    Term::Number(Value::Nat(n))
}
fn rule(name: &str, lhs: Term, rhs: Term) -> RewriteRule {
    RewriteRule { name: name.into(), lhs, rhs }
}

fn instantiate(p: &Pattern, head: bool, fresh: &mut BTreeMap<String, Term>) -> Term {
    match p {
        Pattern::Lit(t) => t.clone(),
        Pattern::Meta(n) => {
            let next = fresh.len() as u32;
            fresh
                .entry(n.clone())
                .or_insert_with(|| if head { v(90 + next) } else { v(100 + next) })
                .clone()
        }
        Pattern::Const(n) => {
            let next = fresh.len() as u64;
            fresh.entry(n.clone()).or_insert_with(|| nat(7 + next)).clone()
        }
        Pattern::App(h, args) => ap(
            instantiate(h, true, fresh),
            args.iter().map(|a| instantiate(a, false, fresh)).collect(),
        ),
    }
}

#[test]
fn every_kernel_builtin_has_an_operator_definition() {
    let vocabulary = vocabulary();
    for b in BUILTINS {
        let op = vocabulary
            .operator(b.id)
            .unwrap_or_else(|| panic!("builtin {} has no defoperator", b.name));
        if let Render::Infix(..) | Render::Frac = op.render {
            assert_eq!(b.arity, 2, "{} renders binary but has arity {}", b.name, b.arity);
        }
        if let Render::Prefix(_) | Render::Postfix(_) = op.render {
            assert_eq!(b.arity, 1, "{} renders unary but has arity {}", b.name, b.arity);
        }
        if let Render::Constant(_) = op.render {
            assert_eq!(b.arity, 0, "{} renders constant but has arity {}", b.name, b.arity);
        }
    }
    assert_eq!(vocabulary.operators.len(), BUILTINS.len());
}

#[test]
fn every_catalogued_invariant_is_reachable_by_its_own_shape() {
    for def in &vocabulary().invariants {
        for (lhs, rhs) in &def.forms {
            let mut fresh = BTreeMap::new();
            let r = rule(&def.id, instantiate(lhs, false, &mut fresh), instantiate(rhs, false, &mut fresh));
            let t = Translation::of(&r);
            let named = t
                .named()
                .unwrap_or_else(|| panic!("{} instance {} translated to nothing", def.id, t.internal_repr()));
            assert!(
                named.invariant_id == def.id || named.also_instance_of.contains(&def.id),
                "{} instance {} was named {} (also {:?})",
                def.id,
                t.internal_repr(),
                named.invariant_id,
                named.also_instance_of
            );
        }
    }
}

#[test]
fn every_structure_requirement_names_a_catalogued_property() {
    let vocabulary = vocabulary();
    let properties: Vec<&str> = vocabulary.invariants.iter().map(|i| i.property.as_str()).collect();
    for s in &vocabulary.structures {
        let required: Vec<&String> = match &s.shape {
            StructureShape::Single(r) => r.iter().collect(),
            StructureShape::Pair { additive, multiplicative, joint } => {
                additive.iter().chain(multiplicative).chain(joint).collect()
            }
        };
        for r in required {
            assert!(properties.contains(&r.as_str()), "{} requires unknown property {r}", s.id);
        }
    }
}

#[test]
fn additive_identity_is_named_in_either_argument_order() {
    let left = Translation::of(&rule("S_7", ap(v(ADD), vec![nat(0), v(100)]), v(100)));
    let right = Translation::of(&rule("S_8", ap(v(ADD), vec![v(100), nat(0)]), v(100)));
    for t in [&left, &right] {
        assert_eq!(t.canonical_name(), "Additive Identity");
        assert_eq!(t.math_domain(), "Peano Arithmetic");
        assert!(t.named().unwrap().also_instance_of.contains(&"identity-element".to_owned()));
    }
    assert_eq!(left.latex(), "0 + a = a");
    assert_eq!(right.latex(), "a + 0 = a");
}

#[test]
fn reversed_orientation_is_still_the_same_theorem() {
    let t = Translation::of(&rule("S_9", v(100), ap(v(MUL), vec![v(100), nat(1)])));
    let named = t.named().unwrap();
    assert_eq!(named.invariant_id, "mul-identity");
    assert_eq!(named.orientation, Orientation::Reversed);
}

#[test]
fn generic_laws_bind_the_operator_they_found() {
    let t = Translation::of(&rule(
        "S_10",
        ap(v(INT_ADD), vec![v(100), ap(v(NEG), vec![v(100)])]),
        Term::Number(Value::Int(0)),
    ));
    let named = t.named().unwrap();
    assert_eq!(named.invariant_id, "additive-inverse");
    assert_eq!(named.operators, vec!["int_add".to_owned(), "neg".to_owned()]);
    assert_eq!(t.latex(), "a + (-a) = 0");
    assert_eq!(t.internal_repr(), "(=> (int_add ?100 (neg ?100)) 0)");

    let abstract_op = Translation::of(&rule(
        "S_11",
        ap(v(60), vec![ap(v(60), vec![v(100), v(101)]), v(102)]),
        ap(v(60), vec![v(100), ap(v(60), vec![v(101), v(102)])]),
    ));
    let named = abstract_op.named().unwrap();
    assert_eq!(named.invariant_id, "associativity");
    assert_eq!(named.operators, vec!["op60".to_owned()]);
}

#[test]
fn uncatalogued_rules_are_characterized_not_left_opaque() {
    let t = Translation::of(&rule(
        "S_12",
        ap(v(ADD), vec![v(100), v(101)]),
        ap(v(MUL), vec![v(100), v(101)]),
    ));
    assert!(t.named().is_none());
    assert!(t.canonical_name().starts_with("Uncatalogued law over {add, mul}"));
    assert_eq!(t.signature().shape, LawShape::Restructuring);
    assert!(t.signature().left_linear);
    assert!(!t.semantic_meaning().is_empty());
    assert!(!t.latex().is_empty());
}

#[test]
fn latex_keeps_grouping_and_precedence_faithful() {
    let assoc = Translation::of(&rule(
        "assoc",
        ap(v(ADD), vec![ap(v(ADD), vec![v(100), v(101)]), v(102)]),
        ap(v(ADD), vec![v(100), ap(v(ADD), vec![v(101), v(102)])]),
    ));
    assert_eq!(assoc.latex(), "(a + b) + c = a + (b + c)");
    let dist = Translation::of(&rule(
        "dist",
        ap(v(MUL), vec![v(100), ap(v(ADD), vec![v(101), v(102)])]),
        ap(v(ADD), vec![ap(v(MUL), vec![v(100), v(101)]), ap(v(MUL), vec![v(100), v(102)])]),
    ));
    assert_eq!(dist.latex(), "a \\cdot (b + c) = a \\cdot b + a \\cdot c");
    assert_eq!(dist.canonical_name(), "Distributivity of Multiplication over Addition");
    assert_eq!(dist.signature().nodes_saved, -3);
    assert!(dist.signature().duplicating);
}

fn peano_library() -> Vec<RewriteRule> {
    vec![
        rule("add-id", ap(v(ADD), vec![nat(0), v(100)]), v(100)),
        rule("mul-id", ap(v(MUL), vec![nat(1), v(100)]), v(100)),
        rule("add-comm", ap(v(ADD), vec![v(100), v(101)]), ap(v(ADD), vec![v(101), v(100)])),
        rule("mul-comm", ap(v(MUL), vec![v(100), v(101)]), ap(v(MUL), vec![v(101), v(100)])),
        rule(
            "add-assoc",
            ap(v(ADD), vec![ap(v(ADD), vec![v(100), v(101)]), v(102)]),
            ap(v(ADD), vec![v(100), ap(v(ADD), vec![v(101), v(102)])]),
        ),
        rule(
            "mul-assoc",
            ap(v(MUL), vec![ap(v(MUL), vec![v(100), v(101)]), v(102)]),
            ap(v(MUL), vec![v(100), ap(v(MUL), vec![v(101), v(102)])]),
        ),
        rule(
            "dist",
            ap(v(MUL), vec![v(100), ap(v(ADD), vec![v(101), v(102)])]),
            ap(v(ADD), vec![ap(v(MUL), vec![v(100), v(101)]), ap(v(MUL), vec![v(100), v(102)])]),
        ),
    ]
}

#[test]
fn certified_peano_laws_establish_a_commutative_semiring() {
    let translations: Vec<Translation> = peano_library().iter().map(Translation::of).collect();
    let claims = structures(&translations);
    let find = |id: &str, ops: &[&str]| {
        claims
            .iter()
            .any(|c| c.structure_id == id && c.operators.iter().map(String::as_str).eq(ops.iter().copied()))
    };
    assert!(find("commutative-monoid", &["add"]), "{claims:#?}");
    assert!(find("commutative-monoid", &["mul"]), "{claims:#?}");
    assert!(find("commutative-semiring", &["add", "mul"]), "{claims:#?}");
}

#[test]
fn derivation_records_each_rewrite_as_a_proof_step() {
    let library = peano_library();
    let start = ap(v(MUL), vec![ap(v(ADD), vec![nat(0), nat(1)]), v(100)]);
    let d = derive(&start, &library[..2], 16);
    assert!(d.complete);
    assert_eq!(d.normal_form, v(100));
    assert_eq!(d.steps.len(), 2);
    match &d.steps[0].kind {
        StepKind::Rewrite { canonical_name, .. } => assert_eq!(canonical_name, "Additive Identity"),
        other => panic!("expected rewrite, got {other:?}"),
    }
    assert_eq!(d.steps[0].position, vec![0]);
}

#[test]
fn derivation_falls_back_to_kernel_computation() {
    let d = derive(&ap(v(ADD), vec![nat(2), nat(3)]), &[], 4);
    assert!(d.complete);
    assert_eq!(d.normal_form, nat(5));
    assert!(matches!(d.steps[0].kind, StepKind::Kernel { .. }));
}

#[test]
fn derivation_does_not_cycle_on_commutativity() {
    let library = peano_library();
    let d = derive(&ap(v(ADD), vec![v(100), v(101)]), &library[2..3], 50);
    assert!(d.complete);
    assert!(d.steps.len() <= 1);
}

#[test]
fn certified_event_carries_the_translation_of_its_own_rule() {
    let r = peano_library().remove(0);
    let MapEvent::RuleCertified { rule, translation, .. } = MapEvent::rule_certified(r.clone(), 32) else {
        panic!("constructor returned another variant");
    };
    assert_eq!(rule, r);
    assert_eq!(translation, Translation::of(&r));
}

#[test]
fn invariant_def_form_reads_back_through_tatara() {
    let t = Translation::of(&peano_library()[6]);
    let text = t.to_invariant_def("certified").to_string();
    let back = tatara_lisp::reader::read(&text).unwrap();
    assert_eq!(back.len(), 1);
    let items = back[0].as_list().unwrap();
    assert_eq!(items[0].as_symbol(), Some("invariant-def"));
    let get = |k: &str| {
        items
            .iter()
            .position(|s| s.as_keyword() == Some(k))
            .and_then(|i| items.get(i + 1))
            .cloned()
            .unwrap_or_else(|| panic!("missing :{k}"))
    };
    assert_eq!(get("canonical-name").as_string(), Some("Distributivity of Multiplication over Addition"));
    assert_eq!(get("latex").as_string(), Some("a \\cdot (b + c) = a \\cdot b + a \\cdot c"));
    assert_eq!(get("algebraic-status").as_keyword(), Some("certified"));
    assert_eq!(get("math-domain").as_string(), Some("Peano Arithmetic"));
}

#[test]
fn theory_document_renders_every_section() {
    let events: Vec<MapEvent> = peano_library()
        .into_iter()
        .chain([rule("weird", ap(v(ADD), vec![v(100), v(101)]), ap(v(MUL), vec![v(100), v(101)]))])
        .map(|r| MapEvent::rule_certified(r, 32))
        .collect();
    let trap = Trap::seal(
        TermRef::from_bytes(b"root"),
        10,
        Some(40),
        TermRef::from_bytes(b"policy"),
        Vec::new(),
        Some(TrapExitReason::ReinforcementPlateau),
    );
    let library = peano_library();
    let doc = TheoryDocument::from_events("Peano", &events)
        .with_trap(&trap)
        .with_derivation(derive(&ap(v(MUL), vec![nat(1), v(100)]), &library, 8));
    assert_eq!(doc.entries.len(), 8);

    let md = doc.to_markdown();
    for needle in [
        "## 1. Translation table",
        "## 2. Definitions and axioms",
        "## 3. Theorems",
        "## 4. Corollaries and reductions",
        "## 5. Fixed points (traps)",
        "## 6. Derivations",
        "Commutative semiring",
        "Uncatalogued law",
        "Gradient Descent Step",
    ] {
        assert!(md.to_lowercase().contains(&needle.to_lowercase()), "markdown lacks {needle}\n{md}");
    }

    let tex = doc.to_latex();
    assert!(tex.starts_with("\\documentclass{article}"));
    assert!(tex.trim_end().ends_with("\\end{document}"));
    assert_eq!(tex.matches("\\begin{").count(), tex.matches("\\end{").count(), "{tex}");
    assert_eq!(tex.matches('{').count(), tex.matches('}').count() + tex.matches("\\}").count() - tex.matches("\\{").count());
}

#[test]
fn certified_rule_exposes_its_translation() {
    let cr = crate::certification::CertifiedRule::new(
        peano_library().remove(1),
        crate::certification::CertificationLevel::Certified,
    );
    assert_eq!(cr.translation().canonical_name(), "Multiplicative Identity");
    let doc = TheoryDocument::from_certified("lib", &[cr]);
    assert_eq!(doc.entries[0].status, "certified");
}
