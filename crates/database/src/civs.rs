//! Parser for `civs.xml.xmb` — civilization definitions.
//!
//! Each `<Civ>` element describes a faction (UNSC, Covenant, Gaia).

use alloc::string::String;
use alloc::vec::Vec;

use crate::node_ext::{NodeExt, expect_root};

/// A single civilization definition from `civs.xml`.
#[derive(Debug, Clone, Default)]
pub struct Civ {
    /// Civilization name, e.g. `"UNSC"`, `"Covenant"`, `"Gaia"`.
    pub name: String,
    /// Alpha flag (from attribute).
    pub alpha: Option<i32>,
    /// Display name string ID.
    pub display_name_id: Option<i32>,
    /// Civ tech to apply at game start.
    pub civ_tech: Option<String>,
    /// Command acknowledgement object.
    pub command_ack_object: Option<String>,
    /// Rally point object.
    pub rally_point_object: Option<String>,
    /// Local rally point object.
    pub local_rally_point_object: Option<String>,
    /// Hull expansion value.
    pub expand_hull: Option<f32>,
    /// Terrain push-off value.
    pub terrain_push_off: Option<f32>,
    /// Building magnet range.
    pub building_magnet_range: Option<f32>,
    /// Transport unit name.
    pub transport: Option<String>,
    /// Transport trigger unit name.
    pub transport_trigger: Option<String>,
    /// Sound bank file.
    pub sound_bank: Option<String>,
    /// Leader menu name string ID.
    pub leader_menu_name_id: Option<i32>,
    /// Whether powers come from the hero unit.
    pub power_from_hero: Option<bool>,
    /// UI control background image path.
    pub ui_control_background: Option<String>,
}

/// Parse all civilizations from a `civs.xml.xmb` document.
pub fn parse(doc: &xmb::Document) -> crate::Result<Vec<Civ>> {
    let root = expect_root(doc, "Civs")?;
    let mut civs = Vec::new();

    for node in root.children_named("Civ") {
        civs.push(parse_civ(node));
    }

    Ok(civs)
}

fn parse_civ(node: &bdt::Node) -> Civ {
    Civ {
        name: node.child_text("Name").unwrap_or_default(),
        alpha: node.attr_i32("Alpha"),
        display_name_id: node.child_i32("DisplayNameID"),
        civ_tech: node.child_text("CivTech"),
        command_ack_object: node.child_text("CommandAckObject"),
        rally_point_object: node.child_text("RallyPointObject"),
        local_rally_point_object: node.child_text("LocalRallyPointObject"),
        expand_hull: node.child_f32("ExpandHull"),
        terrain_push_off: node.child_f32("TerrainPushOff"),
        building_magnet_range: node.child_f32("BuildingMagnetRange"),
        transport: node.child_text("Transport"),
        transport_trigger: node.child_text("TransportTrigger"),
        sound_bank: node.child_text("SoundBank"),
        leader_menu_name_id: node.child_i32("LeaderMenuNameID"),
        power_from_hero: node.child_bool("PowerFromHero"),
        ui_control_background: node.child_text("UIControlBackground"),
    }
}
