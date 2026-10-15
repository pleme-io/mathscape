use super::vocabulary::{vocabulary, StructureShape};
use super::Translation;
use crate::builtin::BUILTINS;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StructureClaim {
    pub structure_id: String,
    pub structure_name: String,
    pub operators: Vec<String>,
    pub carrier: String,
    pub evidence: Vec<String>,
}

#[derive(Default)]
struct Facts {
    unary: BTreeMap<String, BTreeMap<String, Vec<String>>>,
    joint: BTreeMap<(String, String), BTreeMap<String, Vec<String>>>,
}

impl Facts {
    fn of(translations: &[Translation]) -> Self {
        let mut facts = Facts::default();
        for t in translations {
            let Some(n) = t.named() else { continue };
            let evidence = t.rule_name().to_owned();
            match (n.property.as_str(), n.operators.as_slice()) {
                ("distributivity" | "absorption", [outer, inner, ..]) => facts
                    .joint
                    .entry((outer.clone(), inner.clone()))
                    .or_default()
                    .entry(n.property.clone())
                    .or_default()
                    .push(evidence),
                (_, [op, ..]) => facts
                    .unary
                    .entry(op.clone())
                    .or_default()
                    .entry(n.property.clone())
                    .or_default()
                    .push(evidence),
                _ => {}
            }
        }
        facts
    }

    fn holds(&self, op: &str, required: &[String]) -> Option<Vec<String>> {
        let props = self.unary.get(op)?;
        let mut evidence = BTreeSet::new();
        for r in required {
            evidence.extend(props.get(r)?.iter().cloned());
        }
        Some(evidence.into_iter().collect())
    }

    fn joint_holds(&self, mul: &str, add: &str, required: &[String]) -> Option<Vec<String>> {
        let mut evidence = BTreeSet::new();
        for r in required {
            let forward = self.joint.get(&(mul.to_owned(), add.to_owned())).and_then(|m| m.get(r));
            let backward = self.joint.get(&(add.to_owned(), mul.to_owned())).and_then(|m| m.get(r));
            match (forward, backward, r.as_str()) {
                (Some(f), Some(b), "absorption") => {
                    evidence.extend(f.iter().cloned());
                    evidence.extend(b.iter().cloned());
                }
                (_, _, "absorption") => return None,
                (Some(f), _, _) => evidence.extend(f.iter().cloned()),
                _ => return None,
            }
        }
        Some(evidence.into_iter().collect())
    }
}

pub fn structures(translations: &[Translation]) -> Vec<StructureClaim> {
    let vocabulary = vocabulary();
    let facts = Facts::of(translations);
    let carrier = |op: &str| {
        BUILTINS
            .iter()
            .find(|b| b.name == op)
            .and_then(|b| vocabulary.operator(b.id))
            .map(|o| o.carrier.clone())
            .unwrap_or_else(|| "\\text{(discovered carrier)}".to_owned())
    };
    let mut claims = Vec::new();
    for op in facts.unary.keys() {
        for def in &vocabulary.structures {
            if let StructureShape::Single(required) = &def.shape {
                if let Some(evidence) = facts.holds(op, required) {
                    claims.push(StructureClaim {
                        structure_id: def.id.clone(),
                        structure_name: def.name.clone(),
                        operators: vec![op.clone()],
                        carrier: carrier(op),
                        evidence,
                    });
                    break;
                }
            }
        }
    }
    let pairs: BTreeSet<(String, String)> = facts.joint.keys().cloned().collect();
    for (mul, add) in pairs {
        for def in &vocabulary.structures {
            if let StructureShape::Pair { additive, multiplicative, joint } = &def.shape {
                let (Some(a), Some(m), Some(j)) = (
                    facts.holds(&add, additive),
                    facts.holds(&mul, multiplicative),
                    facts.joint_holds(&mul, &add, joint),
                ) else {
                    continue;
                };
                let evidence: BTreeSet<String> = a.into_iter().chain(m).chain(j).collect();
                claims.push(StructureClaim {
                    structure_id: def.id.clone(),
                    structure_name: def.name.clone(),
                    operators: vec![add.clone(), mul.clone()],
                    carrier: carrier(&add),
                    evidence: evidence.into_iter().collect(),
                });
                break;
            }
        }
    }
    claims
}
