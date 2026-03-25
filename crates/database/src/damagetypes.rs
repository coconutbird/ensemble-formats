//! Parser for `damagetypes.xml.xmb` — damage type definitions.
//!
//! Each `<DamageType>` defines an armor/damage category (Light, Heavy, Building, etc.).

use alloc::string::String;
use alloc::vec::Vec;
use serde::Deserialize;

use crate::node_ext::expect_root;

/// A single damage type definition from `damagetypes.xml`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DamageType {
    /// Damage type name, e.g. `"Light"`, `"Heavy"`, `"Building"`.
    #[serde(rename = "$text", default)]
    pub name: String,
    /// Whether this type has an attack rating.
    #[serde(rename = "@AttackRating")]
    pub attack_rating: Option<bool>,
    /// Whether this is a base type.
    #[serde(rename = "@BaseType")]
    pub base_type: Option<bool>,
    /// Whether this is a shielded type.
    #[serde(rename = "@Shielded")]
    pub shielded: Option<bool>,
}

/// Parse all damage types from a `damagetypes.xml.xmb` document.
pub fn parse(doc: &xmb::Document) -> crate::Result<Vec<DamageType>> {
    let root = expect_root(doc, "DamageTypes")?;
    let types: Vec<DamageType> = root
        .children
        .iter()
        .filter(|c| c.name == "DamageType")
        .map(bdt_serde::from_node)
        .collect::<Result<_, _>>()?;
    Ok(types)
}
