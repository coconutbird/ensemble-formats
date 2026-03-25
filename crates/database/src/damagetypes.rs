//! Parser for `damagetypes.xml.xmb` — damage type definitions.
//!
//! Each `<DamageType>` defines an armor/damage category (Light, Heavy, Building, etc.).

use alloc::string::String;
use alloc::vec::Vec;

use crate::node_ext::{NodeExt, expect_root};

/// A single damage type definition from `damagetypes.xml`.
#[derive(Debug, Clone, Default)]
pub struct DamageType {
    /// Damage type name, e.g. `"Light"`, `"Heavy"`, `"Building"`.
    pub name: String,
    /// Whether this type has an attack rating.
    pub attack_rating: Option<bool>,
    /// Whether this is a base type.
    pub base_type: Option<bool>,
    /// Whether this is a shielded type.
    pub shielded: Option<bool>,
}

/// Parse all damage types from a `damagetypes.xml.xmb` document.
pub fn parse(doc: &xmb::Document) -> crate::Result<Vec<DamageType>> {
    let root = expect_root(doc, "DamageTypes")?;
    let mut types = Vec::new();

    for node in root.children_named("DamageType") {
        types.push(DamageType {
            name: node.text_string(),
            attack_rating: node.attr_bool("AttackRating"),
            base_type: node.attr_bool("BaseType"),
            shielded: node.attr_bool("Shielded"),
        });
    }

    Ok(types)
}
