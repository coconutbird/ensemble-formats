//! Parser for `powers.xml.xmb` — leader power definitions.
//!
//! Each `<Power>` element describes a leader power (orbital bombardment, MAC blast, etc.).

use alloc::string::String;
use alloc::vec::Vec;

use crate::node_ext::{NodeExt, expect_root};

/// A single leader power definition from `powers.xml`.
#[derive(Debug, Clone, Default)]
pub struct Power {
    /// Power name (unique key), e.g. `"UnscLeaderNuke"`.
    pub name: String,
    /// Trigger script file name.
    pub trigger_script: Option<String>,
    /// Power attributes.
    pub attributes: PowerAttributes,
    /// Data levels (level-specific parameters).
    pub data_levels: Vec<DataLevel>,
}

/// Attributes block for a power.
#[derive(Debug, Clone, Default)]
pub struct PowerAttributes {
    /// Power type: `"Transport"`, `"Cleansing"`, etc.
    pub power_type: Option<String>,
    /// Whether the power has infinite uses.
    pub infinite_uses: bool,
    /// Whether this is a leader power.
    pub leader_power: bool,
    /// Auto-recharge flag.
    pub auto_recharge: Option<i32>,
    /// Display name string ID.
    pub display_name_id: Option<i32>,
    /// Rollover text string ID.
    pub rollover_text_id: Option<i32>,
    /// Prereq text string ID.
    pub prereq_text_id: Option<i32>,
    /// Icon path.
    pub icon: Option<String>,
    /// Icon location index.
    pub icon_location: Option<i32>,
    /// UI radius.
    pub ui_radius: Option<f32>,
    /// Supply cost.
    pub cost_supplies: Option<f32>,
    /// Power cost.
    pub cost_power: Option<f32>,
    /// Show transport arrows.
    pub show_transport_arrows: Option<bool>,
    /// Show limit flag.
    pub show_limit: bool,
    /// Min distance to squad.
    pub min_distance_to_squad: Option<f32>,
    /// Max distance to squad.
    pub max_distance_to_squad: Option<f32>,
}

/// A data level entry (level-specific power parameters).
#[derive(Debug, Clone, Default)]
pub struct DataLevel {
    /// Level index (0-based). -1 for BaseDataLevel.
    pub level: i32,
    /// Key-value data entries.
    pub entries: Vec<DataEntry>,
}

/// A single data entry within a data level.
#[derive(Debug, Clone, Default)]
pub struct DataEntry {
    /// Data type: `"float"`, `"int"`, `"sound"`, `"protoobject"`, `"texture"`, etc.
    pub data_type: String,
    /// Data name key.
    pub name: String,
    /// Data value (text content).
    pub value: String,
}

/// Parse all powers from a `powers.xml.xmb` document.
pub fn parse(doc: &xmb::Document) -> crate::Result<Vec<Power>> {
    let root = expect_root(doc, "Powers")?;
    let mut powers = Vec::new();

    for node in root.children_named("Power") {
        powers.push(parse_power(node));
    }

    Ok(powers)
}

fn parse_power(node: &bdt::Node) -> Power {
    let mut power = Power {
        name: node.attr_str("name").unwrap_or_default(),
        trigger_script: node.child_text("TriggerScript"),
        ..Default::default()
    };

    if let Some(attrs) = node.child("Attributes") {
        power.attributes = parse_attributes(attrs);

        // BaseDataLevel is inside Attributes
        if let Some(base) = attrs.child("BaseDataLevel") {
            power.data_levels.push(parse_data_level(base, -1));
        }

        // DataLevel entries inside Attributes
        for dl in attrs.children_named("DataLevel") {
            let level = dl.attr_i32("level").unwrap_or(0);
            power.data_levels.push(parse_data_level(dl, level));
        }
    }

    power
}

fn parse_attributes(node: &bdt::Node) -> PowerAttributes {
    let (cost_supplies, cost_power) = if let Some(cost) = node.child("Cost") {
        (cost.attr_f32("Supplies"), cost.attr_f32("Power"))
    } else {
        (None, None)
    };

    PowerAttributes {
        power_type: node.child_text("PowerType"),
        infinite_uses: node.child("InfiniteUses").is_some(),
        leader_power: node.child("LeaderPower").is_some(),
        auto_recharge: node.child_i32("AutoRecharge"),
        display_name_id: node.child_i32("DisplayNameID"),
        rollover_text_id: node.child_i32("RolloverTextID"),
        prereq_text_id: node.child_i32("PrereqTextID"),
        icon: node.child_text("Icon"),
        icon_location: node.child_i32("IconLocation"),
        ui_radius: node.child_f32("UIRadius"),
        cost_supplies,
        cost_power,
        show_transport_arrows: node.child_bool("ShowTransportArrows"),
        show_limit: node.child("ShowLimit").is_some(),
        min_distance_to_squad: node.child_f32("MinDistanceToSquad"),
        max_distance_to_squad: node.child_f32("MaxDistanceToSquad"),
    }
}

fn parse_data_level(node: &bdt::Node, level: i32) -> DataLevel {
    let mut entries = Vec::new();
    for data in node.children_named("Data") {
        entries.push(DataEntry {
            data_type: data.attr_str("type").unwrap_or_default(),
            name: data.attr_str("name").unwrap_or_default(),
            value: data.text_string(),
        });
    }
    DataLevel { level, entries }
}
