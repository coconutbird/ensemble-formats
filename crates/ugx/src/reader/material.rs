//! Material chunk (0x704) parser.
//!
//! Reads materials from BBinaryDataTree packed document.

use alloc::vec::Vec;

use crate::error::Result;
use crate::types::{Map, MapType, Material};

/// Read materials from BBinaryDataTree packed document (chunk 0x704).
///
/// The root node's children are individual material nodes. Each material
/// has a "Name" attribute, map type children (Diffuse, Normal, etc.),
/// UVW velocity children, and a Properties child (BNameValueMap).
pub(crate) fn read_materials(data: &[u8]) -> Result<Vec<Material>> {
    let root = match bdt::Reader::read(data, bdt::Endian::Little)? {
        Some(root) => root,
        None => return Ok(Vec::new()),
    };

    let mut materials = Vec::with_capacity(root.children.len());
    for child in &root.children {
        materials.push(read_material(child));
    }

    Ok(materials)
}

/// Read a single material from a BBinaryDataTree node.
fn read_material(node: &bdt::Node) -> Material {
    let mut mat = Material::default();

    // Name from attribute
    if let Some(attr) = node.get_attribute("Name") {
        mat.name = attr.value.to_string_value();
    }

    // Read properties from "NameValues" child
    if let Some(nv_node) = node.children.iter().find(|c| c.name == "NameValues") {
        for prop in &nv_node.children {
            match prop.name.as_str() {
                "SpecPower" => mat.spec_power = variant_to_f32(&prop.text),
                "SpecColorR" => mat.spec_color[0] = variant_to_f32(&prop.text),
                "SpecColorG" => mat.spec_color[1] = variant_to_f32(&prop.text),
                "SpecColorB" => mat.spec_color[2] = variant_to_f32(&prop.text),
                "EnvReflectivity" => mat.env_reflectivity = variant_to_f32(&prop.text),
                "EnvSharpness" => mat.env_sharpness = variant_to_f32(&prop.text),
                "EnvFresnel" => mat.env_fresnel = variant_to_f32(&prop.text),
                "EnvFresnelPower" => mat.env_fresnel_power = variant_to_f32(&prop.text),
                "AccessoryIndex" => mat.accessory_index = variant_to_u32(&prop.text),
                "Flags" => mat.flags = variant_to_u32(&prop.text),
                "BlendType" => mat.blend_type = variant_to_u8(&prop.text),
                "Opacity" => {
                    let raw = variant_to_u32(&prop.text);
                    mat.opacity = raw as f32 / 255.0;
                }
                _ => {}
            }
        }
    }

    // Read maps from "Maps" child
    if let Some(maps_node) = node.children.iter().find(|c| c.name == "Maps") {
        for map_type in MapType::ALL {
            if let Some(type_node) = maps_node
                .children
                .iter()
                .find(|c| c.name == map_type.name())
            {
                // UVWVel is an attribute on the map type node
                if let Some(uvw_attr) = type_node.get_attribute("UVWVel") {
                    mat.uvw_velocity[map_type as usize][0] = variant_to_f32(&uvw_attr.value);
                }

                // Each <Map> child is a texture reference
                for map_child in &type_node.children {
                    if map_child.name == "Map" {
                        let mut map = Map::default();
                        if let Some(a) = map_child.get_attribute("Name") {
                            map.name = a.value.to_string_value();
                        }
                        if let Some(a) = map_child.get_attribute("Channel") {
                            map.channel = variant_to_i16(&a.value);
                        }
                        if let Some(a) = map_child.get_attribute("Flags") {
                            map.flags = variant_to_u8(&a.value);
                        }
                        mat.maps[map_type as usize].push(map);
                    }
                }
            }
        }
    }

    mat
}

fn variant_to_f32(v: &bdt::Variant) -> f32 {
    match v {
        bdt::Variant::Float(f) => *f,
        bdt::Variant::Double(d) => *d as f32,
        bdt::Variant::Int(i) => *i as f32,
        bdt::Variant::UInt(u) => *u as f32,
        bdt::Variant::Fract24(f) => *f,
        _ => 0.0,
    }
}

fn variant_to_u32(v: &bdt::Variant) -> u32 {
    match v {
        bdt::Variant::UInt(u) => *u,
        bdt::Variant::Int(i) => *i as u32,
        _ => 0,
    }
}

fn variant_to_i16(v: &bdt::Variant) -> i16 {
    match v {
        bdt::Variant::Int(i) => *i as i16,
        bdt::Variant::UInt(u) => *u as i16,
        _ => 0,
    }
}

fn variant_to_u8(v: &bdt::Variant) -> u8 {
    match v {
        bdt::Variant::UInt(u) => *u as u8,
        bdt::Variant::Int(i) => *i as u8,
        _ => 0,
    }
}
