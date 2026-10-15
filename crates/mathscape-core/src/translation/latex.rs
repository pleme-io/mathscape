use super::vocabulary::{Render, Vocabulary};
use crate::eval::RewriteRule;
use crate::term::Term;
use crate::value::Value;
use std::collections::BTreeMap;

const ATOM: u8 = u8::MAX;
const APPLICATION: u8 = u8::MAX - 1;
const PREFIX: u8 = 100;

#[derive(Clone, Debug, Default)]
pub struct VarNames {
    names: BTreeMap<u32, String>,
}

impl VarNames {
    pub fn for_rule(rule: &RewriteRule) -> Self {
        let mut names = Self::default();
        names.collect(&rule.lhs);
        names.collect(&rule.rhs);
        names
    }

    pub fn for_term(term: &Term) -> Self {
        let mut names = Self::default();
        names.collect(term);
        names
    }

    pub fn collect(&mut self, t: &Term) {
        match t {
            Term::Var(id) if *id >= 100 => {
                let next = self.names.len();
                self.names.entry(*id).or_insert_with(|| letter(next));
            }
            Term::Fn(params, body) => {
                for p in params.iter().filter(|p| **p >= 100) {
                    let next = self.names.len();
                    self.names.entry(*p).or_insert_with(|| letter(next));
                }
                self.collect(body);
            }
            Term::Apply(h, args) => {
                self.collect(h);
                args.iter().for_each(|a| self.collect(a));
            }
            Term::Symbol(_, args) => args.iter().for_each(|a| self.collect(a)),
            _ => {}
        }
    }

    pub fn name(&self, id: u32) -> String {
        self.names.get(&id).cloned().unwrap_or_else(|| format!("v_{{{id}}}"))
    }
}

fn letter(i: usize) -> String {
    const LETTERS: &[u8] = b"abcdxyzuvwpqrst";
    match LETTERS.get(i) {
        Some(c) => (*c as char).to_string(),
        None => format!("x_{{{}}}", i - LETTERS.len() + 1),
    }
}

pub fn rule_latex(rule: &RewriteRule, vocabulary: &Vocabulary) -> String {
    let names = VarNames::for_rule(rule);
    format!(
        "{} = {}",
        term_latex_with(&rule.lhs, &names, vocabulary),
        term_latex_with(&rule.rhs, &names, vocabulary)
    )
}

pub fn term_latex(term: &Term, vocabulary: &Vocabulary) -> String {
    term_latex_with(term, &VarNames::for_term(term), vocabulary)
}

pub fn term_latex_with(term: &Term, names: &VarNames, vocabulary: &Vocabulary) -> String {
    render(term, names, vocabulary).0
}

pub fn operator_label(term: &Term, names: &VarNames, vocabulary: &Vocabulary) -> String {
    if let Term::Var(id) = term {
        if let Some(op) = vocabulary.operator(*id) {
            return match &op.render {
                Render::Constant(s) | Render::Function(s) | Render::Prefix(s) | Render::Postfix(s) => s.clone(),
                Render::Infix(s, _) if s.trim().is_empty() || s == "\\," => "\\cdot".to_owned(),
                Render::Infix(s, _) => s.clone(),
                Render::Bracket(open, sep, close) => format!("{open}\\cdot{sep}\\cdot{close}"),
                Render::Frac => "\\div".to_owned(),
            };
        }
    }
    term_latex_with(term, names, vocabulary)
}

fn render(t: &Term, names: &VarNames, vocabulary: &Vocabulary) -> (String, u8) {
    match t {
        Term::Var(id) if *id >= 100 => (names.name(*id), ATOM),
        Term::Var(id) => match vocabulary.operator(*id).map(|o| (&o.render, &o.name)) {
            Some((Render::Constant(s), _)) => (s.clone(), ATOM),
            Some((_, name)) => (format!("\\operatorname{{{}}}", escape_name(name)), ATOM),
            None => (format!("\\mathrm{{op}}_{{{id}}}"), ATOM),
        },
        Term::Point(p) => (format!("p_{{{p}}}"), ATOM),
        Term::Number(v) => value(v),
        Term::Symbol(id, args) if args.is_empty() => (symbol_head(*id), ATOM),
        Term::Symbol(id, args) => (call(&symbol_head(*id), args, names, vocabulary), APPLICATION),
        Term::Fn(params, body) => {
            let ps: Vec<String> = params.iter().map(|p| names.name(*p)).collect();
            (
                format!("\\lambda {}.\\, {}", ps.join("\\, "), render(body, names, vocabulary).0),
                0,
            )
        }
        Term::Apply(head, args) => apply(head, args, names, vocabulary),
    }
}

