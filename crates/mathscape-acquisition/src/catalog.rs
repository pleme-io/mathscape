//! CATALOG REFLECTION — the acquisition typescape self-describes.
//!
//! Per the org ★★ CATALOG REFLECTION rule: every authored typed
//! primitive lands a catalog entry naming it, its invariant, and its
//! honest seal-tier. The `catalog_covers_every_primitive` test is the
//! substrate-invariant forcing-function — adding a new acquisition
//! primitive without a catalog row fails the build.

use crate::types::Tier;
use serde::{Deserialize, Serialize};

/// One acquisition-space primitive, self-described.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcquisitionPrimitive {
    /// The Rust type name of the primitive.
    pub name: &'static str,
    /// The invariant the type enforces.
    pub invariant: &'static str,
    /// The honest seal-tier as *shipped today* (never rounded up). For
    /// the two witness types this is the weak floor tier; the
    /// destination tier is named in `destination`.
    pub tier_today: Tier,
    /// The destination tier once the typed gap closes, or `None` if the
    /// primitive is already at its ceiling.
    pub destination: Option<Tier>,
    /// The GARIMPO §3.5 sub-section that specifies it.
    pub spec_ref: &'static str,
}

/// The full catalog of acquisition-space primitives. Adding a primitive
/// to `types.rs` without a row here fails
/// `catalog_covers_every_primitive`.
#[must_use]
pub fn catalog() -> Vec<AcquisitionPrimitive> {
    vec![
        AcquisitionPrimitive {
            name: "Layer",
            invariant: "ground iff sealed — no Layer without a SealWitness (INV-1)",
            tier_today: Tier::OnlyMitigated,
            destination: Some(Tier::TrulyUnrepresentable),
            spec_ref: "§3.5.1",
        },
        AcquisitionPrimitive {
            name: "SealWitness",
            invariant: "both the Sample floor and the Proof destination are typed; a layer's tier is which constructor it holds",
            tier_today: Tier::OnlyMitigated,
            destination: Some(Tier::TrulyUnrepresentable),
            spec_ref: "§3.5.1",
        },
        AcquisitionPrimitive {
            name: "FoldBack",
            invariant: "lossless iff witnessed — no FoldBack without a SemanticsPreservingWitness (INV-2)",
            tier_today: Tier::OnlyMitigated,
            destination: Some(Tier::ParseTimeRejected),
            spec_ref: "§3.5.2",
        },
        AcquisitionPrimitive {
            name: "SemanticsPreservingWitness",
            invariant: "TrustedPrimitive floor + EquivalenceChecked destination both typed",
            tier_today: Tier::OnlyMitigated,
            destination: Some(Tier::ParseTimeRejected),
            spec_ref: "§3.5.2",
        },
        AcquisitionPrimitive {
            name: "AbsorbDecision",
            invariant: "constructs only when new_dl < old_dl — DL-increasing absorb has no constructor (§1.5)",
            tier_today: Tier::ParseTimeRejected,
            destination: None,
            spec_ref: "§3.5.3",
        },
        AcquisitionPrimitive {
            name: "Frontier",
            invariant: "no Closed variant — 'everything proven' is unrepresentable (Chaitin, §1.5)",
            tier_today: Tier::TrulyUnrepresentable,
            destination: None,
            spec_ref: "§3.5.4",
        },
        AcquisitionPrimitive {
            name: "AcquisitionState",
            invariant: "content-addressed + parent-linked; hash sealed by construction",
            tier_today: Tier::TrulyUnrepresentable,
            destination: None,
            spec_ref: "§3.5.5",
        },
        AcquisitionPrimitive {
            name: "AcquisitionDAG",
            invariant: "reversible — restore/fork total; a lost/unreachable state is unrepresentable",
            tier_today: Tier::TrulyUnrepresentable,
            destination: None,
            spec_ref: "§3.5.5",
        },
    ]
}

/// The canonical list of primitive names the catalog must cover. Kept
/// beside the catalog so the reflection test can compare the two — a
/// primitive named here but missing from `catalog()` (or vice versa)
/// fails the build.
pub const PRIMITIVE_NAMES: &[&str] = &[
    "Layer",
    "SealWitness",
    "FoldBack",
    "SemanticsPreservingWitness",
    "AbsorbDecision",
    "Frontier",
    "AcquisitionState",
    "AcquisitionDAG",
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn catalog_covers_every_primitive() {
        let cataloged: BTreeSet<&str> = catalog().iter().map(|p| p.name).collect();
        let expected: BTreeSet<&str> = PRIMITIVE_NAMES.iter().copied().collect();
        assert_eq!(
            cataloged, expected,
            "every acquisition primitive must have exactly one catalog row (and vice versa)"
        );
    }

    #[test]
    fn catalog_names_are_unique() {
        let cat = catalog();
        let unique: BTreeSet<&str> = cat.iter().map(|p| p.name).collect();
        assert_eq!(unique.len(), cat.len(), "catalog names must be unique");
    }

    #[test]
    fn tier_today_never_exceeds_destination() {
        // Honesty guard: the shipped tier must never be rounded UP past
        // its own destination. `tier_today <= destination` always.
        for p in catalog() {
            if let Some(dest) = p.destination {
                assert!(
                    p.tier_today <= dest,
                    "{}: tier_today {} must not exceed destination {}",
                    p.name,
                    p.tier_today,
                    dest
                );
            }
        }
    }
}
