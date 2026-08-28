//! Material chunk (0x704) parser.
//!
//! Reads materials from `BBinaryDataTree` packed document.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use num_traits::ToPrimitive;

use crate::error::Result;
use crate::types::{
    HoganMaterialData, LegacyMaterialData, Map, MapType, Material, MaterialData, ShaderPermutation,
};

/// Read materials from `BBinaryDataTree` packed document (chunk 0x704).
///
/// The root node's children are individual material nodes. Each material
/// has a "Name" attribute, map type children (Diffuse, Normal, etc.),
/// UVW velocity children, and a Properties child (`BNameValueMap`).
pub(crate) fn read_materials(data: &[u8]) -> Result<Vec<Material>> {
    let Some(root) = bdt::Reader::read(data, bdt::Endian::Little)? else {
        return Ok(Vec::new());
    };

    let mut materials = Vec::with_capacity(root.children.len());
    for child in &root.children {
        materials.push(read_material(child));
    }

    Ok(materials)
}

/// Read a single material from a `BBinaryDataTree` node.
///
/// Supports two formats:
/// - **Legacy** (HW1 + some HW2): `<Material @Name @Ver>` with `<NameValues>` + `<Maps>` children.
/// - **Hogan** (HW2): `<Material>` with a single `<HoganMaterial>` child containing
///   shader permutations, constant buffer data, and texture paths.
fn read_material(node: &bdt::Node) -> Material {
    // Name from attribute (legacy format; absent for Hogan)
    let name = node
        .get_attribute("Name")
        .map(|a| a.value.to_string_value())
        .unwrap_or_default();

    // Ver from attribute (4 = HW1, 5 = HW2 legacy; absent for Hogan)
    let material_version = node
        .get_attribute("Ver")
        .map_or(4, |a| variant_to_u32(&a.value));

    // Check for HW2 Hogan material format
    let data = if let Some(hogan_node) = node.children.iter().find(|c| c.name == "HoganMaterial") {
        MaterialData::Hogan(Box::new(read_hogan_material(hogan_node)))
    } else {
        MaterialData::Legacy(alloc::boxed::Box::new(read_legacy_material(node)))
    };

    Material {
        name,
        material_version,
        data,
    }
}

/// Parse the legacy material format (`NameValues` + Maps children).
fn read_legacy_material(node: &bdt::Node) -> LegacyMaterialData {
    let mut legacy = LegacyMaterialData::default();

    if let Some(nv_node) = node.children.iter().find(|c| c.name == "NameValues") {
        for prop in &nv_node.children {
            match prop.name.as_str() {
                "SpecPower" => legacy.spec_power = variant_to_f32(&prop.text),
                "SpecColorR" => legacy.spec_color[0] = variant_to_f32(&prop.text),
                "SpecColorG" => legacy.spec_color[1] = variant_to_f32(&prop.text),
                "SpecColorB" => legacy.spec_color[2] = variant_to_f32(&prop.text),
                "EnvReflectivity" => legacy.env_reflectivity = variant_to_f32(&prop.text),
                "EnvSharpness" => legacy.env_sharpness = variant_to_f32(&prop.text),
                "EnvFresnel" => legacy.env_fresnel = variant_to_f32(&prop.text),
                "EnvFresnelPower" => legacy.env_fresnel_power = variant_to_f32(&prop.text),
                "AccessoryIndex" => legacy.accessory_index = variant_to_u32(&prop.text),
                "Flags" => legacy.flags = variant_to_u32(&prop.text),
                "BlendType" => legacy.blend_type = variant_to_u8(&prop.text),
                "Opacity" => {
                    let raw = variant_to_u32(&prop.text);
                    legacy.opacity = f32::from(u8::try_from(raw).unwrap_or(u8::MAX)) / 255.0;
                }
                _ => {}
            }
        }
    }

    if let Some(maps_node) = node.children.iter().find(|c| c.name == "Maps") {
        for map_type in MapType::ALL {
            if let Some(type_node) = maps_node
                .children
                .iter()
                .find(|c| c.name == map_type.name())
            {
                if let Some(uvw_attr) = type_node.get_attribute("UVWVel") {
                    legacy.uvw_velocity[map_type as usize][0] = variant_to_f32(&uvw_attr.value);
                }

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
                        legacy.maps[map_type as usize].push(map);
                    }
                }
            }
        }
    }

    legacy
}

