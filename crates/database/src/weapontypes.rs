//! Parser for `weapontypes.xml.xmb` — weapon type damage modifier tables.
//!
//! Each `<WeaponType>` defines damage multipliers against each armor/damage type.

use alloc::string::String;
use alloc::vec::Vec;

use crate::node_ext::{NodeExt, expect_root};

/// A single weapon type with its damage modifier table.
#[derive(Debug, Clone, Default)]
pub struct WeaponType {
    /// Weapon type name, e.g. `"AntiInfantry"`, `"ArmorPiercing"`.
    pub name: String,
    /// Death animation override, e.g. `"DeathByHeadshot"`, `"DeathByFire"`.
    pub death_animation: Option<String>,
    /// Damage modifiers against each damage type.
    pub damage_modifiers: Vec<DamageModifier>,
}

/// A damage modifier entry: multiplier against a specific damage/armor type.
#[derive(Debug, Clone, Default)]
pub struct DamageModifier {
    /// Target damage type, e.g. `"Light"`, `"Heavy"`, `"Building"`.
    pub damage_type: String,
    /// Attack rating.
    pub rating: Option<f32>,
    /// Damage multiplier value.
    pub modifier: f32,
    /// Reflect damage factor (for ram-type weapons).
    pub reflect_damage_factor: Option<f32>,
    /// Whether the target is bowlable (knocked around).
    pub bowlable: Option<bool>,
    /// Whether the target is rammable.
    pub rammable: Option<bool>,
}

/// Parse all weapon types from a `weapontypes.xml.xmb` document.
pub fn parse(doc: &xmb::Document) -> crate::Result<Vec<WeaponType>> {
    let root = expect_root(doc, "WeaponTypes")?;
    let mut types = Vec::new();

    for node in root.children_named("WeaponType") {
        types.push(parse_weapon_type(node));
    }

    Ok(types)
}

fn parse_weapon_type(node: &bdt::Node) -> WeaponType {
    let mut wt = WeaponType {
        name: node.child_text("Name").unwrap_or_default(),
        death_animation: node.child_text("DeathAnimation"),
        ..Default::default()
    };

    for dm in node.children_named("DamageModifier") {
        wt.damage_modifiers.push(DamageModifier {
            damage_type: dm.attr_str("type").unwrap_or_default(),
            rating: dm.attr_f32("rating"),
            modifier: dm.text_string().parse().unwrap_or(0.0),
            reflect_damage_factor: dm.attr_f32("reflectDamageFactor"),
            bowlable: dm.attr_bool("bowlable"),
            rammable: dm.attr_bool("rammable"),
        });
    }

    wt
}
