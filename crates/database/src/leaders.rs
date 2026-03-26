//! Parser for `leaders.xml.xmb` — leader definitions.
//!
//! Each `<Leader>` element describes a playable leader (Cutter, Arbiter, etc.).

use alloc::string::String;
use alloc::vec::Vec;
use serde::Deserialize;

use crate::node_ext::expect_root;

/// A single leader definition from `leaders.xml`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Leader {
    /// Leader name (unique key), e.g. `"Cutter"`, `"Arbiter"`.
    #[serde(rename = "@Name", default)]
    pub name: String,
    /// Icon path.
    #[serde(rename = "@Icon")]
    pub icon: Option<String>,
    /// Leader picker order index.
    #[serde(rename = "@LeaderPickerOrder")]
    pub leader_picker_order: Option<i32>,
    /// Stats ID.
    #[serde(rename = "@StatsID")]
    pub stats_id: Option<i32>,
    /// Default player slot flags (hex string).
    #[serde(rename = "@DefaultPlayerSlotFlags")]
    pub default_player_slot_flags: Option<String>,
    /// Alpha flag.
    #[serde(rename = "@Alpha")]
    pub alpha: Option<i32>,
    /// Whether this is a random leader placeholder.
    #[serde(rename = "@Random")]
    pub random: Option<bool>,
    /// Civilization name.
    #[serde(rename = "Civ")]
    pub civ: Option<String>,
    /// Tech to apply for this leader.
    #[serde(rename = "Tech")]
    pub tech: Option<String>,
    /// Name string ID.
    #[serde(rename = "NameID")]
    pub name_id: Option<i32>,
    /// Description string ID.
    #[serde(rename = "DescriptionID")]
    pub description_id: Option<i32>,
    /// Flash civ ID.
    #[serde(rename = "FlashCivID")]
    pub flash_civ_id: Option<i32>,
    /// Flash image name.
    #[serde(rename = "FlashImg")]
    pub flash_img: Option<String>,
    /// Flash portrait image path.
    #[serde(rename = "FlashPortrait")]
    pub flash_portrait: Option<String>,
    /// UI control background image path.
    #[serde(rename = "UIControlBackground")]
    pub ui_control_background: Option<String>,
    /// Starting resources.
    #[serde(rename = "Resource", default)]
    pub resources: Vec<ResourceEntry>,
    /// Starting unit definition.
    #[serde(rename = "StartingUnit")]
    pub starting_unit: Option<StartingUnit>,
    /// Starting squad definitions.
    #[serde(rename = "StartingSquad", default)]
    pub starting_squads: Vec<StartingSquad>,
    /// Rally point offset.
    #[serde(rename = "RallyPointOffset")]
    pub rally_point_offset: Option<String>,
    /// Repair rate.
    #[serde(rename = "RepairRate")]
    pub repair_rate: Option<f32>,
    /// Repair delay in seconds.
    #[serde(rename = "RepairDelay")]
    pub repair_delay: Option<f32>,
    /// Repair cost.
    #[serde(rename = "RepairCost", default)]
    pub repair_cost: Vec<ResourceEntry>,
    /// Repair time in seconds.
    #[serde(rename = "RepairTime")]
    pub repair_time: Option<f32>,
    /// Population caps.
    #[serde(rename = "Pop", default)]
    pub pops: Vec<PopEntry>,
    /// Reverse hot-drop resource cost.
    #[serde(rename = "ReverseHotDropCost")]
    pub reverse_hot_drop_cost: Option<f32>,
    /// Test flag (debug/development).
    #[serde(rename = "@Test")]
    pub test: Option<bool>,
}

/// A resource entry (type + amount).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ResourceEntry {
    #[serde(rename = "@Type", default)]
    pub resource_type: String,
    #[serde(rename = "$text", default)]
    pub amount: f32,
}

/// A starting unit definition.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct StartingUnit {
    #[serde(rename = "$text", default)]
    pub proto_object: String,
    #[serde(rename = "@Offset")]
    pub offset: Option<String>,
    #[serde(rename = "@BuildOther")]
    pub build_other: Option<String>,
    #[serde(rename = "@DoppleOnStart")]
    pub dopple_on_start: Option<bool>,
}

/// A starting squad definition.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct StartingSquad {
    #[serde(rename = "$text", default)]
    pub proto_squad: String,
    #[serde(rename = "@FlyIn")]
    pub fly_in: Option<bool>,
    #[serde(rename = "@Offset")]
    pub offset: Option<String>,
}

/// A population entry (type + count + max).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PopEntry {
    #[serde(rename = "@Type", default)]
    pub pop_type: String,
    #[serde(rename = "$text", default)]
    pub count: f32,
    #[serde(rename = "@Max")]
    pub max: Option<f32>,
}

/// Parse all leaders from a `leaders.xml.xmb` document.
pub fn parse(doc: &xmb::Document) -> crate::Result<Vec<Leader>> {
    let root = expect_root(doc, "Leaders")?;
    let leaders: Vec<Leader> = root
        .children
        .iter()
        .filter(|c| c.name == "Leader")
        .map(bdt_serde::from_node)
        .collect::<Result<_, _>>()?;
    Ok(leaders)
}
