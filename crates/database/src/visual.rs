//! Parser for `.vis.xmb` — visual definitions.
//!
//! A visual file defines the models, animations, attachments, and effects for
//! a game entity. The root element is `<visual>` with `<model>` children.

use alloc::string::String;
use alloc::vec::Vec;
use serde::Deserialize;

use crate::node_ext::expect_root;

/// A complete visual definition from a `.vis.xmb` file.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Visual {
    /// Default model name (from `defaultmodel` attribute on root).
    #[serde(rename = "@defaultmodel")]
    pub default_model: Option<String>,
    /// Named models (e.g. `"Default"`, `"Turret"`, `"Wheel"`).
    #[serde(rename = "model", default)]
    pub models: Vec<Model>,
}

/// A named model within a visual.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Model {
    /// Model name (e.g. `"Default"`, `"Turret"`).
    #[serde(rename = "@name", default)]
    pub name: String,
    /// Component (contains asset refs and attachments).
    #[serde(rename = "component")]
    pub component: Option<Component>,
    /// Animations.
    #[serde(rename = "anim", default)]
    pub anims: Vec<Anim>,
}

/// Component data: model files and attachment points.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Component {
    /// Direct asset references in the component.
    #[serde(rename = "asset", default)]
    pub assets: Vec<Asset>,
    /// Attachment points (model refs, particle effects, etc.).
    #[serde(rename = "attach", default)]
    pub attachments: Vec<Attachment>,
    /// Impact/board/launch points.
    #[serde(rename = "point", default)]
    pub points: Vec<Point>,
    /// Logic-switched asset variants (tech upgrades).
    #[serde(rename = "logic")]
    pub logic: Option<Logic>,
}

/// An asset reference (model or animation file).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Asset {
    /// Asset type: `"Model"` or `"Anim"`.
    #[serde(rename = "@type", default)]
    pub asset_type: String,
    /// File path (relative, no `art\` prefix, no extension).
    #[serde(rename = "file")]
    pub file: Option<String>,
    /// Damage model file path.
    #[serde(rename = "damagefile")]
    pub damage_file: Option<String>,
    /// Weight for random selection.
    #[serde(rename = "weight")]
    pub weight: Option<i32>,
}

/// An attachment point.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Attachment {
    /// Attachment type: `"ModelRef"`, `"ParticleFile"`, `"TerrainEffect"`.
    #[serde(rename = "@type", default)]
    pub attach_type: String,
    /// Attachment name / reference.
    #[serde(rename = "@name", default)]
    pub name: String,
    /// Target bone.
    #[serde(rename = "@tobone")]
    pub to_bone: Option<String>,
    /// Source bone.
    #[serde(rename = "@frombone")]
    pub from_bone: Option<String>,
    /// Whether to sync animations.
    #[serde(rename = "@syncanims")]
    pub sync_anims: Option<bool>,
}

/// A point on a component (impact, board, launch, pickup).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Point {
    /// Point type: `"Impact"`, `"Board"`, `"Launch"`, `"Pickup"`.
    #[serde(rename = "@pointType", default)]
    pub point_type: String,
    /// Bone name.
    #[serde(rename = "@bone")]
    pub bone: Option<String>,
    /// Point data (material type, e.g. `"Metal"`).
    #[serde(rename = "@pointData")]
    pub point_data: Option<String>,
}

/// Logic switch for tech-based model variants.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Logic {
    /// Logic type: `"Tech"`.
    #[serde(rename = "@type", default)]
    pub logic_type: String,
    /// Logic data entries (one per tech level).
    #[serde(rename = "logicdata", default)]
    pub entries: Vec<LogicEntry>,
}

/// A single logic entry (maps a tech value to an asset).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct LogicEntry {
    /// Tech value that activates this variant (empty = default).
    #[serde(rename = "@value", default)]
    pub value: String,
    /// Model reference name.
    #[serde(rename = "@modelref")]
    pub model_ref: Option<String>,
    /// Weight.
    #[serde(rename = "@weight")]
    pub weight: Option<i32>,
    /// The asset selected for this variant.
    #[serde(rename = "asset")]
    pub asset: Option<Asset>,
}

/// An animation definition.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Anim {
    /// Animation type: `"Idle"`, `"Walk"`, `"Death"`, etc.
    #[serde(rename = "@type", default)]
    pub anim_type: String,
    /// Exit action: `"Loop"`, `"Freeze"`, `"Transition"`.
    #[serde(rename = "@exitAction")]
    pub exit_action: Option<String>,
    /// Tween time.
    #[serde(rename = "@tweenTime")]
    pub tween_time: Option<i32>,
    /// Tween-to animation name.
    #[serde(rename = "@tweenToAnimation")]
    pub tween_to_animation: Option<String>,
    /// Asset references (animation files).
    #[serde(rename = "asset", default)]
    pub assets: Vec<Asset>,
    /// Attachments active during this animation.
    #[serde(rename = "attach", default)]
    pub attachments: Vec<Attachment>,
}

/// Parse a visual definition from a `.vis.xmb` document.
pub fn parse(doc: &xmb::Document) -> crate::Result<Visual> {
    let root = expect_root(doc, "visual")?;
    let vis: Visual = bdt_serde::from_node(root)?;
    Ok(vis)
}
