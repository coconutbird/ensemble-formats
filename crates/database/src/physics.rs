//! Parser for physics XMB files.
//!
//! Physics data is split across three files per entity:
//! - `*.physics.xmb` — physics config (blueprint ref, vehicle type, center offset)
//! - `*.blueprint.xmb` — physical properties (mass, friction, restitution, shape ref)
//! - `*.shp.xmb` — Havok collision shape (XML-serialized, e.g. `hkBoxShape`)

use alloc::string::String;
use alloc::vec::Vec;
use serde::Deserialize;

use crate::node_ext::expect_root;

/// Physics configuration from a `.physics.xmb` file.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Physics {
    /// Blueprint name reference.
    #[serde(rename = "blueprint")]
    pub blueprint: Option<String>,
    /// Whether this object can be thrown by projectiles.
    #[serde(rename = "ThrownByProjectiles")]
    pub thrown_by_projectiles: Option<bool>,
    /// Vehicle type name.
    #[serde(rename = "Vehicle")]
    pub vehicle: Option<String>,
    /// Center of mass offset (comma-separated floats).
    #[serde(rename = "CenterOffset")]
    pub center_offset: Option<String>,
    /// Terrain effects path.
    #[serde(rename = "TerrainEffects")]
    pub terrain_effects: Option<String>,
}

/// Physical properties from a `.blueprint.xmb` file.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Blueprint {
    /// Mass in kg.
    #[serde(rename = "mass")]
    pub mass: Option<f32>,
    /// Friction coefficient.
    #[serde(rename = "friction")]
    pub friction: Option<f32>,
    /// Restitution (bounciness).
    #[serde(rename = "restitution")]
    pub restitution: Option<f32>,
    /// Linear damping.
    #[serde(rename = "linearDamping")]
    pub linear_damping: Option<f32>,
    /// Angular damping.
    #[serde(rename = "angularDamping")]
    pub angular_damping: Option<f32>,
    /// Shape reference name.
    #[serde(rename = "shape")]
    pub shape: Option<String>,
}

/// A Havok collision shape from a `.shp.xmb` file.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Shape {
    /// Havok version string (e.g. `"V_20200_B_20031014"`).
    #[serde(rename = "@version")]
    pub hke_version: Option<String>,
    /// Shape objects.
    #[serde(rename = "hkobject", default)]
    pub objects: Vec<HavokObject>,
}

/// A single Havok object (shape primitive).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct HavokObject {
    /// Object name (e.g. `"body"`).
    #[serde(rename = "@name", default)]
    pub name: String,
    /// Object type (e.g. `"hkBoxShape"`, `"hkConvexVerticesShape"`).
    #[serde(rename = "@type", default)]
    pub object_type: String,
    /// Parameters as key-value pairs.
    #[serde(rename = "hkparam", default)]
    pub params: Vec<HavokParam>,
}

/// A Havok parameter.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct HavokParam {
    /// Parameter name (e.g. `"halfExtents"`, `"radius"`).
    #[serde(rename = "@name", default)]
    pub name: String,
    /// Parameter type (e.g. `"hkTypeVector4"`, `"hkTypeReal"`).
    #[serde(rename = "@type", default)]
    pub param_type: String,
    /// Raw value as string.
    #[serde(rename = "$text", default)]
    pub value: String,
}

/// Parse a `.physics.xmb` document.
pub fn parse_physics(doc: &xmb::Document) -> crate::Result<Physics> {
    let root = expect_root(doc, "physics")?;
    let physics: Physics = bdt_serde::from_node(root)?;
    Ok(physics)
}

/// Parse a `.blueprint.xmb` document.
pub fn parse_blueprint(doc: &xmb::Document) -> crate::Result<Blueprint> {
    let root = crate::node_ext::root_node(doc)?;
    let bp: Blueprint = bdt_serde::from_node(root)?;
    Ok(bp)
}

/// Parse a `.shp.xmb` document (Havok XML shapes).
pub fn parse_shape(doc: &xmb::Document) -> crate::Result<Shape> {
    let root = expect_root(doc, "hke")?;
    let shape: Shape = bdt_serde::from_node(root)?;
    Ok(shape)
}