fn apply(head: &Term, args: &[Term], names: &VarNames, vocabulary: &Vocabulary) -> (String, u8) {
    let op = match head {
        Term::Var(id) if *id < 100 => vocabulary.operator(*id),
        _ => None,
    };
    let Some(op) = op else {
        let head_text = render(head, names, vocabulary).0;
        return (call(&head_text, args, names, vocabulary), APPLICATION);
    };
    let child = |t: &Term| render(t, names, vocabulary);
    match (&op.render, args) {
        (Render::Constant(s), []) => (s.clone(), ATOM),
        (Render::Infix(symbol, precedence), [a, b]) => {
            let (l, lp) = child(a);
            let (r, rp) = child(b);
            let sep = if symbol == "\\," { "\\,".to_owned() } else { format!(" {symbol} ") };
            (
                format!(
                    "{}{sep}{}",
                    wrap(l, lp <= *precedence),
                    wrap(r, rp <= *precedence || rp == PREFIX)
                ),
                *precedence,
            )
        }
        (Render::Prefix(symbol), [a]) => {
            let (x, xp) = child(a);
            (format!("{symbol}{}", wrap(x, xp < APPLICATION)), PREFIX)
        }
        (Render::Postfix(symbol), [a]) => {
            let (x, xp) = child(a);
            (format!("{}{symbol}", wrap(x, xp < ATOM)), ATOM)
        }
        (Render::Bracket(open, sep, close), _) => {
            let inner: Vec<String> = args.iter().map(|a| child(a).0).collect();
            (format!("{open}{}{close}", inner.join(sep)), ATOM)
        }
        (Render::Frac, [a, b]) => (format!("\\frac{{{}}}{{{}}}", child(a).0, child(b).0), ATOM),
        (Render::Function(name), _) => (call(name, args, names, vocabulary), APPLICATION),
        _ => (
            call(&format!("\\operatorname{{{}}}", escape_name(&op.name)), args, names, vocabulary),
            APPLICATION,
        ),
    }
}

fn call(head: &str, args: &[Term], names: &VarNames, vocabulary: &Vocabulary) -> String {
    let inner: Vec<String> = args.iter().map(|a| render(a, names, vocabulary).0).collect();
    format!("{head}({})", inner.join(", "))
}

fn wrap(s: String, parens: bool) -> String {
    if parens { format!("({s})") } else { s }
}

fn symbol_head(id: u32) -> String {
    format!("\\mathsf{{S}}_{{{id}}}")
}

fn escape_name(name: &str) -> String {
    name.replace('_', "\\_")
}

fn value(v: &Value) -> (String, u8) {
    match v {
        Value::Nat(n) => (n.to_string(), ATOM),
        Value::Int(n) if *n < 0 => (n.to_string(), PREFIX),
        Value::Int(n) => (n.to_string(), ATOM),
        Value::Float(_) => {
            let f = v.as_float().unwrap_or_default();
            let s = if f.fract() == 0.0 { format!("{f:.1}") } else { f.to_string() };
            (s, if f < 0.0 { PREFIX } else { ATOM })
        }
        Value::Tensor { shape, data } => (matrix(shape, &data.iter().map(i64::to_string).collect::<Vec<_>>()), ATOM),
        other => match other.as_float_tensor() {
            Some((shape, data)) => (
                matrix(shape, &data.iter().map(f64::to_string).collect::<Vec<_>>()),
                ATOM,
            ),
            None => (format!("\\texttt{{{other}}}"), ATOM),
        },
    }
}

fn matrix(shape: &[usize], cells: &[String]) -> String {
    match shape {
        [_] => format!("\\begin{{bmatrix}} {} \\end{{bmatrix}}", cells.join(" & ")),
        [_, cols] if *cols > 0 => {
            let rows: Vec<String> = cells.chunks(*cols).map(|r| r.join(" & ")).collect();
            format!("\\begin{{bmatrix}} {} \\end{{bmatrix}}", rows.join(" \\\\ "))
        }
        _ => {
            let dims: Vec<String> = shape.iter().map(usize::to_string).collect();
            format!("T_{{{}}}", dims.join("\\times "))
        }
    }
}
