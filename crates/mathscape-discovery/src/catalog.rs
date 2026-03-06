//! Known-math catalog: structural patterns for recognized mathematical properties.

use serde::{Deserialize, Serialize};

/// A recognized mathematical property or identity.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KnownProperty {
    /// Unique identifier for the property.
    pub id: &'static str,
    /// Human-readable name.
    pub name: &'static str,
    /// Mathematical domain (e.g., "algebra", "number theory").
    pub domain: &'static str,
    /// LaTeX representation of the property.
    pub latex: &'static str,
    /// Description for non-specialists.
    pub description: &'static str,
}

/// The full catalog of known mathematical properties.
pub fn catalog() -> Vec<KnownProperty> {
    vec![
        // === Algebraic Properties ===
        KnownProperty {
            id: "commutativity",
            name: "Commutativity",
            domain: "algebra",
            latex: "f(a, b) = f(b, a)",
            description: "A binary operation whose result is the same regardless of argument order.",
        },
        KnownProperty {
            id: "associativity",
            name: "Associativity",
            domain: "algebra",
            latex: "f(f(a, b), c) = f(a, f(b, c))",
            description: "A binary operation where grouping doesn't matter.",
        },
        KnownProperty {
            id: "identity_element",
            name: "Identity Element",
            domain: "algebra",
            latex: "f(a, e) = a",
            description: "An element that leaves others unchanged under a binary operation.",
        },
        KnownProperty {
            id: "inverse",
            name: "Inverse",
            domain: "algebra",
            latex: "f(a, g(a)) = e",
            description: "An element paired with its opposite yields the identity.",
        },
        KnownProperty {
            id: "distributivity",
            name: "Distributivity",
            domain: "algebra",
            latex: "f(a, g(b, c)) = g(f(a, b), f(a, c))",
            description: "One operation distributes over another.",
        },
        KnownProperty {
            id: "idempotence",
            name: "Idempotence",
            domain: "algebra",
            latex: "f(a, a) = a",
            description: "Applying an operation to the same element twice returns that element.",
        },
        KnownProperty {
            id: "absorption",
            name: "Absorption",
            domain: "lattice theory",
            latex: "f(a, g(a, b)) = a",
            description: "One operation absorbs another.",
        },
        KnownProperty {
            id: "involution",
            name: "Involution",
            domain: "algebra",
            latex: "f(f(a)) = a",
            description: "Applying a function twice returns the original value.",
        },
        // === Arithmetic Identities ===
        KnownProperty {
            id: "add_identity",
            name: "Additive Identity",
            domain: "arithmetic",
            latex: "a + 0 = a",
            description: "Zero is the identity element for addition.",
        },
        KnownProperty {
            id: "mul_identity",
            name: "Multiplicative Identity",
            domain: "arithmetic",
            latex: "a \\times 1 = a",
            description: "One is the identity element for multiplication.",
        },
        KnownProperty {
            id: "mul_zero",
            name: "Multiplication by Zero",
            domain: "arithmetic",
            latex: "a \\times 0 = 0",
            description: "Multiplying by zero always yields zero.",
        },
        KnownProperty {
            id: "add_commutativity",
            name: "Commutativity of Addition",
            domain: "arithmetic",
            latex: "a + b = b + a",
            description: "Addition is commutative.",
        },
        KnownProperty {
            id: "mul_commutativity",
            name: "Commutativity of Multiplication",
            domain: "arithmetic",
            latex: "a \\times b = b \\times a",
            description: "Multiplication is commutative.",
        },
        KnownProperty {
            id: "add_associativity",
            name: "Associativity of Addition",
            domain: "arithmetic",
            latex: "(a + b) + c = a + (b + c)",
            description: "Addition is associative.",
        },
        KnownProperty {
            id: "mul_associativity",
            name: "Associativity of Multiplication",
            domain: "arithmetic",
            latex: "(a \\times b) \\times c = a \\times (b \\times c)",
            description: "Multiplication is associative.",
        },
        KnownProperty {
            id: "distributivity_mul_add",
            name: "Distributivity of Multiplication over Addition",
            domain: "arithmetic",
            latex: "a \\times (b + c) = a \\times b + a \\times c",
            description: "Multiplication distributes over addition.",
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn catalog_returns_16_properties() {
        let props = catalog();
        assert_eq!(
            props.len(),
            16,
            "catalog should contain exactly 16 properties, got {}",
            props.len()
        );
    }

    #[test]
    fn all_ids_are_unique() {
        let props = catalog();
        let ids: HashSet<&str> = props.iter().map(|p| p.id).collect();
        assert_eq!(
            ids.len(),
            props.len(),
            "all property IDs should be unique; found {} unique out of {}",
            ids.len(),
            props.len()
        );
    }

    #[test]
    fn all_required_fields_are_non_empty() {
        for prop in catalog() {
            assert!(!prop.id.is_empty(), "id should not be empty");
            assert!(!prop.name.is_empty(), "name should not be empty for {}", prop.id);
            assert!(
                !prop.domain.is_empty(),
                "domain should not be empty for {}",
                prop.id
            );
            assert!(
                !prop.latex.is_empty(),
                "latex should not be empty for {}",
                prop.id
            );
            assert!(
                !prop.description.is_empty(),
                "description should not be empty for {}",
                prop.id
            );
        }
    }
}
