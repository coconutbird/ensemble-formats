//! Parser for `powers.xml.xmb` — leader power definitions.
//!
//! Each `<Power>` element describes a leader power (orbital bombardment, MAC blast, etc.).

use alloc::string::String;
use alloc::vec::Vec;
use serde::Deserialize;

use crate::node_ext::expect_root;

/// A single leader power definition from `powers.xml`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Power {
    /// Power name (unique key), e.g. `"UnscLeaderNuke"`.
    #[serde(rename = "@name", default)]
    pub name: String,
    /// Trigger script file name.
    #[serde(rename = "TriggerScript")]
    pub trigger_script: Option<String>,
    /// Power attributes (contains data levels too).
    #[serde(rename = "Attributes")]
    pub attributes: Option<PowerAttributes>,
}

/// Attributes block for a power.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PowerAttributes {
    /// Power type: `"Transport"`, `"Cleansing"`, etc.
    #[serde(rename = "PowerType")]
    pub power_type: Option<String>,
    /// Presence-based: element exists = true.
    #[serde(rename = "InfiniteUses")]
    pub infinite_uses: Option<String>,
    /// Presence-based: element exists = true.
    #[serde(rename = "LeaderPower")]
    pub leader_power: Option<String>,
    /// Auto-recharge flag.
    #[serde(rename = "AutoRecharge")]
    pub auto_recharge: Option<i32>,
    /// Display name string ID.
    #[serde(rename = "DisplayNameID")]
    pub display_name_id: Option<i32>,
    /// Rollover text string ID.
    #[serde(rename = "RolloverTextID")]
    pub rollover_text_id: Option<i32>,
    /// Prereq text string ID.
    #[serde(rename = "PrereqTextID")]
    pub prereq_text_id: Option<i32>,
    /// Icon path.
    #[serde(rename = "Icon")]
    pub icon: Option<String>,
    /// Icon location index.
    #[serde(rename = "IconLocation")]
    pub icon_location: Option<i32>,
    /// UI radius.
    #[serde(rename = "UIRadius")]
    pub ui_radius: Option<f32>,
    /// Cost element.
    #[serde(rename = "Cost")]
    pub cost: Option<PowerCost>,
    /// Show transport arrows.
    #[serde(rename = "ShowTransportArrows")]
    pub show_transport_arrows: Option<bool>,
    /// Presence-based: element exists = true.
    #[serde(rename = "ShowLimit")]
    pub show_limit: Option<String>,
    /// Min distance to squad.
    #[serde(rename = "MinDistanceToSquad")]
    pub min_distance_to_squad: Option<f32>,
    /// Max distance to squad.
    #[serde(rename = "MaxDistanceToSquad")]
    pub max_distance_to_squad: Option<f32>,
    /// Base data level (inside Attributes).
    #[serde(rename = "BaseDataLevel")]
    pub base_data_level: Option<DataLevel>,
    /// Data levels (inside Attributes).
    #[serde(rename = "DataLevel", default)]
    pub data_levels: Vec<DataLevel>,
    /// Camera effect on power activation.
    #[serde(rename = "CameraEffectIn")]
    pub camera_effect_in: Option<String>,
    /// Camera effect on power deactivation.
    #[serde(rename = "CameraEffectOut")]
    pub camera_effect_out: Option<String>,
    /// Allow user camera scroll during power.
    #[serde(rename = "CameraEnableUserScroll")]
    pub camera_enable_user_scroll: Option<bool>,
    /// Allow user camera yaw during power.
    #[serde(rename = "CameraEnableUserYaw")]
    pub camera_enable_user_yaw: Option<bool>,
    /// Allow user camera zoom during power.
    #[serde(rename = "CameraEnableUserZoom")]
    pub camera_enable_user_zoom: Option<bool>,
    /// Camera pitch maximum during power.
    #[serde(rename = "CameraPitchMax")]
    pub camera_pitch_max: Option<f32>,
    /// Camera pitch minimum during power.
    #[serde(rename = "CameraPitchMin")]
    pub camera_pitch_min: Option<f32>,
    /// Camera zoom maximum during power.
    #[serde(rename = "CameraZoomMax")]
    pub camera_zoom_max: Option<f32>,
    /// Camera zoom minimum during power.
    #[serde(rename = "CameraZoomMin")]
    pub camera_zoom_min: Option<f32>,
    /// Minigame name associated with this power.
    #[serde(rename = "Minigame")]
    pub minigame: Option<String>,
    /// Multi-recharge power name.
    #[serde(rename = "MultiRechargePower")]
    pub multi_recharge_power: Option<String>,
    /// Power cannot be disrupted.
    #[serde(rename = "NotDisruptable")]
    pub not_disruptable: Option<String>,
    /// Sequential recharge flag.
    #[serde(rename = "SequentialRecharge")]
    pub sequential_recharge: Option<String>,
    /// Show in power menu.
    #[serde(rename = "ShowInPowerMenu")]
    pub show_in_power_menu: Option<bool>,
    /// Show target highlight effect.
    #[serde(rename = "ShowTargetHighlight")]
    pub show_target_highlight: Option<bool>,
    /// Tech prerequisite name.
    #[serde(rename = "TechPrereq")]
    pub tech_prereq: Option<String>,
    /// Whether this is a unit power.
    #[serde(rename = "UnitPower")]
    pub unit_power: Option<bool>,
    /// Population type reference.
    #[serde(rename = "Pop")]
    pub pop: Option<String>,

    /// Unused leftover from development — loaded by the Flash UI system, not the power loader.
    #[serde(rename = "FlashUI")]
    pub flash_ui: Option<String>,
}

/// Cost element with attribute-based supplies/power.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PowerCost {
    #[serde(rename = "@Supplies")]
    pub supplies: Option<f32>,
    #[serde(rename = "@Power")]
    pub power: Option<f32>,
}

/// A data level entry (level-specific power parameters).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DataLevel {
    /// Level index (0-based). Absent for BaseDataLevel.
    #[serde(rename = "@level")]
    pub level: Option<i32>,
    /// Key-value data entries.
    #[serde(rename = "Data", default)]
    pub entries: Vec<DataEntry>,
}

/// A single data entry within a data level.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DataEntry {
    /// Data type: `"float"`, `"int"`, `"sound"`, `"protoobject"`, `"texture"`, etc.
    #[serde(rename = "@type", default)]
    pub data_type: String,
    /// Data name key.
    #[serde(rename = "@name", default)]
    pub name: String,
    /// Data value (text content).
    #[serde(rename = "$text", default)]
    pub value: String,
}

/// Parse all powers from a `powers.xml.xmb` document.
pub fn parse(doc: &xmb::Document) -> crate::Result<Vec<Power>> {
    let root = expect_root(doc, "Powers")?;
    let powers: Vec<Power> = root
        .children
        .iter()
        .filter(|c| c.name == "Power")
        .map(bdt_serde::from_node)
        .collect::<Result<_, _>>()?;
    Ok(powers)
}
