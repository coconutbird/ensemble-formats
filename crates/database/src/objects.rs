//! Parser for `objects.xml.xmb` — the master proto-object database.
//!
//! Each `<Object>` element describes a game entity: unit, building, projectile, etc.

use alloc::string::String;
use alloc::vec::Vec;

use crate::node_ext::{NodeExt, expect_root};

/// A single proto-object definition from `objects.xml`.
#[derive(Debug, Clone, Default)]
pub struct ProtoObject {
    /// Object name (unique key), e.g. `"unsc_veh_warthog_01"`.
    pub name: String,
    /// Numeric ID (the `id` attribute, often 0).
    pub id: Option<i32>,
    /// Database ID (the `dbid` attribute).
    pub dbid: Option<i32>,
    /// Object class: `"Unit"`, `"Squad"`, `"Building"`, `"Object"`, etc.
    pub object_class: Option<String>,
    /// Visual file path (relative, no `art\` prefix, no extension).
    pub visual: Option<String>,
    /// Physics info bare name (resolves to `physics\{name}.*`).
    pub physics_info: Option<String>,
    /// Physics replacement info bare name.
    pub physics_replacement_info: Option<String>,
    /// Tactics file base name (resolves to `data\tactics\{name}.xmb`).
    pub tactics: Option<String>,
    /// Hitpoints.
    pub hitpoints: Option<f32>,
    /// Movement type: `"Land"`, `"Air"`, etc.
    pub movement_type: Option<String>,
    /// Velocity.
    pub velocity: Option<f32>,
    /// Turn rate.
    pub turn_rate: Option<f32>,
    /// Line of sight radius.
    pub los: Option<f32>,
    /// Portrait icon path.
    pub portrait_icon: Option<String>,
    /// Display name string ID.
    pub display_name_id: Option<i32>,
    /// Rollover text string ID.
    pub rollover_text_id: Option<i32>,
    /// Bounty value.
    pub bounty: Option<i32>,
    /// Flags (e.g. `"ForceToGaiaPlayer"`, `"Invulnerable"`, `"NoRender"`).
    pub flags: Vec<String>,
    /// Object types (e.g. `"CreepDifficultyMarker"`).
    pub object_types: Vec<String>,
    /// Hardpoints.
    pub hardpoints: Vec<Hardpoint>,
    /// Veterancy levels.
    pub veterancy: Vec<VeterancyLevel>,
}

/// A hardpoint on a proto-object (turret mount point).
#[derive(Debug, Clone, Default)]
pub struct Hardpoint {
    pub name: String,
    pub yaw_rate: Option<f32>,
    pub pitch_rate: Option<f32>,
    pub pitch_min_angle: Option<f32>,
    pub pitch_max_angle: Option<f32>,
    pub yaw_attachment: Option<String>,
    pub pitch_attachment: Option<String>,
    pub autocenter: Option<bool>,
}

/// A veterancy level entry.
#[derive(Debug, Clone, Default)]
pub struct VeterancyLevel {
    pub level: i32,
    pub xp: Option<f32>,
    pub damage: Option<f32>,
    pub velocity: Option<f32>,
    pub accuracy: Option<f32>,
    pub work_rate: Option<f32>,
    pub weapon_range: Option<f32>,
    pub damage_taken: Option<f32>,
}

/// Parse all proto-objects from an `objects.xml.xmb` document.
pub fn parse(doc: &xmb::Document) -> crate::Result<Vec<ProtoObject>> {
    let root = expect_root(doc, "Objects")?;
    let mut objects = Vec::new();

    for node in root.children_named("Object") {
        objects.push(parse_object(node));
    }

    Ok(objects)
}

fn parse_object(node: &bdt::Node) -> ProtoObject {
    let mut obj = ProtoObject {
        name: node.attr_str("name").unwrap_or_default(),
        id: node.attr_i32("id"),
        dbid: node.attr_i32("dbid"),
        object_class: node.child_text("ObjectClass"),
        visual: node.child_text("Visual"),
        physics_info: node.child_text("PhysicsInfo"),
        physics_replacement_info: node.child_text("PhysicsReplacementInfo"),
        tactics: node.child_text("Tactics"),
        hitpoints: node.child_f32("Hitpoints"),
        movement_type: node.child_text("MovementType"),
        velocity: node.child_f32("Velocity"),
        turn_rate: node.child_f32("TurnRate"),
        los: node.child_f32("LOS"),
        portrait_icon: node.child_text("PortraitIcon"),
        display_name_id: node.child_i32("DisplayNameID"),
        rollover_text_id: node.child_i32("RolloverTextID"),
        bounty: node.child_i32("Bounty"),
        ..Default::default()
    };

    for flag in node.children_named("Flag") {
        obj.flags.push(flag.text_string());
    }
    for ot in node.children_named("ObjectType") {
        obj.object_types.push(ot.text_string());
    }
    for hp in node.children_named("Hardpoint") {
        obj.hardpoints.push(parse_hardpoint(hp));
    }
    for vet in node.children_named("Veterancy") {
        obj.veterancy.push(parse_veterancy(vet));
    }

    obj
}

fn parse_hardpoint(node: &bdt::Node) -> Hardpoint {
    Hardpoint {
        name: node.attr_str("name").unwrap_or_default(),
        yaw_rate: node.attr_f32("yawrate"),
        pitch_rate: node.attr_f32("pitchrate"),
        pitch_min_angle: node.attr_f32("pitchMinAngle"),
        pitch_max_angle: node.attr_f32("pitchMaxAngle"),
        yaw_attachment: node.attr_str("yawattachment"),
        pitch_attachment: node.attr_str("pitchattachment"),
        autocenter: node.attr_bool("autocenter"),
    }
}

fn parse_veterancy(node: &bdt::Node) -> VeterancyLevel {
    VeterancyLevel {
        level: node.attr_i32("Level").unwrap_or(0),
        xp: node.attr_f32("XP"),
        damage: node.attr_f32("Damage"),
        velocity: node.attr_f32("Velocity"),
        accuracy: node.attr_f32("Accuracy"),
        work_rate: node.attr_f32("WorkRate"),
        weapon_range: node.attr_f32("WeaponRange"),
        damage_taken: node.attr_f32("DamageTaken"),
    }
}
