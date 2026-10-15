//! Structured representation data for React UI consumption.
//! All types serialize to JSON and are designed for direct use by
//! frontend components.

use crate::matcher::Identification;
use serde::{Deserialize, Serialize};

/// A single discovery: a symbol that was identified as a known
/// mathematical property.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Discovery {
    /// Epoch when this symbol was discovered.
    pub epoch: i32,
    /// Symbol metadata.
    pub symbol: SymbolInfo,
    /// Identification result (if matched).
    pub identification: Option<Identification>,
    /// Metrics at the time of discovery.
    pub metrics: DiscoveryMetrics,
    /// Proof status.
    pub proof: Option<ProofInfo>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SymbolInfo {
    pub id: i32,
    pub name: String,
    pub arity: i32,
    /// S-expression of the LHS pattern.
    pub lhs: String,
    /// S-expression of the RHS replacement.
    pub rhs: String,
    pub generality: Option<f64>,
    pub irreducibility: Option<f64>,
    pub is_meta: bool,
    pub status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DiscoveryMetrics {
    pub compression_ratio_delta: Option<f64>,
    pub novelty_score: Option<f64>,
    pub generality: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProofInfo {
    pub status: String,
    pub proof_type: String,
    pub step_count: i32,
    pub lean_available: bool,
}

/// Timeline of all discoveries ordered by epoch.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DiscoveryTimeline {
    pub discoveries: Vec<Discovery>,
    pub total_epochs: i32,
    pub total_symbols: i32,
    pub identified_count: i32,
}

/// Expression tree visualization data — nodes and edges for rendering.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExpressionTree {
    pub nodes: Vec<TreeNode>,
    pub edges: Vec<TreeEdge>,
    pub metadata: TreeMetadata,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TreeNode {
    pub id: String,
    #[serde(rename = "type")]
    pub node_type: String,
    pub label: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TreeEdge {
    pub from: String,
    pub to: String,
    pub label: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TreeMetadata {
    pub depth: usize,
    pub node_count: usize,
    pub hash: String,
}

/// Symbol relationship graph — for interactive force-directed layout.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SymbolGraph {
    pub nodes: Vec<SymbolNode>,
    pub edges: Vec<SymbolEdge>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SymbolNode {
    pub id: i32,
    pub name: String,
    pub epoch: i32,
    pub identified_as: Option<String>,
    pub arity: i32,
    pub is_meta: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SymbolEdge {
    pub from: i32,
    pub to: i32,
    #[serde(rename = "type")]
    pub edge_type: String,
}

/// Proof chain visualization — ordered rewrite steps.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProofChain {
    pub symbol_name: String,
    pub steps: Vec<ProofStep>,
    pub total_steps: usize,
    pub status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProofStep {
    pub index: usize,
    pub rule_applied: String,
    /// S-expression before this rewrite step.
    pub before: String,
    /// S-expression after this rewrite step.
    pub after: String,
}

/// Epoch metrics for time-series charts.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EpochMetrics {
    pub epoch: i32,
    pub compression_ratio: f64,
    pub description_length: i32,
    pub novelty_total: f64,
    pub meta_compression: f64,
    pub library_size: i32,
    pub population_diversity: Option<f64>,
    pub phase: Option<String>,
    pub duration_ms: Option<i32>,
}

/// Build an `ExpressionTree` from a `Term` for visualization.
pub fn term_to_tree(term: &mathscape_core::Term, hash: &str) -> ExpressionTree {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut counter = 0;

    fn walk(
        term: &mathscape_core::Term,
        nodes: &mut Vec<TreeNode>,
        edges: &mut Vec<TreeEdge>,
        counter: &mut usize,
    ) -> String {
        let id = format!("n{counter}");
        *counter += 1;

        match term {
            mathscape_core::Term::Point(pid) => {
                nodes.push(TreeNode {
                    id: id.clone(),
                    node_type: "point".into(),
                    label: format!("p{pid}"),
                });
            }
            mathscape_core::Term::Number(v) => {
                nodes.push(TreeNode {
                    id: id.clone(),
                    node_type: "number".into(),
                    label: format!("{v}"),
                });
            }
            mathscape_core::Term::Var(v) => {
                nodes.push(TreeNode {
                    id: id.clone(),
                    node_type: "var".into(),
                    label: format!("?{v}"),
                });
            }
            mathscape_core::Term::Fn(params, body) => {
                nodes.push(TreeNode {
                    id: id.clone(),
                    node_type: "fn".into(),
                    label: format!(
                        "fn({})",
                        params.iter().map(|p| format!("?{p}")).collect::<Vec<_>>().join(", ")
                    ),
                });
                let body_id = walk(body, nodes, edges, counter);
                edges.push(TreeEdge {
                    from: id.clone(),
                    to: body_id,
                    label: "body".into(),
                });
            }
            mathscape_core::Term::Apply(func, args) => {
                nodes.push(TreeNode {
                    id: id.clone(),
                    node_type: "apply".into(),
                    label: "apply".into(),
                });
                let func_id = walk(func, nodes, edges, counter);
                edges.push(TreeEdge {
                    from: id.clone(),
                    to: func_id,
                    label: "func".into(),
                });
                for (i, arg) in args.iter().enumerate() {
                    let arg_id = walk(arg, nodes, edges, counter);
                    edges.push(TreeEdge {
                        from: id.clone(),
                        to: arg_id,
                        label: format!("arg{i}"),
                    });
                }
            }
            mathscape_core::Term::Symbol(sid, args) => {
                nodes.push(TreeNode {
                    id: id.clone(),
                    node_type: "symbol".into(),
                    label: format!("S{sid}"),
                });
                for (i, arg) in args.iter().enumerate() {
                    let arg_id = walk(arg, nodes, edges, counter);
                    edges.push(TreeEdge {
                        from: id.clone(),
                        to: arg_id,
                        label: format!("arg{i}"),
                    });
                }
            }
        }

        id
    }

    walk(term, &mut nodes, &mut edges, &mut counter);

    ExpressionTree {
        metadata: TreeMetadata {
            depth: term.depth(),
            node_count: term.size(),
            hash: hash.to_string(),
        },
        nodes,
        edges,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mathscape_core::test_helpers::{apply, nat, var};

    #[test]
    fn term_to_tree_correct_node_count() {
        // add(1, 2) => Apply + Var(add) + Number(1) + Number(2) = 4 nodes
        let term = apply(var(2), vec![nat(1), nat(2)]);
        let tree = term_to_tree(&term, "test-hash");
        assert_eq!(tree.nodes.len(), 4);
        assert_eq!(tree.metadata.node_count, 4);
    }

    #[test]
    fn term_to_tree_correct_edges_for_apply() {
        // add(1, 2) => Apply node has edges to: func, arg0, arg1 = 3 edges
        let term = apply(var(2), vec![nat(1), nat(2)]);
        let tree = term_to_tree(&term, "hash");
        assert_eq!(tree.edges.len(), 3);

        // Check edge labels
        let labels: Vec<&str> = tree.edges.iter().map(|e| e.label.as_str()).collect();
        assert!(labels.contains(&"func"), "should have func edge");
        assert!(labels.contains(&"arg0"), "should have arg0 edge");
        assert!(labels.contains(&"arg1"), "should have arg1 edge");

        // All edges originate from the root node (n0)
        assert!(
            tree.edges.iter().all(|e| e.from == "n0"),
            "all edges should originate from root apply node"
        );
    }

    #[test]
    fn representation_types_serialize_to_json() {
        // Discovery
        let discovery = Discovery {
            epoch: 42,
            symbol: SymbolInfo {
                id: 1,
                name: "S_001".into(),
                arity: 2,
                lhs: "(add ?100 ?101)".into(),
                rhs: "(add ?101 ?100)".into(),
                generality: Some(0.8),
                irreducibility: Some(1.0),
                is_meta: false,
                status: "active".into(),
            },
            identification: None,
            metrics: DiscoveryMetrics {
                compression_ratio_delta: Some(0.05),
                novelty_score: Some(0.8),
                generality: Some(0.8),
            },
            proof: None,
        };
        let json = serde_json::to_string(&discovery);
        assert!(json.is_ok(), "Discovery should serialize to JSON");

        // DiscoveryTimeline
        let timeline = DiscoveryTimeline {
            discoveries: vec![discovery],
            total_epochs: 42,
            total_symbols: 1,
            identified_count: 0,
        };
        let json = serde_json::to_string(&timeline);
        assert!(json.is_ok(), "DiscoveryTimeline should serialize to JSON");

        // ExpressionTree
        let tree = ExpressionTree {
            nodes: vec![TreeNode {
                id: "n0".into(),
                node_type: "number".into(),
                label: "42".into(),
            }],
            edges: vec![],
            metadata: TreeMetadata {
                depth: 1,
                node_count: 1,
                hash: "abc".into(),
            },
        };
        let json = serde_json::to_string(&tree);
        assert!(json.is_ok(), "ExpressionTree should serialize to JSON");

        // EpochMetrics
        let metrics = EpochMetrics {
            epoch: 10,
            compression_ratio: 0.3,
            description_length: 100,
            novelty_total: 0.5,
            meta_compression: 0.1,
            library_size: 5,
            population_diversity: Some(0.7),
            phase: Some("exploration".into()),
            duration_ms: Some(1200),
        };
        let json = serde_json::to_string(&metrics);
        assert!(json.is_ok(), "EpochMetrics should serialize to JSON");

        // ProofChain
        let chain = ProofChain {
            symbol_name: "S_001".into(),
            steps: vec![ProofStep {
                index: 0,
                rule_applied: "add-identity".into(),
                before: "(add 5 0)".into(),
                after: "5".into(),
            }],
            total_steps: 1,
            status: "verified".into(),
        };
        let json = serde_json::to_string(&chain);
        assert!(json.is_ok(), "ProofChain should serialize to JSON");

        // SymbolGraph
        let graph = SymbolGraph {
            nodes: vec![SymbolNode {
                id: 1,
                name: "S_001".into(),
                epoch: 10,
                identified_as: Some("commutativity".into()),
                arity: 2,
                is_meta: false,
            }],
            edges: vec![SymbolEdge {
                from: 1,
                to: 2,
                edge_type: "subsumes".into(),
            }],
        };
        let json = serde_json::to_string(&graph);
        assert!(json.is_ok(), "SymbolGraph should serialize to JSON");
    }
}
