//! Parser for `techs.xml.xmb` — tech tree definitions.
//!
//! Techs represent upgrades, unlocks, and game state modifications.

use alloc::string::String;
use alloc::vec::Vec;
use serde::Deserialize;

use crate::node_ext::expect_root;

/// A tech definition from `techs.xml`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Tech {
    /// Tech name (unique key), e.g. `"Unsc_warthog_upgrade1"`.
    #[serde(rename = "@name", default)]
    pub name: String,
    /// Tech type: `"Normal"`, etc.
    #[serde(rename = "@type")]
    pub tech_type: Option<String>,
    /// Database ID.
    #[serde(rename = "DBID")]
    pub dbid: Option<i32>,
    /// Research time in seconds.
    #[serde(rename = "ResearchPoints")]
    pub research_points: Option<f32>,
    /// Status: `"OBTAINABLE"`, etc.
    #[serde(rename = "Status")]
    pub status: Option<String>,
    /// Flags.
    #[serde(rename = "Flag", default)]
    pub flags: Vec<String>,
    /// Effects wrapper.
    #[serde(rename = "Effects")]
    pub effects: Option<EffectsWrapper>,
}

/// Wrapper for the `<Effects>` element containing `<Effect>` children.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct EffectsWrapper {
    #[serde(rename = "Effect", default)]
    pub entries: Vec<TechEffect>,
}

/// A single tech effect.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TechEffect {
    /// Effect type: `"Data"`, etc.
    #[serde(rename = "@type", default)]
    pub effect_type: String,
    /// Numeric amount.
    #[serde(rename = "@amount")]
    pub amount: Option<f32>,
    /// Subtype: `"Bounty"`, `"AbilityDisabled"`, etc.
    #[serde(rename = "@subtype")]
    pub subtype: Option<String>,
    /// Relativity: `"Percent"`, `"Absolute"`, etc.
    #[serde(rename = "@relativity")]
    pub relativity: Option<String>,
    /// Target element.
    #[serde(rename = "Target")]
    pub target: Option<EffectTarget>,
}

/// Target element within an effect: `<Target type="...">value</Target>`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct EffectTarget {
    #[serde(rename = "@type")]
    pub target_type: Option<String>,
    #[serde(rename = "$text")]
    pub value: Option<String>,
}

/// Parse all techs from a `techs.xml.xmb` document.
pub fn parse(doc: &xmb::Document) -> crate::Result<Vec<Tech>> {
    let root = expect_root(doc, "TechTree")?;
    let techs: Vec<Tech> = root
        .children
        .iter()
        .filter(|c| c.name == "Tech")
        .map(bdt_serde::from_node)
        .collect::<Result<_, _>>()?;
    Ok(techs)
}
