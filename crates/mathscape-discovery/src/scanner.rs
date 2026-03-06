//! Epoch scanner: iterates over epoch data, extracts discoveries,
//! runs identification against the known-math catalog.

use crate::matcher;
use crate::representation::{Discovery, DiscoveryMetrics, DiscoveryTimeline, SymbolInfo};
use mathscape_core::eval::RewriteRule;

/// A raw symbol record from the database, before identification.
#[derive(Clone, Debug)]
pub struct SymbolRecord {
    pub symbol_id: i32,
    pub name: String,
    pub epoch_discovered: i32,
    pub arity: i32,
    pub generality: Option<f64>,
    pub irreducibility: Option<f64>,
    pub is_meta: bool,
    pub status: String,
    /// S-expression of the LHS pattern.
    pub lhs_sexpr: String,
    /// S-expression of the RHS replacement.
    pub rhs_sexpr: String,
}

/// Scan a list of symbol records, identify each against the catalog,
/// and produce a discovery timeline.
pub fn scan_symbols(symbols: &[SymbolRecord]) -> DiscoveryTimeline {
    let mut discoveries = Vec::new();

    for sym in symbols {
        // Parse LHS and RHS as Terms for structural matching
        let lhs_term = mathscape_core::parse::parse(&sym.lhs_sexpr).ok();
        let rhs_term = mathscape_core::parse::parse(&sym.rhs_sexpr).ok();

        let identification = match (&lhs_term, &rhs_term) {
            (Some(lhs), Some(rhs)) => {
                let rule = RewriteRule {
                    name: sym.name.clone(),
                    lhs: lhs.clone(),
                    rhs: rhs.clone(),
                };
                // Take the highest-confidence identification
                matcher::identify(&rule).into_iter().max_by(|a, b| {
                    a.confidence
                        .partial_cmp(&b.confidence)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
            }
            _ => None,
        };

        discoveries.push(Discovery {
            epoch: sym.epoch_discovered,
            symbol: SymbolInfo {
                id: sym.symbol_id,
                name: sym.name.clone(),
                arity: sym.arity,
                lhs: sym.lhs_sexpr.clone(),
                rhs: sym.rhs_sexpr.clone(),
                generality: sym.generality,
                irreducibility: sym.irreducibility,
                is_meta: sym.is_meta,
                status: sym.status.clone(),
            },
            identification,
            metrics: DiscoveryMetrics {
                compression_ratio_delta: None, // filled from epoch data
                novelty_score: sym.generality, // approximation
                generality: sym.generality,
            },
            proof: None, // filled from proof data
        });
    }

    let total = discoveries.len() as i32;
    let identified = discoveries
        .iter()
        .filter(|d| d.identification.is_some())
        .count() as i32;

    let max_epoch = discoveries.iter().map(|d| d.epoch).max().unwrap_or(0);

    DiscoveryTimeline {
        discoveries,
        total_epochs: max_epoch,
        total_symbols: total,
        identified_count: identified,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_identifies_commutativity() {
        let symbols = vec![SymbolRecord {
            symbol_id: 1,
            name: "S_001".into(),
            epoch_discovered: 120,
            arity: 2,
            generality: Some(0.8),
            irreducibility: Some(1.0),
            is_meta: false,
            status: "active".into(),
            lhs_sexpr: "(add ?100 ?101)".into(),
            rhs_sexpr: "(add ?101 ?100)".into(),
        }];

        let timeline = scan_symbols(&symbols);
        assert_eq!(timeline.total_symbols, 1);
        assert_eq!(timeline.identified_count, 1);
        assert_eq!(
            timeline.discoveries[0]
                .identification
                .as_ref()
                .unwrap()
                .property_id,
            "add_commutativity"
        );
    }

    #[test]
    fn scan_identifies_identity() {
        let symbols = vec![SymbolRecord {
            symbol_id: 2,
            name: "S_002".into(),
            epoch_discovered: 50,
            arity: 2,
            generality: Some(0.9),
            irreducibility: Some(1.0),
            is_meta: false,
            status: "active".into(),
            lhs_sexpr: "(add ?100 0)".into(),
            rhs_sexpr: "?100".into(),
        }];

        let timeline = scan_symbols(&symbols);
        assert_eq!(timeline.identified_count, 1);
        assert_eq!(
            timeline.discoveries[0]
                .identification
                .as_ref()
                .unwrap()
                .property_id,
            "add_identity"
        );
    }

    #[test]
    fn empty_symbols_produces_empty_timeline() {
        let timeline = scan_symbols(&[]);
        assert_eq!(timeline.total_symbols, 0);
        assert_eq!(timeline.identified_count, 0);
        assert_eq!(timeline.total_epochs, 0);
        assert!(timeline.discoveries.is_empty());
    }

    #[test]
    fn unparseable_sexpr_produces_no_identification() {
        let symbols = vec![SymbolRecord {
            symbol_id: 99,
            name: "S_099".into(),
            epoch_discovered: 10,
            arity: 2,
            generality: Some(0.5),
            irreducibility: Some(1.0),
            is_meta: false,
            status: "active".into(),
            lhs_sexpr: "((( not valid".into(),
            rhs_sexpr: ")))".into(),
        }];

        let timeline = scan_symbols(&symbols);
        assert_eq!(timeline.total_symbols, 1);
        assert_eq!(timeline.identified_count, 0);
        assert!(timeline.discoveries[0].identification.is_none());
    }

    #[test]
    fn multiple_symbols_some_identified_some_not() {
        let symbols = vec![
            // This one should be identified as add_commutativity
            SymbolRecord {
                symbol_id: 1,
                name: "S_001".into(),
                epoch_discovered: 100,
                arity: 2,
                generality: Some(0.8),
                irreducibility: Some(1.0),
                is_meta: false,
                status: "active".into(),
                lhs_sexpr: "(add ?100 ?101)".into(),
                rhs_sexpr: "(add ?101 ?100)".into(),
            },
            // This one won't match any known property
            SymbolRecord {
                symbol_id: 2,
                name: "S_002".into(),
                epoch_discovered: 200,
                arity: 2,
                generality: Some(0.3),
                irreducibility: Some(1.0),
                is_meta: false,
                status: "active".into(),
                lhs_sexpr: "(add ?100 ?101)".into(),
                rhs_sexpr: "(mul ?100 ?101)".into(),
            },
            // This one should be identified as add_identity
            SymbolRecord {
                symbol_id: 3,
                name: "S_003".into(),
                epoch_discovered: 300,
                arity: 2,
                generality: Some(0.9),
                irreducibility: Some(1.0),
                is_meta: false,
                status: "active".into(),
                lhs_sexpr: "(add ?100 0)".into(),
                rhs_sexpr: "?100".into(),
            },
        ];

        let timeline = scan_symbols(&symbols);
        assert_eq!(timeline.total_symbols, 3);
        assert_eq!(timeline.identified_count, 2);
        assert_eq!(timeline.total_epochs, 300);

        // First: identified
        assert!(timeline.discoveries[0].identification.is_some());
        assert_eq!(
            timeline.discoveries[0]
                .identification
                .as_ref()
                .unwrap()
                .property_id,
            "add_commutativity"
        );
        // Second: not identified
        assert!(timeline.discoveries[1].identification.is_none());
        // Third: identified
        assert!(timeline.discoveries[2].identification.is_some());
        assert_eq!(
            timeline.discoveries[2]
                .identification
                .as_ref()
                .unwrap()
                .property_id,
            "add_identity"
        );
    }
}
