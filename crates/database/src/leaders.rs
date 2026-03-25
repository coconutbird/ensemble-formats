//! Parser for `leaders.xml.xmb` — leader definitions.
//!
//! Each `<Leader>` element describes a playable leader (Cutter, Arbiter, etc.).

use alloc::string::String;
use alloc::vec::Vec;

use crate::node_ext::{NodeExt, expect_root};

/// A single leader definition from `leaders.xml`.
#[derive(Debug, Clone, Default)]
pub struct Leader {
    /// Leader name (unique key), e.g. `"Cutter"`, `"Arbiter"`.
    pub name: String,
    /// Icon path.
    pub icon: Option<String>,
    /// Leader picker order index.
    pub leader_picker_order: Option<i32>,
    /// Stats ID.
    pub stats_id: Option<i32>,
    /// Default player slot flags (hex string).
    pub default_player_slot_flags: Option<String>,
    /// Alpha flag.
    pub alpha: Option<i32>,
    /// Whether this is a random leader placeholder.
    pub random: Option<bool>,
    /// Civilization name.
    pub civ: Option<String>,
    /// Tech to apply for this leader.
    pub tech: Option<String>,
    /// Name string ID.
    pub name_id: Option<i32>,
    /// Description string ID.
    pub description_id: Option<i32>,
    /// Flash civ ID.
    pub flash_civ_id: Option<i32>,
    /// Flash image name.
    pub flash_img: Option<String>,
    /// Flash portrait image path.
    pub flash_portrait: Option<String>,
    /// UI control background image path.
    pub ui_control_background: Option<String>,
    /// Starting resources.
    pub resources: Vec<ResourceEntry>,
    /// Starting unit definition.
    pub starting_unit: Option<StartingUnit>,
    /// Starting squad definitions.
    pub starting_squads: Vec<StartingSquad>,
    /// Rally point offset.
    pub rally_point_offset: Option<String>,
    /// Repair rate.
    pub repair_rate: Option<f32>,
    /// Repair delay in seconds.
    pub repair_delay: Option<f32>,
    /// Repair cost.
    pub repair_cost: Vec<ResourceEntry>,
    /// Repair time in seconds.
    pub repair_time: Option<f32>,
    /// Population caps.
    pub pops: Vec<PopEntry>,
}

/// A resource entry (type + amount).
#[derive(Debug, Clone, Default)]
pub struct ResourceEntry {
    pub resource_type: String,
    pub amount: f32,
}

/// A starting unit definition.
#[derive(Debug, Clone, Default)]
pub struct StartingUnit {
    pub proto_object: String,
    pub offset: Option<String>,
    pub build_other: Option<String>,
    pub dopple_on_start: Option<bool>,
}

/// A starting squad definition.
#[derive(Debug, Clone, Default)]
pub struct StartingSquad {
    pub proto_squad: String,
    pub fly_in: Option<bool>,
    pub offset: Option<String>,
}

/// A population entry (type + count + max).
#[derive(Debug, Clone, Default)]
pub struct PopEntry {
    pub pop_type: String,
    pub count: f32,
    pub max: Option<f32>,
}

/// Parse all leaders from a `leaders.xml.xmb` document.
pub fn parse(doc: &xmb::Document) -> crate::Result<Vec<Leader>> {
    let root = expect_root(doc, "Leaders")?;
    let mut leaders = Vec::new();

    for node in root.children_named("Leader") {
        leaders.push(parse_leader(node));
    }

    Ok(leaders)
}

fn parse_leader(node: &bdt::Node) -> Leader {
    let mut leader = Leader {
        name: node.attr_str("Name").unwrap_or_default(),
        icon: node.attr_str("Icon"),
        leader_picker_order: node.attr_i32("LeaderPickerOrder"),
        stats_id: node.attr_i32("StatsID"),
        default_player_slot_flags: node.attr_str("DefaultPlayerSlotFlags"),
        alpha: node.attr_i32("Alpha"),
        random: node.attr_bool("Random"),
        civ: node.child_text("Civ"),
        tech: node.child_text("Tech"),
        name_id: node.child_i32("NameID"),
        description_id: node.child_i32("DescriptionID"),
        flash_civ_id: node.child_i32("FlashCivID"),
        flash_img: node.child_text("FlashImg"),
        flash_portrait: node.child_text("FlashPortrait"),
        ui_control_background: node.child_text("UIControlBackground"),
        rally_point_offset: node.child_text("RallyPointOffset"),
        repair_rate: node.child_f32("RepairRate"),
        repair_delay: node.child_f32("RepairDelay"),
        repair_time: node.child_f32("RepairTime"),
        ..Default::default()
    };

    for res in node.children_named("Resource") {
        leader.resources.push(ResourceEntry {
            resource_type: res.attr_str("Type").unwrap_or_default(),
            amount: res.text_string().parse().unwrap_or(0.0),
        });
    }

    if let Some(su) = node.child("StartingUnit") {
        leader.starting_unit = Some(StartingUnit {
            proto_object: su.text_string(),
            offset: su.attr_str("Offset"),
            build_other: su.attr_str("BuildOther"),
            dopple_on_start: su.attr_bool("DoppleOnStart"),
        });
    }

    for ss in node.children_named("StartingSquad") {
        leader.starting_squads.push(StartingSquad {
            proto_squad: ss.text_string(),
            fly_in: ss.attr_bool("FlyIn"),
            offset: ss.attr_str("Offset"),
        });
    }

    for rc in node.children_named("RepairCost") {
        leader.repair_cost.push(ResourceEntry {
            resource_type: rc.attr_str("Type").unwrap_or_default(),
            amount: rc.text_string().parse().unwrap_or(0.0),
        });
    }

    for pop in node.children_named("Pop") {
        leader.pops.push(PopEntry {
            pop_type: pop.attr_str("Type").unwrap_or_default(),
            count: pop.text_string().parse().unwrap_or(0.0),
            max: pop.attr_f32("Max"),
        });
    }

    leader
}
