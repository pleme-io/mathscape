use super::derivation::{Derivation, StepKind};
use super::latex::{term_latex_with, VarNames};
use super::structure::{structures, StructureClaim};
use super::vocabulary::vocabulary;
use super::{Identity, TrapTranslation, Translation};
use crate::builtin::BUILTINS;
use crate::certification::CertifiedRule;
use crate::mathscape_map::MapEvent;
use crate::trap::Trap;
use std::collections::BTreeSet;
use std::fmt::Write as _;

#[derive(Clone, Debug, PartialEq)]
pub struct TheoryEntry {
    pub translation: Translation,
    pub status: String,
    pub evidence_samples: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TheoryDocument {
    pub title: String,
    pub entries: Vec<TheoryEntry>,
    pub rejected: Vec<(Translation, String)>,
    pub traps: Vec<TrapTranslation>,
    pub derivations: Vec<Derivation>,
}

impl TheoryDocument {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Self::default()
        }
    }

    pub fn from_events<'e>(title: impl Into<String>, events: impl IntoIterator<Item = &'e MapEvent>) -> Self {
        let mut doc = Self::new(title);
        for event in events {
            doc.observe(event);
        }
        doc
    }

    pub fn from_certified(title: impl Into<String>, rules: &[CertifiedRule]) -> Self {
        let mut doc = Self::new(title);
        for cr in rules {
            doc.push(cr.translation(), level_keyword(cr), cr.evidence_samples);
        }
        doc
    }

    pub fn observe(&mut self, event: &MapEvent) {
        match event {
            MapEvent::RuleCertified {
                translation,
                evidence_samples,
                ..
            } => self.push(translation.clone(), "certified".to_owned(), *evidence_samples),
            MapEvent::RuleRejectedAtCertification { rule, reason } => {
                self.rejected.push((Translation::of(rule), reason.clone()));
            }
            _ => {}
        }
    }

    pub fn push(&mut self, translation: Translation, status: String, evidence_samples: usize) {
        if self
            .entries
            .iter()
            .any(|e| e.translation.rule_hash() == translation.rule_hash())
        {
            return;
        }
        self.entries.push(TheoryEntry {
            translation,
            status,
            evidence_samples,
        });
    }

    pub fn with_trap(mut self, trap: &Trap) -> Self {
        self.traps.push(TrapTranslation::of(trap));
        self
    }

    pub fn with_derivation(mut self, derivation: Derivation) -> Self {
        self.derivations.push(derivation);
        self
    }

    pub fn structures(&self) -> Vec<StructureClaim> {
        let translations: Vec<Translation> = self.entries.iter().map(|e| e.translation.clone()).collect();
        structures(&translations)
    }

    fn operators_used(&self) -> Vec<&'static str> {
        let used: BTreeSet<&str> = self
            .entries
            .iter()
            .flat_map(|e| e.translation.signature().operators.iter().map(String::as_str))
            .collect();
        BUILTINS.iter().map(|b| b.name).filter(|n| used.contains(n)).collect()
    }

    fn theorem_number(&self, rule_name: &str) -> Option<usize> {
        self.entries
            .iter()
            .position(|e| e.translation.rule_name() == rule_name)
            .map(|i| i + 1)
    }

    pub fn to_markdown(&self) -> String {
        let v = vocabulary();
        let mut s = String::new();
        let _ = writeln!(s, "# {}\n", self.title);

        let _ = writeln!(s, "## 1. Translation table\n");
        let _ = writeln!(
            s,
            "| # | Rule | Internal representation | Canonical name | Domain | LaTeX | Status | Reduction |"
        );
        let _ = writeln!(s, "|---|---|---|---|---|---|---|---|");
        for (i, e) in self.entries.iter().enumerate() {
            let t = &e.translation;
            let _ = writeln!(
                s,
                "| {} | `{}` | `{}` | {} | {} | ${}$ | {} | {} |",
                i + 1,
                cell(t.rule_name()),
                cell(t.internal_repr()),
                cell(t.canonical_name()),
                cell(t.math_domain()),
                cell(t.latex()),
                e.status,
                cell(&t.complexity_reduction()),
            );
        }

        let _ = writeln!(s, "\n## 2. Definitions and axioms\n");
        let _ = writeln!(s, "| Operator | Name | Carrier | Kernel axiom |");
        let _ = writeln!(s, "|---|---|---|---|");
        for name in self.operators_used() {
            if let Some(op) = BUILTINS.iter().find(|b| b.name == name).and_then(|b| v.operator(b.id)) {
                let _ = writeln!(
                    s,
                    "| `{}` | {} | ${}$ | ${}$ |",
                    op.name,
                    op.math_name,
                    cell(&op.carrier),
                    cell(&op.axiom)
                );
            }
        }
        let _ = writeln!(s, "\nKernel procedures (Rust, not certified rules):\n");
        for p in &v.procedures {
            let _ = writeln!(
                s,
                "- **{}** ({}, {}): ${}$, implemented by `{}`",
                p.canonical_name, p.layer, p.math_domain, p.latex, p.implemented_by
            );
        }

        let _ = writeln!(s, "\n## 3. Theorems\n");
        for (i, e) in self.entries.iter().enumerate() {
            let t = &e.translation;
            let _ = writeln!(s, "**Theorem {} ({}).** $${}$$", i + 1, t.canonical_name(), t.latex());
            match t.identity() {
                Identity::Named(n) => {
                    let _ = writeln!(
                        s,
                        "Schema ${}${}. {}",
                        n.schema_latex,
                        bindings_text(&n.bindings),
                        n.semantic_meaning
                    );
                    if !n.also_instance_of.is_empty() {
                        let _ = writeln!(s, "Also an instance of: {}.", also_names(&n.also_instance_of));
                    }
                }
                Identity::Characterized(c) => {
                    let _ = writeln!(s, "{}", c.description);
                }
            }
            let _ = writeln!(
                s,
                "Status: {} ({} evidence samples). Rule `{}`, hash `{}`.\n",
                e.status,
                e.evidence_samples,
                t.rule_name(),
                t.rule_hash()
            );
        }

        let _ = writeln!(s, "## 4. Corollaries and reductions\n");
        let claims = self.structures();
        if claims.is_empty() {
            let _ = writeln!(s, "No algebraic structure is established by the certified laws yet.\n");
        }
        for (i, c) in claims.iter().enumerate() {
            let _ = writeln!(
                s,
                "**Corollary {}.** $({}, {})$ is a {} (from {}).\n",
                i + 1,
                c.carrier,
                c.operators.join(", "),
                c.structure_name.to_lowercase(),
                self.evidence_text(&c.evidence)
            );
        }
        let total: i64 = self.entries.iter().map(|e| e.translation.signature().nodes_saved).sum();
        let _ = writeln!(
            s,
            "Each application of a theorem changes description length by its node delta; summed over one application of every theorem: {total:+} nodes.\n"
        );

        if !self.traps.is_empty() {
            let _ = writeln!(s, "## 5. Fixed points (traps)\n");
            for t in &self.traps {
                let _ = writeln!(s, "- **{}** `{}`: ${}$. {}", t.canonical_name, t.trap_hash, t.latex, t.semantic_meaning);
                if let Some(exit) = &t.exit {
                    let _ = writeln!(s, "  {exit}");
                }
            }
            let _ = writeln!(s);
        }

        if !self.derivations.is_empty() {
            let _ = writeln!(s, "## 6. Derivations\n");
            for d in &self.derivations {
                s.push_str(&self.derivation_markdown(d));
            }
        }

        if !self.rejected.is_empty() {
            let _ = writeln!(s, "## Rejected at certification\n");
            for (t, reason) in &self.rejected {
                let _ = writeln!(s, "- `{}` ${}$ ({}): {}", t.rule_name(), t.latex(), t.canonical_name(), reason);
            }
        }
        s
    }

    fn evidence_text(&self, rules: &[String]) -> String {
        let refs: Vec<String> = rules
            .iter()
            .map(|r| match self.theorem_number(r) {
                Some(n) => format!("Theorem {n}"),
                None => format!("`{r}`"),
            })
            .collect();
        refs.join(", ")
    }

    fn derivation_markdown(&self, d: &Derivation) -> String {
        let v = vocabulary();
        let names = VarNames::for_term(&d.start);
        let mut s = String::new();
        let _ = writeln!(s, "$$\\begin{{aligned}}");
        let _ = writeln!(s, "& {} \\\\", term_latex_with(&d.start, &names, v));
        for step in &d.steps {
            let _ = writeln!(
                s,
                "&= {} && \\text{{{}}} \\\\",
                term_latex_with(&step.after, &names, v),
                tex_text(&self.justification(&step.kind))
            );
        }
        let _ = writeln!(s, "\\end{{aligned}}$$\n");
        for (i, step) in d.steps.iter().enumerate() {
            let _ = writeln!(
                s,
                "{}. at position {:?}: {}",
                i + 1,
                step.position,
                self.proof_step_text(&step.kind)
            );
        }
        let _ = writeln!(
            s,
            "\n{}\n",
            if d.complete {
                "The final term is a normal form under the library and the kernel."
            } else {
                "Stopped at the step limit; the final term may not be a normal form."
            }
        );
        s
    }

    fn justification(&self, kind: &StepKind) -> String {
        match kind {
            StepKind::Rewrite { rule_name, canonical_name, .. } => match self.theorem_number(rule_name) {
                Some(n) => format!("Thm {n} ({canonical_name})"),
                None => canonical_name.clone(),
            },
            StepKind::Kernel { operator, .. } => format!("kernel: {operator}"),
        }
    }

    fn proof_step_text(&self, kind: &StepKind) -> String {
        match kind {
            StepKind::Rewrite { rule_name, canonical_name, substitution, modulo_commutativity } => format!(
                "substitution instance of {} (`{rule_name}`, {canonical_name}) under ${}${}, closed under congruence",
                self.theorem_number(rule_name)
                    .map(|n| format!("Theorem {n}"))
                    .unwrap_or_else(|| "a library law".to_owned()),
                substitution_latex(substitution),
                if *modulo_commutativity { ", applied modulo the kernel's commutativity of the operator" } else { "" }
            ),
            StepKind::Kernel { operator, axiom } => format!(
                "definitional computation by the kernel operator `{operator}` (axiom ${axiom}$)"
            ),
        }
    }

    pub fn to_latex(&self) -> String {
        let v = vocabulary();
        let mut s = String::new();
        let _ = writeln!(s, "\\documentclass{{article}}");
        let _ = writeln!(s, "\\usepackage{{amsmath,amssymb,amsthm}}");
        let _ = writeln!(s, "\\usepackage[utf8]{{inputenc}}");
        let _ = writeln!(s, "\\newtheorem{{theorem}}{{Theorem}}");
        let _ = writeln!(s, "\\newtheorem{{corollary}}{{Corollary}}");
        let _ = writeln!(s, "\\theoremstyle{{definition}}");
        let _ = writeln!(s, "\\newtheorem{{definition}}{{Definition}}");
        let _ = writeln!(s, "\\title{{{}}}", tex_text(&self.title));
        let _ = writeln!(s, "\\date{{}}");
        let _ = writeln!(s, "\\begin{{document}}");
        let _ = writeln!(s, "\\maketitle\n");

        let _ = writeln!(s, "\\section{{Definitions and axioms}}");
        for name in self.operators_used() {
            if let Some(op) = BUILTINS.iter().find(|b| b.name == name).and_then(|b| v.operator(b.id)) {
                let _ = writeln!(
                    s,
                    "\\begin{{definition}}[{}]\nOn ${}$: \\[ {} \\]\n\\end{{definition}}\n",
                    tex_text(&op.math_name),
                    op.carrier,
                    op.axiom
                );
            }
        }
        if !v.procedures.is_empty() {
            let _ = writeln!(s, "Kernel procedures (implemented in the kernel, not certified rules):");
            let _ = writeln!(s, "\\begin{{itemize}}");
            for p in &v.procedures {
                let _ = writeln!(s, "\\item {} ({}): ${}$", tex_text(&p.canonical_name), tex_text(&p.layer), p.latex);
            }
            let _ = writeln!(s, "\\end{{itemize}}\n");
        }

        let _ = writeln!(s, "\\section{{Theorems}}");
        for e in &self.entries {
            let t = &e.translation;
            let _ = writeln!(s, "\\begin{{theorem}}[{}]\n\\[ {} \\]\n\\end{{theorem}}", tex_text(t.canonical_name()), t.latex());
            match t.identity() {
                Identity::Named(n) => {
                    let _ = writeln!(
                        s,
                        "Schema ${}${}. {}",
                        n.schema_latex,
                        bindings_text(&n.bindings),
                        tex_text(&n.semantic_meaning)
                    );
                    if !n.also_instance_of.is_empty() {
                        let _ = writeln!(s, "Also an instance of: {}.", tex_text(&also_names(&n.also_instance_of)));
                    }
                }
                Identity::Characterized(c) => {
                    let _ = writeln!(s, "{}", tex_text(&c.description));
                }
            }
            let _ = writeln!(
                s,
                "Status: \\textsc{{{}}} ({} samples); rule \\texttt{{{}}}; reduction {}.\n",
                tex_text(&e.status),
                e.evidence_samples,
                tex_text(t.rule_name()),
                tex_text(&t.complexity_reduction())
            );
        }

        let _ = writeln!(s, "\\section{{Corollaries and reductions}}");
        for c in self.structures() {
            let _ = writeln!(
                s,
                "\\begin{{corollary}}\n$({}, {})$ is a {}, by {}.\n\\end{{corollary}}\n",
                c.carrier,
                tex_text(&c.operators.join(", ")),
                tex_text(&c.structure_name.to_lowercase()),
                tex_text(&self.evidence_text(&c.evidence).replace('`', ""))
            );
        }
        let total: i64 = self.entries.iter().map(|e| e.translation.signature().nodes_saved).sum();
        let _ = writeln!(s, "Net description-length change for one application of every theorem: ${total:+}$ nodes.\n");

        if !self.traps.is_empty() {
            let _ = writeln!(s, "\\section{{Fixed points}}");
            for t in &self.traps {
                let _ = writeln!(s, "\\paragraph{{{}}} \\[ {} \\] {}\n", tex_text(&t.canonical_name), t.latex, tex_text(&t.semantic_meaning));
            }
        }

        if !self.derivations.is_empty() {
            let _ = writeln!(s, "\\section{{Derivations}}");
            for d in &self.derivations {
                let names = VarNames::for_term(&d.start);
                let _ = writeln!(s, "\\begin{{align*}}");
                let _ = writeln!(s, "& {} \\\\", term_latex_with(&d.start, &names, v));
                for step in &d.steps {
                    let _ = writeln!(
                        s,
                        "&= {} && \\text{{{}}} \\\\",
                        term_latex_with(&step.after, &names, v),
                        tex_text(&self.justification(&step.kind))
                    );
                }
                let _ = writeln!(s, "\\end{{align*}}\n");
            }
        }

        let _ = writeln!(s, "\\end{{document}}");
        s
    }
}

