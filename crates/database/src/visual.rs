//! Parser for `.vis.xmb` — visual definitions.
//!
//! A visual file defines the models, animations, attachments, and effects for
//! a game entity. The root element is `<visual>` with `<model>` children.

use alloc::string::String;
use alloc::vec::Vec;

use crate::node_ext::{NodeExt, expect_root};

/// A complete visual definition from a `.vis.xmb` file.
#[derive(Debug, Clone, Default)]
pub struct Visual {
    /// Default model name (from `defaultmodel` attribute on root).
    pub default_model: Option<String>,
    /// Named models (e.g. `"Default"`, `"Turret"`, `"Wheel"`).
    pub models: Vec<Model>,
}

/// A named model within a visual.
#[derive(Debug, Clone, Default)]
pub struct Model {
    /// Model name (e.g. `"Default"`, `"Turret"`).
    pub name: String,
    /// Component (contains asset refs and attachments).
    pub component: Option<Component>,
    /// Animations.
    pub anims: Vec<Anim>,
}

/// Component data: model files and attachment points.
#[derive(Debug, Clone, Default)]
pub struct Component {
    /// Direct asset references in the component.
    pub assets: Vec<Asset>,
    /// Attachment points (model refs, particle effects, etc.).
    pub attachments: Vec<Attachment>,
    /// Impact/board/launch points.
    pub points: Vec<Point>,
    /// Logic-switched asset variants (tech upgrades).
    pub logic: Option<Logic>,
}

/// An asset reference (model or animation file).
#[derive(Debug, Clone, Default)]
pub struct Asset {
    /// Asset type: `"Model"` or `"Anim"`.
    pub asset_type: String,
    /// File path (relative, no `art\` prefix, no extension).
    pub file: Option<String>,
    /// Damage model file path.
    pub damage_file: Option<String>,
    /// Weight for random selection.
    pub weight: Option<i32>,
}

/// An attachment point.
#[derive(Debug, Clone, Default)]
pub struct Attachment {
    /// Attachment type: `"ModelRef"`, `"ParticleFile"`, `"TerrainEffect"`.
    pub attach_type: String,
    /// Attachment name / reference.
    pub name: String,
    /// Target bone.
    pub to_bone: Option<String>,
    /// Source bone.
    pub from_bone: Option<String>,
    /// Whether to sync animations.
    pub sync_anims: Option<bool>,
}

/// A point on a component (impact, board, launch, pickup).
#[derive(Debug, Clone, Default)]
pub struct Point {
    /// Point type: `"Impact"`, `"Board"`, `"Launch"`, `"Pickup"`.
    pub point_type: String,
    /// Bone name.
    pub bone: Option<String>,
    /// Point data (material type, e.g. `"Metal"`).
    pub point_data: Option<String>,
}

/// Logic switch for tech-based model variants.
#[derive(Debug, Clone, Default)]
pub struct Logic {
    /// Logic type: `"Tech"`.
    pub logic_type: String,
    /// Logic data entries (one per tech level).
    pub entries: Vec<LogicEntry>,
}

/// A single logic entry (maps a tech value to an asset).
#[derive(Debug, Clone, Default)]
pub struct LogicEntry {
    /// Tech value that activates this variant (empty = default).
    pub value: String,
    /// Model reference name.
    pub model_ref: Option<String>,
    /// Weight.
    pub weight: Option<i32>,
    /// The asset selected for this variant.
    pub asset: Option<Asset>,
}

/// An animation definition.
#[derive(Debug, Clone, Default)]
pub struct Anim {
    /// Animation type: `"Idle"`, `"Walk"`, `"Death"`, etc.
    pub anim_type: String,
    /// Exit action: `"Loop"`, `"Freeze"`, `"Transition"`.
    pub exit_action: Option<String>,
    /// Tween time.
    pub tween_time: Option<i32>,
    /// Tween-to animation name.
    pub tween_to_animation: Option<String>,
    /// Asset references (animation files).
    pub assets: Vec<Asset>,
    /// Attachments active during this animation.
    pub attachments: Vec<Attachment>,
}

/// Parse a visual definition from a `.vis.xmb` document.
pub fn parse(doc: &xmb::Document) -> crate::Result<Visual> {
    let root = expect_root(doc, "visual")?;
    let mut vis = Visual {
        default_model: root.attr_str("defaultmodel"),
        ..Default::default()
    };

    for model_node in root.children_named("model") {
        vis.models.push(parse_model(model_node));
    }

    Ok(vis)
}

fn parse_model(node: &bdt::Node) -> Model {
    let mut model = Model {
        name: node.attr_str("name").unwrap_or_default(),
        ..Default::default()
    };

    if let Some(comp) = node.child("component") {
        model.component = Some(parse_component(comp));
    }

    for anim_node in node.children_named("anim") {
        model.anims.push(parse_anim(anim_node));
    }

    model
}

fn parse_component(node: &bdt::Node) -> Component {
    let mut comp = Component::default();

    for asset_node in node.children_named("asset") {
        comp.assets.push(parse_asset(asset_node));
    }
    for attach_node in node.children_named("attach") {
        comp.attachments.push(parse_attachment(attach_node));
    }
    for point_node in node.children_named("point") {
        comp.points.push(Point {
            point_type: point_node.attr_str("pointType").unwrap_or_default(),
            bone: point_node.attr_str("bone"),
            point_data: point_node.attr_str("pointData"),
        });
    }
    if let Some(logic_node) = node.child("logic") {
        comp.logic = Some(parse_logic(logic_node));
    }

    comp
}

fn parse_asset(node: &bdt::Node) -> Asset {
    Asset {
        asset_type: node.attr_str("type").unwrap_or_default(),
        file: node.child_text("file"),
        damage_file: node.child_text("damagefile"),
        weight: node.child_i32("weight"),
    }
}

fn parse_attachment(node: &bdt::Node) -> Attachment {
    Attachment {
        attach_type: node.attr_str("type").unwrap_or_default(),
        name: node.attr_str("name").unwrap_or_default(),
        to_bone: node.attr_str("tobone"),
        from_bone: node.attr_str("frombone"),
        sync_anims: node.attr_bool("syncanims"),
    }
}

fn parse_logic(node: &bdt::Node) -> Logic {
    let mut logic = Logic {
        logic_type: node.attr_str("type").unwrap_or_default(),
        ..Default::default()
    };

    for entry_node in node.children_named("logicdata") {
        let mut entry = LogicEntry {
            value: entry_node.attr_str("value").unwrap_or_default(),
            model_ref: entry_node.attr_str("modelref"),
            weight: entry_node.attr_i32("weight"),
            ..Default::default()
        };
        if let Some(asset_node) = entry_node.child("asset") {
            entry.asset = Some(parse_asset(asset_node));
        }
        logic.entries.push(entry);
    }

    logic
}

fn parse_anim(node: &bdt::Node) -> Anim {
    let mut anim = Anim {
        anim_type: node.attr_str("type").unwrap_or_default(),
        exit_action: node.attr_str("exitAction"),
        tween_time: node.attr_i32("tweenTime"),
        tween_to_animation: node.attr_str("tweenToAnimation"),
        ..Default::default()
    };

    for asset_node in node.children_named("asset") {
        anim.assets.push(parse_asset(asset_node));
    }
    for attach_node in node.children_named("attach") {
        anim.attachments.push(parse_attachment(attach_node));
    }

    anim
}
