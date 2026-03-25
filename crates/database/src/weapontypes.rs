//! Parser for `weapontypes.xml.xmb` — weapon type damage modifier tables.
//!
//! Each `<WeaponType>` defines damage multipliers against each armor/damage type.

use alloc::string::String;
use alloc::vec::Vec;
use serde::Deserialize;

use crate::node_ext::expect_root;

/// A single weapon type with its damage modifier table.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WeaponType {
    /// Weapon type name, e.g. `"AntiInfantry"`, `"ArmorPiercing"`.
    #[serde(rename = "Name", default)]
    pub name: String,
    /// Death animation override, e.g. `"DeathByHeadshot"`, `"DeathByFire"`.
    #[serde(rename = "DeathAnimation")]
    pub death_animation: Option<String>,
    /// Damage modifiers against each damage type.
    #[serde(rename = "DamageModifier", default)]
    pub damage_modifiers: Vec<DamageModifier>,
}

/// A damage modifier entry: multiplier against a specific damage/armor type.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DamageModifier {
    /// Target damage type, e.g. `"Light"`, `"Heavy"`, `"Building"`.
    #[serde(rename = "@type", default)]
    pub damage_type: String,
    /// Attack rating.
    #[serde(rename = "@rating")]
    pub rating: Option<f32>,
    /// Damage multiplier value.
    #[serde(rename = "$text", default)]
    pub modifier: f32,
    /// Reflect damage factor (for ram-type weapons).
    #[serde(rename = "@reflectDamageFactor")]
    pub reflect_damage_factor: Option<f32>,
    /// Whether the target is bowlable (knocked around).
    #[serde(rename = "@bowlable")]
    pub bowlable: Option<bool>,
    /// Whether the target is rammable.
    #[serde(rename = "@rammable")]
    pub rammable: Option<bool>,
}

/// Parse all weapon types from a `weapontypes.xml.xmb` document.
pub fn parse(doc: &xmb::Document) -> crate::Result<Vec<WeaponType>> {
    let root = expect_root(doc, "WeaponTypes")?;
    let types: Vec<WeaponType> = root
        .children
        .iter()
        .filter(|c| c.name == "WeaponType")
        .map(bdt_serde::from_node)
        .collect::<Result<_, _>>()?;
    Ok(types)
}
