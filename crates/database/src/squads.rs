//! Parser for `squads.xml.xmb` — squad definitions.
//!
//! Squads are groups of units that the player trains and controls together.

use alloc::string::String;
use alloc::vec::Vec;
use serde::Deserialize;

use crate::node_ext::expect_root;

/// A squad definition from `squads.xml`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Squad {
    /// Squad name (unique key), e.g. `"unsc_veh_warthog_01"`.
    #[serde(rename = "@name", default)]
    pub name: String,
    /// Database ID.
    #[serde(rename = "@dbid")]
    pub dbid: Option<i32>,
    /// Portrait icon path.
    #[serde(rename = "PortraitIcon")]
    pub portrait_icon: Option<String>,
    /// Minimap icon shape.
    #[serde(rename = "MinimapIcon")]
    pub minimap_icon: Option<String>,
    /// Display name string ID.
    #[serde(rename = "DisplayNameID")]
    pub display_name_id: Option<i32>,
    /// Rollover text string ID.
    #[serde(rename = "RolloverTextID")]
    pub rollover_text_id: Option<i32>,
    /// Role text string ID.
    #[serde(rename = "RoleTextID")]
    pub role_text_id: Option<i32>,
    /// Prerequisite text string ID.
    #[serde(rename = "PrereqTextID")]
    pub prereq_text_id: Option<i32>,
    /// Build time in seconds.
    #[serde(rename = "BuildPoints")]
    pub build_points: Option<f32>,
    /// Resource costs.
    #[serde(rename = "Cost", default)]
    pub costs: Vec<Cost>,
    /// Units in this squad (wrapper element).
    #[serde(rename = "Units")]
    pub units: Option<UnitsWrapper>,
    /// HP bar name.
    #[serde(rename = "HPBar")]
    pub hp_bar: Option<String>,
    /// Birth configuration.
    #[serde(rename = "Birth")]
    pub birth: Option<String>,
    /// Flags.
    #[serde(rename = "Flag", default)]
    pub flags: Vec<String>,
    /// Leash distance for AI.
    #[serde(rename = "LeashDistance")]
    pub leash_distance: Option<f32>,
    /// Aggro distance for AI.
    #[serde(rename = "AggroDistance")]
    pub aggro_distance: Option<f32>,
    /// Sub-select sort priority.
    #[serde(rename = "SubSelectSort")]
    pub sub_select_sort: Option<i32>,
}

/// Wrapper for the `<Units>` element containing `<Unit>` children.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct UnitsWrapper {
    #[serde(rename = "Unit", default)]
    pub entries: Vec<UnitEntry>,
}

/// A resource cost entry.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Cost {
    #[serde(rename = "@resourcetype", default)]
    pub resource_type: String,
    #[serde(rename = "$text", default)]
    pub amount: f32,
}

/// A unit entry within a squad.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct UnitEntry {
    /// Proto-object name this unit references.
    #[serde(rename = "$text", default)]
    pub proto_object: String,
    /// Number of this unit in the squad.
    #[serde(rename = "@count", default = "default_one")]
    pub count: i32,
    /// Role: `"normal"`, etc.
    #[serde(rename = "@role")]
    pub role: Option<String>,
}

fn default_one() -> i32 {
    1
}

/// Parse all squads from a `squads.xml.xmb` document.
pub fn parse(doc: &xmb::Document) -> crate::Result<Vec<Squad>> {
    let root = expect_root(doc, "Squads")?;
    let squads: Vec<Squad> = root
        .children
        .iter()
        .filter(|c| c.name == "Squad")
        .map(bdt_serde::from_node)
        .collect::<Result<_, _>>()?;
    Ok(squads)
}
