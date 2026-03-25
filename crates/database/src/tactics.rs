//! Parser for `.tactics.xmb` — combat tactics definitions.
//!
//! Tactics define the weapons, actions, and target priorities for a unit.

use alloc::string::String;
use alloc::vec::Vec;
use serde::Deserialize;

use crate::node_ext::expect_root;

/// A complete tactics definition from a `.tactics.xmb` file.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TacticData {
    /// Weapons available to this unit.
    #[serde(rename = "Weapon", default)]
    pub weapons: Vec<Weapon>,
    /// Actions the unit can perform.
    #[serde(rename = "Action", default)]
    pub actions: Vec<Action>,
}

/// A weapon definition.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Weapon {
    /// Weapon name, e.g. `"Machinegun"`, `"GaussCannon"`.
    #[serde(rename = "Name", default)]
    pub name: String,
    /// Attack rate (seconds between attacks).
    #[serde(rename = "AttackRate")]
    pub attack_rate: Option<f32>,
    /// Damage per second.
    #[serde(rename = "DamagePerSecond")]
    pub dps: Option<f32>,
    /// Weapon type classification.
    #[serde(rename = "WeaponType")]
    pub weapon_type: Option<String>,
    /// Projectile proto-object name.
    #[serde(rename = "Projectile")]
    pub projectile: Option<String>,
    /// Maximum range.
    #[serde(rename = "MaxRange")]
    pub max_range: Option<f32>,
    /// Accuracy (0.0–1.0).
    #[serde(rename = "Accuracy")]
    pub accuracy: Option<f32>,
    /// Maximum deviation.
    #[serde(rename = "MaxDeviation")]
    pub max_deviation: Option<f32>,
    /// Moving accuracy.
    #[serde(rename = "MovingAccuracy")]
    pub moving_accuracy: Option<f32>,
    /// Moving max deviation.
    #[serde(rename = "MovingMaxDeviation")]
    pub moving_max_deviation: Option<f32>,
    /// Hardpoint this weapon is mounted on.
    #[serde(rename = "Hardpoint")]
    pub hardpoint: Option<String>,
    /// AOE radius.
    #[serde(rename = "AOERadius")]
    pub aoe_radius: Option<f32>,
    /// Target priorities.
    #[serde(rename = "TargetPriority", default)]
    pub target_priorities: Vec<TargetPriority>,
    /// Presence-based: element exists = true.
    #[serde(rename = "SmallArmsDeflectable")]
    pub small_arms_deflectable: Option<String>,
    /// Presence-based: element exists = true.
    #[serde(rename = "Dodgeable")]
    pub dodgeable: Option<String>,
}

/// Target priority for a weapon.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TargetPriority {
    /// Target type: `"Infantry"`, `"Aircraft"`, etc.
    #[serde(rename = "@type", default)]
    pub target_type: String,
    /// Priority value (higher = preferred).
    #[serde(rename = "$text", default)]
    pub priority: f32,
}

/// An action a unit can perform.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Action {
    /// Action name.
    #[serde(rename = "Name", default)]
    pub name: String,
    /// Action type.
    #[serde(rename = "ActionType")]
    pub action_type: Option<String>,
    /// Weapon name used for this action.
    #[serde(rename = "Weapon")]
    pub weapon: Option<String>,
    /// Duration.
    #[serde(rename = "Duration")]
    pub duration: Option<f32>,
    /// Presence-based: element exists = true.
    #[serde(rename = "Default")]
    pub default: Option<String>,
}

/// Parse tactics from a `.tactics.xmb` document.
pub fn parse(doc: &xmb::Document) -> crate::Result<TacticData> {
    let root = expect_root(doc, "TacticData")?;
    let data: TacticData = bdt_serde::from_node(root)?;
    Ok(data)
}
