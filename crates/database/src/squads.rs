//! Parser for `squads.xml.xmb` — squad definitions.
//!
//! Squads are groups of units that the player trains and controls together.

use alloc::string::String;
use alloc::vec::Vec;

use crate::node_ext::{NodeExt, expect_root};

/// A squad definition from `squads.xml`.
#[derive(Debug, Clone, Default)]
pub struct Squad {
    /// Squad name (unique key), e.g. `"unsc_veh_warthog_01"`.
    pub name: String,
    /// Database ID.
    pub dbid: Option<i32>,
    /// Portrait icon path.
    pub portrait_icon: Option<String>,
    /// Minimap icon shape.
    pub minimap_icon: Option<String>,
    /// Display name string ID.
    pub display_name_id: Option<i32>,
    /// Rollover text string ID.
    pub rollover_text_id: Option<i32>,
    /// Role text string ID.
    pub role_text_id: Option<i32>,
    /// Prerequisite text string ID.
    pub prereq_text_id: Option<i32>,
    /// Build time in seconds.
    pub build_points: Option<f32>,
    /// Resource costs.
    pub costs: Vec<Cost>,
    /// Units in this squad.
    pub units: Vec<UnitEntry>,
    /// HP bar name.
    pub hp_bar: Option<String>,
    /// Birth configuration.
    pub birth: Option<String>,
    /// Flags.
    pub flags: Vec<String>,
    /// Leash distance for AI.
    pub leash_distance: Option<f32>,
    /// Aggro distance for AI.
    pub aggro_distance: Option<f32>,
    /// Sub-select sort priority.
    pub sub_select_sort: Option<i32>,
}

/// A resource cost entry.
#[derive(Debug, Clone, Default)]
pub struct Cost {
    pub resource_type: String,
    pub amount: f32,
}

/// A unit entry within a squad.
#[derive(Debug, Clone, Default)]
pub struct UnitEntry {
    /// Proto-object name this unit references.
    pub proto_object: String,
    /// Number of this unit in the squad.
    pub count: i32,
    /// Role: `"normal"`, etc.
    pub role: Option<String>,
}

/// Parse all squads from a `squads.xml.xmb` document.
pub fn parse(doc: &xmb::Document) -> crate::Result<Vec<Squad>> {
    let root = expect_root(doc, "Squads")?;
    let mut squads = Vec::new();

    for node in root.children_named("Squad") {
        squads.push(parse_squad(node));
    }

    Ok(squads)
}

fn parse_squad(node: &bdt::Node) -> Squad {
    let mut squad = Squad {
        name: node.attr_str("name").unwrap_or_default(),
        dbid: node.attr_i32("dbid"),
        portrait_icon: node.child_text("PortraitIcon"),
        minimap_icon: node.child_text("MinimapIcon"),
        display_name_id: node.child_i32("DisplayNameID"),
        rollover_text_id: node.child_i32("RolloverTextID"),
        role_text_id: node.child_i32("RoleTextID"),
        prereq_text_id: node.child_i32("PrereqTextID"),
        build_points: node.child_f32("BuildPoints"),
        hp_bar: node.child_text("HPBar"),
        birth: node.child_text("Birth"),
        leash_distance: node.child_f32("LeashDistance"),
        aggro_distance: node.child_f32("AggroDistance"),
        sub_select_sort: node.child_i32("SubSelectSort"),
        ..Default::default()
    };

    for cost in node.children_named("Cost") {
        squad.costs.push(Cost {
            resource_type: cost.attr_str("resourcetype").unwrap_or_default(),
            amount: cost.text.as_float().unwrap_or(0.0),
        });
    }

    if let Some(units_node) = node.child("Units") {
        for unit in units_node.children_named("Unit") {
            squad.units.push(UnitEntry {
                proto_object: unit.text_string(),
                count: unit.attr_i32("count").unwrap_or(1),
                role: unit.attr_str("role"),
            });
        }
    }

    for flag in node.children_named("Flag") {
        squad.flags.push(flag.text_string());
    }

    squad
}
