//! Parser for `.tactics.xmb` — combat tactics definitions.
//!
//! Tactics define the weapons, actions, and target priorities for a unit.

use alloc::string::String;
use alloc::vec::Vec;

use crate::node_ext::{NodeExt, expect_root};

/// A complete tactics definition from a `.tactics.xmb` file.
#[derive(Debug, Clone, Default)]
pub struct TacticData {
    /// Weapons available to this unit.
    pub weapons: Vec<Weapon>,
    /// Actions the unit can perform.
    pub actions: Vec<Action>,
}

/// A weapon definition.
#[derive(Debug, Clone, Default)]
pub struct Weapon {
    /// Weapon name, e.g. `"Machinegun"`, `"GaussCannon"`.
    pub name: String,
    /// Attack rate (seconds between attacks).
    pub attack_rate: Option<f32>,
    /// Damage per second.
    pub dps: Option<f32>,
    /// Weapon type classification.
    pub weapon_type: Option<String>,
    /// Projectile proto-object name.
    pub projectile: Option<String>,
    /// Maximum range.
    pub max_range: Option<f32>,
    /// Accuracy (0.0–1.0).
    pub accuracy: Option<f32>,
    /// Maximum deviation.
    pub max_deviation: Option<f32>,
    /// Moving accuracy.
    pub moving_accuracy: Option<f32>,
    /// Moving max deviation.
    pub moving_max_deviation: Option<f32>,
    /// Hardpoint this weapon is mounted on.
    pub hardpoint: Option<String>,
    /// AOE radius.
    pub aoe_radius: Option<f32>,
    /// Target priorities.
    pub target_priorities: Vec<TargetPriority>,
    /// Whether this weapon is small-arms deflectable.
    pub small_arms_deflectable: bool,
    /// Whether this weapon is dodgeable.
    pub dodgeable: bool,
}

/// Target priority for a weapon.
#[derive(Debug, Clone, Default)]
pub struct TargetPriority {
    /// Target type: `"Infantry"`, `"Aircraft"`, etc.
    pub target_type: String,
    /// Priority value (higher = preferred).
    pub priority: f32,
}

/// An action a unit can perform.
#[derive(Debug, Clone, Default)]
pub struct Action {
    /// Action name.
    pub name: String,
    /// Action type.
    pub action_type: Option<String>,
    /// Weapon name used for this action.
    pub weapon: Option<String>,
    /// Duration.
    pub duration: Option<f32>,
    /// Whether this is the default action.
    pub default: bool,
}

/// Parse tactics from a `.tactics.xmb` document.
pub fn parse(doc: &xmb::Document) -> crate::Result<TacticData> {
    let root = expect_root(doc, "TacticData")?;
    let mut data = TacticData::default();

    for weapon_node in root.children_named("Weapon") {
        data.weapons.push(parse_weapon(weapon_node));
    }
    for action_node in root.children_named("Action") {
        data.actions.push(parse_action(action_node));
    }

    Ok(data)
}

fn parse_weapon(node: &bdt::Node) -> Weapon {
    let mut weapon = Weapon {
        name: node.child_text("Name").unwrap_or_default(),
        attack_rate: node.child_f32("AttackRate"),
        dps: node.child_f32("DamagePerSecond"),
        weapon_type: node.child_text("WeaponType"),
        projectile: node.child_text("Projectile"),
        max_range: node.child_f32("MaxRange"),
        accuracy: node.child_f32("Accuracy"),
        max_deviation: node.child_f32("MaxDeviation"),
        moving_accuracy: node.child_f32("MovingAccuracy"),
        moving_max_deviation: node.child_f32("MovingMaxDeviation"),
        hardpoint: node.child_text("Hardpoint"),
        aoe_radius: node.child_f32("AOERadius"),
        small_arms_deflectable: node.child("SmallArmsDeflectable").is_some(),
        dodgeable: node.child("Dodgeable").is_some(),
        ..Default::default()
    };

    for tp in node.children_named("TargetPriority") {
        weapon.target_priorities.push(TargetPriority {
            target_type: tp.attr_str("type").unwrap_or_default(),
            priority: tp.text.as_float().unwrap_or(0.0),
        });
    }

    weapon
}

fn parse_action(node: &bdt::Node) -> Action {
    Action {
        name: node.child_text("Name").unwrap_or_default(),
        action_type: node.child_text("ActionType"),
        weapon: node.child_text("Weapon"),
        duration: node.child_f32("Duration"),
        default: node.child("Default").is_some(),
    }
}
