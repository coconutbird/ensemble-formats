//! Parser for `objects.xml.xmb` — the master proto-object database.
//!
//! Each `<Object>` element describes a game entity: unit, building, projectile, etc.

use alloc::string::String;
use alloc::vec::Vec;
use serde::Deserialize;

use crate::node_ext::expect_root;

/// A single proto-object definition from `objects.xml`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProtoObject {
    /// Object name (unique key), e.g. `"unsc_veh_warthog_01"`.
    #[serde(rename = "@name", default)]
    pub name: String,
    /// Numeric ID (the `id` attribute, often 0).
    #[serde(rename = "@id")]
    pub id: Option<i32>,
    /// Database ID (the `dbid` attribute).
    #[serde(rename = "@dbid")]
    pub dbid: Option<i32>,
    /// Object class: `"Unit"`, `"Squad"`, `"Building"`, `"Object"`, etc.
    #[serde(rename = "ObjectClass")]
    pub object_class: Option<String>,
    /// Visual file path (relative, no `art\` prefix, no extension).
    #[serde(rename = "Visual")]
    pub visual: Option<String>,
    /// Physics info bare name (resolves to `physics\{name}.*`).
    #[serde(rename = "PhysicsInfo")]
    pub physics_info: Option<String>,
    /// Physics replacement info bare name.
    #[serde(rename = "PhysicsReplacementInfo")]
    pub physics_replacement_info: Option<String>,
    /// Tactics file base name (resolves to `data\tactics\{name}.xmb`).
    #[serde(rename = "Tactics")]
    pub tactics: Option<String>,
    /// Hitpoints.
    #[serde(rename = "Hitpoints")]
    pub hitpoints: Option<f32>,
    /// Movement type: `"Land"`, `"Air"`, etc.
    #[serde(rename = "MovementType")]
    pub movement_type: Option<String>,
    /// Velocity.
    #[serde(rename = "Velocity")]
    pub velocity: Option<f32>,
    /// Turn rate.
    #[serde(rename = "TurnRate")]
    pub turn_rate: Option<f32>,
    /// Line of sight radius.
    #[serde(rename = "LOS")]
    pub los: Option<f32>,
    /// Portrait icon path.
    #[serde(rename = "PortraitIcon")]
    pub portrait_icon: Option<String>,
    /// Display name string ID.
    #[serde(rename = "DisplayNameID")]
    pub display_name_id: Option<i32>,
    /// Rollover text string ID.
    #[serde(rename = "RolloverTextID")]
    pub rollover_text_id: Option<i32>,
    /// Bounty value.
    #[serde(rename = "Bounty")]
    pub bounty: Option<i32>,
    /// Flags (e.g. `"ForceToGaiaPlayer"`, `"Invulnerable"`, `"NoRender"`).
    #[serde(rename = "Flag", default)]
    pub flags: Vec<String>,
    /// Object types (e.g. `"CreepDifficultyMarker"`).
    #[serde(rename = "ObjectType", default)]
    pub object_types: Vec<String>,
    /// Hardpoints.
    #[serde(rename = "Hardpoint", default)]
    pub hardpoints: Vec<Hardpoint>,
    /// Veterancy levels.
    #[serde(rename = "Veterancy", default)]
    pub veterancy: Vec<VeterancyLevel>,
}

/// A hardpoint on a proto-object (turret mount point).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Hardpoint {
    #[serde(rename = "@name", default)]
    pub name: String,
    #[serde(rename = "@yawrate")]
    pub yaw_rate: Option<f32>,
    #[serde(rename = "@pitchrate")]
    pub pitch_rate: Option<f32>,
    #[serde(rename = "@pitchMinAngle")]
    pub pitch_min_angle: Option<f32>,
    #[serde(rename = "@pitchMaxAngle")]
    pub pitch_max_angle: Option<f32>,
    #[serde(rename = "@yawattachment")]
    pub yaw_attachment: Option<String>,
    #[serde(rename = "@pitchattachment")]
    pub pitch_attachment: Option<String>,
    #[serde(rename = "@autocenter")]
    pub autocenter: Option<bool>,
}

/// A veterancy level entry.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct VeterancyLevel {
    #[serde(rename = "@Level", default)]
    pub level: i32,
    #[serde(rename = "@XP")]
    pub xp: Option<f32>,
    #[serde(rename = "@Damage")]
    pub damage: Option<f32>,
    #[serde(rename = "@Velocity")]
    pub velocity: Option<f32>,
    #[serde(rename = "@Accuracy")]
    pub accuracy: Option<f32>,
    #[serde(rename = "@WorkRate")]
    pub work_rate: Option<f32>,
    #[serde(rename = "@WeaponRange")]
    pub weapon_range: Option<f32>,
    #[serde(rename = "@DamageTaken")]
    pub damage_taken: Option<f32>,
}

/// Parse all proto-objects from an `objects.xml.xmb` document.
pub fn parse(doc: &xmb::Document) -> crate::Result<Vec<ProtoObject>> {
    let root = expect_root(doc, "Objects")?;
    let objects: Vec<ProtoObject> = root
        .children
        .iter()
        .filter(|c| c.name == "Object")
        .map(bdt_serde::from_node)
        .collect::<Result<_, _>>()?;
    Ok(objects)
}
