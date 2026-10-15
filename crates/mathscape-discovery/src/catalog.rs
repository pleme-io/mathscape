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

/// The full catalog of known mathematical properties, read from the
/// shared translation vocabulary in `mathscape_core::translation`.
pub fn catalog() -> Vec<KnownProperty> {
    mathscape_core::translation::vocabulary()
        .invariants
        .iter()
        .map(|i| KnownProperty {
            id: i.id.as_str(),
            name: i.canonical_name.as_str(),
            domain: i.math_domain.as_str(),
            latex: i.latex.as_str(),
            description: i.semantic_meaning.as_str(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn catalog_mirrors_the_translation_vocabulary() {
        let props = catalog();
        assert_eq!(
            props.len(),
            mathscape_core::translation::vocabulary().invariants.len()
        );
        assert!(props.iter().any(|p| p.id == "distributivity-mul-add"));
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