fn level_keyword(cr: &CertifiedRule) -> String {
    format!("{:?}", cr.level).to_lowercase()
}

fn bindings_text(bindings: &[(String, String)]) -> String {
    let parts: Vec<String> = bindings
        .iter()
        .filter(|(k, v)| k != v)
        .map(|(k, v)| format!("${k} := {v}$"))
        .collect();
    if parts.is_empty() {
        String::new()
    } else {
        format!(" with {}", parts.join(", "))
    }
}

fn also_names(ids: &[String]) -> String {
    let v = vocabulary();
    ids.iter()
        .map(|id| v.invariant(id).map_or_else(|| id.clone(), |d| d.canonical_name.clone()))
        .collect::<Vec<_>>()
        .join(", ")
}

fn substitution_latex(sub: &[(String, String)]) -> String {
    let parts: Vec<String> = sub.iter().map(|(k, v)| format!("{k} \\mapsto {v}")).collect();
    format!("\\sigma = \\{{{}\\}}", parts.join(",\\ "))
}

fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

fn tex_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '_' | '%' | '&' | '#' | '$' | '{' | '}' => {
                out.push('\\');
                out.push(c);
            }
            '\\' => out.push_str("\\textbackslash{}"),
            '~' => out.push_str("\\textasciitilde{}"),
            '^' => out.push_str("\\textasciicircum{}"),
            _ => out.push(c),
        }
    }
    out
}
