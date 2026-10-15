mod derivation;
mod latex;
mod matcher;
mod render;
mod structure;
mod vocabulary;

pub use derivation::{derive, Derivation, DerivationStep, StepKind};
pub use latex::{operator_label, rule_latex, term_latex, term_latex_with, VarNames};
pub use matcher::Orientation;
pub use render::{TheoryDocument, TheoryEntry};
pub use structure::{structures, StructureClaim};
pub use vocabulary::{
    vocabulary, InvariantDef, OperatorDef, Pattern, ProcedureDef, Render, StructureDef,
    StructureShape, Vocabulary, VocabularyError,
};

use crate::builtin;
use crate::certification::rule_content_hash;
use crate::eval::RewriteRule;
use crate::term::Term;
use crate::trap::{Trap, TrapExitReason};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use tatara_lisp::ast::Sexp;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Translation {
    rule_hash: String,
    rule_name: String,
    internal_repr: String,
    latex: String,
    identity: Identity,
    signature: RuleSignature,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Identity {
    Named(NamedInvariant),
    Characterized(Characterization),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NamedInvariant {
    pub invariant_id: String,
    pub property: String,
    pub canonical_name: String,
    pub math_domain: String,
    pub schema_latex: String,
    pub semantic_meaning: String,
    pub orientation: Orientation,
    pub operators: Vec<String>,
    pub bindings: Vec<(String, String)>,
    pub also_instance_of: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Characterization {
    pub descriptive_name: String,
    pub description: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LawShape {
    CollapseToVariable,
    CollapseToConstant,
    Permutation,
    Restructuring,
    Reduction,
    Expansion,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RuleSignature {
    pub shape: LawShape,
    pub lhs_nodes: usize,
    pub rhs_nodes: usize,
    pub nodes_saved: i64,
    pub variables: usize,
    pub left_linear: bool,
    pub erasing: bool,
    pub duplicating: bool,
    pub operators: Vec<String>,
}

impl Translation {
    pub fn of(rule: &RewriteRule) -> Self {
        let vocabulary = vocabulary();
        let names = VarNames::for_rule(rule);
        let signature = RuleSignature::of(rule);
        let found = matcher::matches(vocabulary, rule);
        let identity = match found.split_first() {
            Some((primary, rest)) => {
                let mut also = Vec::new();
                for m in rest {
                    if m.def.id != primary.def.id && !also.contains(&m.def.id) {
                        also.push(m.def.id.clone());
                    }
                }
                Identity::Named(NamedInvariant {
                    invariant_id: primary.def.id.clone(),
                    property: primary.def.property.clone(),
                    canonical_name: primary.def.canonical_name.clone(),
                    math_domain: primary.def.math_domain.clone(),
                    schema_latex: primary.def.latex.clone(),
                    semantic_meaning: primary.def.semantic_meaning.clone(),
                    orientation: primary.orientation,
                    operators: pattern_operators(primary),
                    bindings: primary
                        .bindings
                        .iter()
                        .map(|(k, v)| (k.clone(), operator_label(v, &names, vocabulary)))
                        .collect(),
                    also_instance_of: also,
                })
            }
            None => Identity::Characterized(Characterization::of(&signature)),
        };
        Self {
            rule_hash: rule_content_hash(rule).to_string(),
            rule_name: rule.name.clone(),
            internal_repr: format!("(=> {} {})", internal_sexpr(&rule.lhs), internal_sexpr(&rule.rhs)),
            latex: rule_latex(rule, vocabulary),
            identity,
            signature,
        }
    }

    pub fn rule_hash(&self) -> &str {
        &self.rule_hash
    }

    pub fn rule_name(&self) -> &str {
        &self.rule_name
    }

    pub fn internal_repr(&self) -> &str {
        &self.internal_repr
    }

    pub fn latex(&self) -> &str {
        &self.latex
    }

    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    pub fn signature(&self) -> &RuleSignature {
        &self.signature
    }

    pub fn named(&self) -> Option<&NamedInvariant> {
        match &self.identity {
            Identity::Named(n) => Some(n),
            Identity::Characterized(_) => None,
        }
    }

    pub fn canonical_name(&self) -> &str {
        match &self.identity {
            Identity::Named(n) => &n.canonical_name,
            Identity::Characterized(c) => &c.descriptive_name,
        }
    }

    pub fn math_domain(&self) -> &str {
        match &self.identity {
            Identity::Named(n) => &n.math_domain,
            Identity::Characterized(_) => "Uncatalogued (characterized by structural properties)",
        }
    }

    pub fn semantic_meaning(&self) -> &str {
        match &self.identity {
            Identity::Named(n) => &n.semantic_meaning,
            Identity::Characterized(c) => &c.description,
        }
    }

    pub fn complexity_reduction(&self) -> String {
        let s = &self.signature;
        format!(
            "{} -> {} nodes per rewrite (delta-DL {:+})",
            s.lhs_nodes, s.rhs_nodes, s.nodes_saved
        )
    }

    pub fn to_invariant_def(&self, status: &str) -> Sexp {
        let mut items = vec![
            Sexp::symbol("invariant-def"),
            Sexp::keyword("id"),
            Sexp::string(self.rule_hash.clone()),
            Sexp::keyword("rule-name"),
            Sexp::string(self.rule_name.clone()),
            Sexp::keyword("internal-repr"),
            Sexp::string(self.internal_repr.clone()),
            Sexp::keyword("canonical-name"),
            Sexp::string(self.canonical_name()),
            Sexp::keyword("math-domain"),
            Sexp::string(self.math_domain()),
            Sexp::keyword("latex"),
            Sexp::string(self.latex.clone()),
            Sexp::keyword("semantic-meaning"),
            Sexp::string(self.semantic_meaning()),
            Sexp::keyword("complexity-reduction"),
            Sexp::string(self.complexity_reduction()),
            Sexp::keyword("algebraic-status"),
            Sexp::keyword(status),
        ];
        if let Some(n) = self.named() {
            items.push(Sexp::keyword("invariant"));
            items.push(Sexp::string(n.invariant_id.clone()));
            items.push(Sexp::keyword("schema-latex"));
            items.push(Sexp::string(n.schema_latex.clone()));
        }
        Sexp::List(items)
    }
}

fn pattern_operators(m: &matcher::Match<'_>) -> Vec<String> {
    fn heads(p: &Pattern, out: &mut Vec<Pattern>) {
        if let Pattern::App(h, args) = p {
            out.push((**h).clone());
            args.iter().for_each(|a| heads(a, out));
        }
    }
    let lhs = m
        .def
        .forms
        .iter()
        .find(|(l, _)| {
            let mut hs = Vec::new();
            heads(l, &mut hs);
            hs.iter().all(|h| match h {
                Pattern::Meta(name) => m.bindings.contains_key(name),
                _ => true,
            })
        })
        .map(|(l, _)| l)
        .unwrap_or(&m.def.forms[0].0);
    let mut hs = Vec::new();
    heads(lhs, &mut hs);
    let mut out: Vec<String> = Vec::new();
    for h in hs {
        let name = match h {
            Pattern::Lit(t) => operator_name(&t),
            Pattern::Meta(n) | Pattern::Const(n) => match m.bindings.get(&n) {
                Some(t) => operator_name(t),
                None => continue,
            },
            Pattern::App(..) => continue,
        };
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

pub fn internal_sexpr(t: &Term) -> String {
    match t {
        Term::Var(id) if *id < 100 => operator_name(t),
        Term::Apply(h, args) => {
            let mut parts = vec![internal_sexpr(h)];
            parts.extend(args.iter().map(internal_sexpr));
            format!("({})", parts.join(" "))
        }
        Term::Symbol(id, args) if !args.is_empty() => {
            let mut parts = vec![format!("S{id}")];
            parts.extend(args.iter().map(internal_sexpr));
            format!("({})", parts.join(" "))
        }
        Term::Fn(params, body) => {
            let ps: Vec<String> = params.iter().map(|p| format!("?{p}")).collect();
            format!("(fn ({}) {})", ps.join(" "), internal_sexpr(body))
        }
        other => other.to_string(),
    }
}

pub fn operator_name(t: &Term) -> String {
    match t {
        Term::Var(id) if *id < 100 => builtin::lookup(*id)
            .map(|b| b.name.to_owned())
            .unwrap_or_else(|| format!("op{id}")),
        Term::Symbol(id, _) => format!("S{id}"),
        other => other.to_string(),
    }
}

impl RuleSignature {
    pub fn of(rule: &RewriteRule) -> Self {
        let lhs_vars = var_counts(&rule.lhs);
        let rhs_vars = var_counts(&rule.rhs);
        let lhs_nodes = rule.lhs.size();
        let rhs_nodes = rule.rhs.size();
        let mut operators = BTreeSet::new();
        collect_operators(&rule.lhs, &mut operators);
        collect_operators(&rule.rhs, &mut operators);
        let shape = if matches!(rule.rhs, Term::Var(id) if id >= 100) {
            LawShape::CollapseToVariable
        } else if matcher::is_constant(&rule.rhs) {
            LawShape::CollapseToConstant
        } else if lhs_nodes == rhs_nodes && leaves(&rule.lhs) == leaves(&rule.rhs) {
            LawShape::Permutation
        } else if lhs_nodes == rhs_nodes {
            LawShape::Restructuring
        } else if lhs_nodes > rhs_nodes {
            LawShape::Reduction
        } else {
            LawShape::Expansion
        };
        Self {
            shape,
            lhs_nodes,
            rhs_nodes,
            nodes_saved: lhs_nodes as i64 - rhs_nodes as i64,
            variables: lhs_vars.len(),
            left_linear: lhs_vars.values().all(|c| *c == 1),
            erasing: lhs_vars.keys().any(|v| !rhs_vars.contains_key(v)),
            duplicating: rhs_vars.iter().any(|(v, c)| lhs_vars.get(v).is_some_and(|l| c > l)),
            operators: operators.into_iter().collect(),
        }
    }
}

impl Characterization {
    fn of(s: &RuleSignature) -> Self {
        let shape = match s.shape {
            LawShape::CollapseToVariable => "collapse to a variable",
            LawShape::CollapseToConstant => "collapse to a constant",
            LawShape::Permutation => "permutation of the same leaves",
            LawShape::Restructuring => "size-preserving restructuring",
            LawShape::Reduction => "size-reducing rewrite",
            LawShape::Expansion => "size-increasing rewrite",
        };
        let ops = if s.operators.is_empty() {
            "no operators".to_owned()
        } else {
            s.operators.join(", ")
        };
        let mut traits = Vec::new();
        traits.push(if s.left_linear { "left-linear" } else { "non-left-linear" });
        if s.erasing {
            traits.push("erasing");
        }
        if s.duplicating {
            traits.push("duplicating");
        }
        Self {
            descriptive_name: format!("Uncatalogued law over {{{ops}}}: {shape}"),
            description: format!(
                "No catalogued invariant matches. Structurally: {shape} ({} -> {} nodes), {} variable(s), {}, over {{{ops}}}.",
                s.lhs_nodes,
                s.rhs_nodes,
                s.variables,
                traits.join(", ")
            ),
        }
    }
}

fn var_counts(t: &Term) -> std::collections::BTreeMap<u32, usize> {
    fn walk(t: &Term, out: &mut std::collections::BTreeMap<u32, usize>) {
        match t {
            Term::Var(id) if *id >= 100 => *out.entry(*id).or_default() += 1,
            Term::Fn(_, body) => walk(body, out),
            Term::Apply(h, args) => {
                walk(h, out);
                args.iter().for_each(|a| walk(a, out));
            }
            Term::Symbol(_, args) => args.iter().for_each(|a| walk(a, out)),
            _ => {}
        }
    }
    let mut out = std::collections::BTreeMap::new();
    walk(t, &mut out);
    out
}

fn collect_operators(t: &Term, out: &mut BTreeSet<String>) {
    match t {
        Term::Apply(h, args) => {
            if !matches!(h.as_ref(), Term::Var(id) if *id >= 100) {
                out.insert(operator_name(h));
            }
            collect_operators(h, out);
            args.iter().for_each(|a| collect_operators(a, out));
        }
        Term::Symbol(id, args) => {
            out.insert(format!("S{id}"));
            args.iter().for_each(|a| collect_operators(a, out));
        }
        Term::Fn(_, body) => collect_operators(body, out),
        _ => {}
    }
}

fn leaves(t: &Term) -> Vec<Term> {
    fn walk(t: &Term, out: &mut Vec<Term>) {
        match t {
            Term::Apply(h, args) => {
                walk(h, out);
                args.iter().for_each(|a| walk(a, out));
            }
            Term::Symbol(id, args) => {
                out.push(Term::Symbol(*id, Vec::new()));
                args.iter().for_each(|a| walk(a, out));
            }
            Term::Fn(_, body) => walk(body, out),
            leaf => out.push(leaf.clone()),
        }
    }
    let mut out = Vec::new();
    walk(t, &mut out);
    out.sort();
    out
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrapTranslation {
    pub trap_hash: String,
    pub canonical_name: String,
    pub math_domain: String,
    pub latex: String,
    pub semantic_meaning: String,
    pub exit: Option<String>,
}

impl TrapTranslation {
    pub fn of(trap: &Trap) -> Self {
        let span = match trap.epoch_id_left {
            Some(left) => format!("epochs {}..{left}", trap.epoch_id_entered),
            None => format!("epochs {}.. (still held)", trap.epoch_id_entered),
        };
        Self {
            trap_hash: trap.content_hash.to_string(),
            canonical_name: "Fixed Point of the Discovery Operator".to_owned(),
            math_domain: "Dynamical Systems (fixed-point theory)".to_owned(),
            latex: format!(
                "\\Phi(\\mathcal{{L}}^{{*}}) = \\mathcal{{L}}^{{*}},\\quad \\mathcal{{L}}^{{*}} = \\texttt{{{}}}",
                trap.registry_root
            ),
            semantic_meaning: format!(
                "The registry state L* is invariant under one epoch of the machine Phi (no accepts, promotions or demotions) across {span}; an empirically stable fixed point, not a proven attractor."
            ),
            exit: trap.exit_reason.as_ref().map(exit_meaning),
        }
    }
}

fn exit_meaning(reason: &TrapExitReason) -> String {
    match reason {
        TrapExitReason::ReinforcementPlateau => {
            "Left by a discovery burst after reinforcement plateaued (vanishing delta-DL gradient).".to_owned()
        }
        TrapExitReason::PromotionFired(h) => format!("Left because primitive {h} was promoted (gates 4-5 cleared)."),
        TrapExitReason::DemotionFired(h) => format!("Left because primitive {h} was demoted."),
        TrapExitReason::CorpusRotation(c) => format!("Left because the corpus rotated to {c:?}."),
        TrapExitReason::PolicyChange(h) => format!("Left because the operator changed policy to {h}."),
    }
}

#[cfg(test)]
mod tests;