/// Parse HW2 Hogan material data from a `<HoganMaterial>` BDT node.
fn read_hogan_material(node: &bdt::Node) -> HoganMaterialData {
    let mut perms = Vec::new();
    // Read up to 4 shader permutation pairs (name0/hash0 .. name3/hash3)
    for i in 0..4 {
        let name_key = alloc::format!("shaderPermutationName{i}");
        let hash_key = alloc::format!("shaderPermutationHash{i}");
        if let (Some(name_attr), Some(hash_attr)) =
            (node.get_attribute(&name_key), node.get_attribute(&hash_key))
        {
            perms.push(ShaderPermutation {
                name: name_attr.value.to_string_value(),
                hash: variant_to_u32(&hash_attr.value),
            });
        }
    }

    let ufx_version = node
        .get_attribute("ufxVersion")
        .map_or(9, |a| variant_to_u32(&a.value));
    let blend_mode = node
        .get_attribute("blendMode")
        .map_or(0, |a| variant_to_u32(&a.value));
    let shadow_requires_consts = node
        .get_attribute("shadowRequiresConsts")
        .is_some_and(|a| variant_to_bool(&a.value));
    let skinned = node
        .get_attribute("skinned")
        .is_some_and(|a| variant_to_bool(&a.value));
    let terrain_blending = node
        .get_attribute("terrainBlending")
        .is_some_and(|a| variant_to_bool(&a.value));

    let mut vs_cb_data = Vec::new();
    let mut ps_cb_data = Vec::new();
    let mut hs_cb_data = Vec::new();
    let mut ds_cb_data = Vec::new();
    let mut gs_cb_data = Vec::new();
    let mut textures = String::new();

    for child in &node.children {
        match child.name.as_str() {
            "VSCBData" => vs_cb_data = variant_to_bytes(&child.text),
            "PSCBData" => ps_cb_data = variant_to_bytes(&child.text),
            "HSCBData" => hs_cb_data = variant_to_bytes(&child.text),
            "DSCBData" => ds_cb_data = variant_to_bytes(&child.text),
            "GSCBData" => gs_cb_data = variant_to_bytes(&child.text),
            "textures" => textures = child.text.to_string_value(),
            _ => {}
        }
    }

    HoganMaterialData {
        shader_permutations: perms,
        ufx_version,
        blend_mode,
        shadow_requires_consts,
        skinned,
        terrain_blending,
        vs_cb_data,
        ps_cb_data,
        hs_cb_data,
        ds_cb_data,
        gs_cb_data,
        textures,
    }
}

fn variant_to_f32(v: &bdt::Variant) -> f32 {
    match v {
        bdt::Variant::Float(value) | bdt::Variant::Fract24(value) => *value,
        bdt::Variant::Double(value) => value.to_f32().unwrap_or_default(),
        bdt::Variant::Int(value) => value.to_f32().unwrap_or_default(),
        bdt::Variant::UInt(value) => value.to_f32().unwrap_or_default(),
        _ => 0.0,
    }
}

fn variant_to_u32(v: &bdt::Variant) -> u32 {
    match v {
        bdt::Variant::UInt(u) => *u,
        bdt::Variant::Int(value) => u32::try_from(*value).unwrap_or_default(),
        _ => 0,
    }
}

fn variant_to_i16(v: &bdt::Variant) -> i16 {
    match v {
        bdt::Variant::Int(value) => i16::try_from(*value).unwrap_or_default(),
        bdt::Variant::UInt(value) => i16::try_from(*value).unwrap_or_default(),
        _ => 0,
    }
}

fn variant_to_u8(v: &bdt::Variant) -> u8 {
    match v {
        bdt::Variant::UInt(value) => u8::try_from(*value).unwrap_or_default(),
        bdt::Variant::Int(value) => u8::try_from(*value).unwrap_or_default(),
        _ => 0,
    }
}

fn variant_to_bool(v: &bdt::Variant) -> bool {
    match v {
        bdt::Variant::Bool(b) => *b,
        bdt::Variant::UInt(u) => *u != 0,
        bdt::Variant::Int(i) => *i != 0,
        _ => false,
    }
}

/// Extract raw bytes from a BDT variant used for constant buffer data.
///
/// CB data is stored as a BDT "string" node containing raw binary. Because
/// the BDT reader decodes strings via `from_utf8_lossy`, non-UTF-8 binary
/// data may have been corrupted. A future `Bytes` variant would fix this.
fn variant_to_bytes(v: &bdt::Variant) -> Vec<u8> {
    match v {
        bdt::Variant::String(s) | bdt::Variant::UString(s) => s.as_bytes().to_vec(),
        bdt::Variant::UInt(u) => {
            if *u == 0 {
                Vec::new()
            } else {
                u.to_le_bytes().to_vec()
            }
        }
        _ => Vec::new(),
    }
}
