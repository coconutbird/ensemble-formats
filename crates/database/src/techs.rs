//! Parser for `techs.xml.xmb` — tech tree definitions.
//!
//! Techs represent upgrades, unlocks, and game state modifications.

use alloc::string::String;
use alloc::vec::Vec;

use crate::node_ext::{NodeExt, expect_root};

/// A tech definition from `techs.xml`.
#[derive(Debug, Clone, Default)]
pub struct Tech {
    /// Tech name (unique key), e.g. `"Unsc_warthog_upgrade1"`.
    pub name: String,
    /// Tech type: `"Normal"`, etc.
    pub tech_type: Option<String>,
    /// Database ID.
    pub dbid: Option<i32>,
    /// Research time in seconds.
    pub research_points: Option<f32>,
    /// Status: `"OBTAINABLE"`, etc.
    pub status: Option<String>,
    /// Flags.
    pub flags: Vec<String>,
    /// Effects this tech applies.
    pub effects: Vec<TechEffect>,
}

/// A single tech effect.
#[derive(Debug, Clone, Default)]
pub struct TechEffect {
    /// Effect type: `"Data"`, etc.
    pub effect_type: String,
    /// Numeric amount.
    pub amount: Option<f32>,
    /// Subtype: `"Bounty"`, `"AbilityDisabled"`, etc.
    pub subtype: Option<String>,
    /// Relativity: `"Percent"`, `"Absolute"`, etc.
    pub relativity: Option<String>,
    /// Target type (from `<Target type="...">` attribute).
    pub target_type: Option<String>,
    /// Target value (text content of `<Target>`).
    pub target: Option<String>,
}

/// Parse all techs from a `techs.xml.xmb` document.
pub fn parse(doc: &xmb::Document) -> crate::Result<Vec<Tech>> {
    let root = expect_root(doc, "TechTree")?;
    let mut techs = Vec::new();

    for node in root.children_named("Tech") {
        techs.push(parse_tech(node));
    }

    Ok(techs)
}

fn parse_tech(node: &bdt::Node) -> Tech {
    let mut tech = Tech {
        name: node.attr_str("name").unwrap_or_default(),
        tech_type: node.attr_str("type"),
        dbid: node.child_i32("DBID"),
        research_points: node.child_f32("ResearchPoints"),
        status: node.child_text("Status"),
        ..Default::default()
    };

    for flag in node.children_named("Flag") {
        tech.flags.push(flag.text_string());
    }

    if let Some(effects_node) = node.child("Effects") {
        for effect in effects_node.children_named("Effect") {
            tech.effects.push(parse_effect(effect));
        }
    }

    tech
}

fn parse_effect(node: &bdt::Node) -> TechEffect {
    let mut effect = TechEffect {
        effect_type: node.attr_str("type").unwrap_or_default(),
        amount: node.attr_f32("amount"),
        subtype: node.attr_str("subtype"),
        relativity: node.attr_str("relativity"),
        ..Default::default()
    };

    if let Some(target) = node.child("Target") {
        effect.target_type = target.attr_str("type");
        let text = target.text_string();
        if !text.is_empty() {
            effect.target = Some(text);
        }
    }

    effect
}
