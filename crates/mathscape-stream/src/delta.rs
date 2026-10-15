use mathscape_core::eval::RewriteRule;
use mathscape_core::hash::TermRef;
use mathscape_core::mathscape_map::MapEvent;
use mathscape_proof::map_event_lisp::{map_event_from_sexp, map_event_to_sexp};
use std::collections::BTreeMap;
use tatara_lisp::ast::{Atom, Sexp};

#[derive(Debug, Clone, PartialEq)]
pub struct Discovery {
    pub epoch: u64,
    pub seed: u64,
    pub candidates: Vec<RewriteRule>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EpochDelta {
    pub epoch: u64,
    pub seed: u64,
    pub parent_root: TermRef,
    pub new_root: TermRef,
    pub timestamp: u64,
    pub events: Vec<MapEvent>,
}

impl EpochDelta {
    pub fn certified(&self) -> impl Iterator<Item = &RewriteRule> {
        self.events.iter().filter_map(|e| match e {
            MapEvent::RuleCertified { rule, .. } => Some(rule),
            _ => None,
        })
    }

    pub fn rejected(&self) -> impl Iterator<Item = &RewriteRule> {
        self.events.iter().filter_map(|e| match e {
            MapEvent::RuleRejectedAtCertification { rule, .. } => Some(rule),
            _ => None,
        })
    }

    #[must_use]
    pub fn certified_count(&self) -> usize {
        self.certified().count()
    }

    #[must_use]
    pub fn to_sexp(&self) -> Sexp {
        Sexp::List(vec![
            Sexp::symbol("epoch-delta"),
            Sexp::keyword("epoch-id"),
            Sexp::string(self.epoch.to_string()),
            Sexp::keyword("seed"),
            Sexp::string(self.seed.to_string()),
            Sexp::keyword("parent-merkle-root"),
            Sexp::string(self.parent_root.to_hex()),
            Sexp::keyword("new-merkle-root"),
            Sexp::string(self.new_root.to_hex()),
            Sexp::keyword("timestamp"),
            Sexp::string(self.timestamp.to_string()),
            Sexp::keyword("events"),
            Sexp::List(self.events.iter().map(map_event_to_sexp).collect()),
        ])
    }

    #[must_use]
    pub fn to_line(&self) -> String {
        self.to_sexp().to_string()
    }

    #[must_use]
    pub fn from_sexp(sexp: &Sexp) -> Option<Self> {
        let items = sexp.as_list()?;
        if items.first()?.as_symbol()? != "epoch-delta" {
            return None;
        }
        let mut fields: BTreeMap<&str, &Sexp> = BTreeMap::new();
        for pair in items[1..].chunks(2) {
            let [Sexp::Atom(Atom::Keyword(k)), v] = pair else {
                return None;
            };
            fields.insert(k.as_str(), v);
        }
        let text = |k: &str| match fields.get(k) {
            Some(Sexp::Atom(Atom::Str(s))) => Some(s.as_str()),
            _ => None,
        };
        let number = |k: &str| text(k).and_then(|s| s.parse::<u64>().ok());
        let root = |k: &str| text(k).and_then(TermRef::from_hex);
        let events = match fields.get("events")? {
            Sexp::Nil => Vec::new(),
            Sexp::List(xs) => xs.iter().map(map_event_from_sexp).collect::<Option<Vec<_>>>()?,
            _ => return None,
        };
        Some(Self {
            epoch: number("epoch-id")?,
            seed: number("seed")?,
            parent_root: root("parent-merkle-root")?,
            new_root: root("new-merkle-root")?,
            timestamp: number("timestamp")?,
            events,
        })
    }

    #[must_use]
    pub fn from_line(line: &str) -> Option<Self> {
        let forms = tatara_lisp::reader::read(line).ok()?;
        match forms.as_slice() {
            [one] => Self::from_sexp(one),
            _ => None,
        }
    }
}
