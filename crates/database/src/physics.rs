//! Parser for physics XMB files.
//!
//! Physics data is split across three files per entity:
//! - `*.physics.xmb` — physics config (blueprint ref, vehicle type, center offset)
//! - `*.blueprint.xmb` — physical properties (mass, friction, restitution, shape ref)
//! - `*.shp.xmb` — Havok collision shape (XML-serialized, e.g. `hkBoxShape`)

use alloc::string::String;
use alloc::vec::Vec;

use crate::node_ext::{NodeExt, expect_root};

/// Physics configuration from a `.physics.xmb` file.
#[derive(Debug, Clone, Default)]
pub struct Physics {
    /// Blueprint name reference.
    pub blueprint: Option<String>,
    /// Whether this object can be thrown by projectiles.
    pub thrown_by_projectiles: Option<bool>,
    /// Vehicle type name.
    pub vehicle: Option<String>,
    /// Center of mass offset (comma-separated floats).
    pub center_offset: Option<String>,
    /// Terrain effects path.
    pub terrain_effects: Option<String>,
}

/// Physical properties from a `.blueprint.xmb` file.
#[derive(Debug, Clone, Default)]
pub struct Blueprint {
    /// Mass in kg.
    pub mass: Option<f32>,
    /// Friction coefficient.
    pub friction: Option<f32>,
    /// Restitution (bounciness).
    pub restitution: Option<f32>,
    /// Linear damping.
    pub linear_damping: Option<f32>,
    /// Angular damping.
    pub angular_damping: Option<f32>,
    /// Shape reference name.
    pub shape: Option<String>,
}

/// A Havok collision shape from a `.shp.xmb` file.
#[derive(Debug, Clone, Default)]
pub struct Shape {
    /// Havok version string (e.g. `"V_20200_B_20031014"`).
    pub hke_version: Option<String>,
    /// Shape objects.
    pub objects: Vec<HavokObject>,
}

/// A single Havok object (shape primitive).
#[derive(Debug, Clone, Default)]
pub struct HavokObject {
    /// Object name (e.g. `"body"`).
    pub name: String,
    /// Object type (e.g. `"hkBoxShape"`, `"hkConvexVerticesShape"`).
    pub object_type: String,
    /// Parameters as key-value pairs.
    pub params: Vec<HavokParam>,
}

/// A Havok parameter.
#[derive(Debug, Clone, Default)]
pub struct HavokParam {
    /// Parameter name (e.g. `"halfExtents"`, `"radius"`).
    pub name: String,
    /// Parameter type (e.g. `"hkTypeVector4"`, `"hkTypeReal"`).
    pub param_type: String,
    /// Raw value as string.
    pub value: String,
}

/// Parse a `.physics.xmb` document.
pub fn parse_physics(doc: &xmb::Document) -> crate::Result<Physics> {
    let root = expect_root(doc, "physics")?;
    Ok(Physics {
        blueprint: root.child_text("blueprint"),
        thrown_by_projectiles: root.child_bool("ThrownByProjectiles"),
        vehicle: root.child_text("Vehicle"),
        center_offset: root.child_text("CenterOffset"),
        terrain_effects: root.child_text("TerrainEffects"),
    })
}

/// Parse a `.blueprint.xmb` document.
pub fn parse_blueprint(doc: &xmb::Document) -> crate::Result<Blueprint> {
    let root = crate::node_ext::root_node(doc)?;
    Ok(Blueprint {
        mass: root.child_f32("mass"),
        friction: root.child_f32("friction"),
        restitution: root.child_f32("restitution"),
        linear_damping: root.child_f32("linearDamping"),
        angular_damping: root.child_f32("angularDamping"),
        shape: root.child_text("shape"),
    })
}

/// Parse a `.shp.xmb` document (Havok XML shapes).
pub fn parse_shape(doc: &xmb::Document) -> crate::Result<Shape> {
    let root = expect_root(doc, "hke")?;
    let mut shape = Shape {
        hke_version: root.attr_str("version"),
        ..Default::default()
    };

    for obj_node in root.children_named("hkobject") {
        let mut obj = HavokObject {
            name: obj_node.attr_str("name").unwrap_or_default(),
            object_type: obj_node.attr_str("type").unwrap_or_default(),
            ..Default::default()
        };
        for param_node in obj_node.children_named("hkparam") {
            obj.params.push(HavokParam {
                name: param_node.attr_str("name").unwrap_or_default(),
                param_type: param_node.attr_str("type").unwrap_or_default(),
                value: param_node.text_string(),
            });
        }
        shape.objects.push(obj);
    }

    Ok(shape)
}
