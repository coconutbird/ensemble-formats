//! Material chunk (0x704) parser.
//!
//! Reads materials from `BBinaryDataTree` packed document.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use num_traits::ToPrimitive;

use crate::error::{Error, Result};
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
        if child.name.eq_ignore_ascii_case("Material") {
            materials.push(read_material(child)?);
        }
    }

    Ok(materials)
}

/// Read a single material from a `BBinaryDataTree` node.
///
/// Supports two formats:
/// - **Legacy** (HW1 + some HW2): `<Material @Name @Ver>` with `<NameValues>` + `<Maps>` children.
/// - **Hogan** (HW2): `<Material>` with a single `<HoganMaterial>` child containing
///   shader permutations, constant buffer data, and texture paths.
fn read_material(node: &bdt::Node) -> Result<Material> {
    // Check for HW2 Hogan material format before requiring legacy attributes.
    let hogan_node = child_named(node, "HoganMaterial");
    let (name, material_version, data) = if let Some(hogan_node) = hogan_node {
        (
            attribute_named(node, "Name")
                .map(|attribute| attribute.value.to_string_value())
                .unwrap_or_default(),
            attribute_named(node, "Ver").map_or(4, |attribute| variant_to_u32(&attribute.value)),
            MaterialData::Hogan(Box::new(read_hogan_material(hogan_node))),
        )
    } else {
        let name = attribute_named(node, "Name")
            .ok_or(Error::MissingMaterialAttribute("Name"))?
            .value
            .to_string_value();
        let material_version = variant_to_u32(
            &attribute_named(node, "Ver")
                .ok_or(Error::MissingMaterialAttribute("Ver"))?
                .value,
        );
        (
            name,
            material_version,
            MaterialData::Legacy(alloc::boxed::Box::new(read_legacy_material(node))),
        )
    };

    Ok(Material {
        name,
        material_version,
        data,
    })
}

/// Parse the legacy material format (`NameValues` + Maps children).
fn read_legacy_material(node: &bdt::Node) -> LegacyMaterialData {
    let mut legacy = LegacyMaterialData::default();

    if let Some(nv_node) = child_named(node, "NameValues") {
        for prop in &nv_node.children {
            let name = prop.name.as_str();
            if name.eq_ignore_ascii_case("SpecPower") {
                legacy.spec_power = variant_to_f32(&prop.text);
            } else if name.eq_ignore_ascii_case("SpecColorR") {
                legacy.spec_color[0] = variant_to_f32(&prop.text);
            } else if name.eq_ignore_ascii_case("SpecColorG") {
                legacy.spec_color[1] = variant_to_f32(&prop.text);
            } else if name.eq_ignore_ascii_case("SpecColorB") {
                legacy.spec_color[2] = variant_to_f32(&prop.text);
            } else if name.eq_ignore_ascii_case("EnvReflectivity") {
                legacy.env_reflectivity = variant_to_f32(&prop.text);
            } else if name.eq_ignore_ascii_case("EnvSharpness") {
                legacy.env_sharpness = variant_to_f32(&prop.text);
            } else if name.eq_ignore_ascii_case("EnvFresnel") {
                legacy.env_fresnel = variant_to_f32(&prop.text);
            } else if name.eq_ignore_ascii_case("EnvFresnelPower") {
                legacy.env_fresnel_power = variant_to_f32(&prop.text);
            } else if name.eq_ignore_ascii_case("AccessoryIndex") {
                legacy.accessory_index = variant_to_u32(&prop.text);
            } else if name.eq_ignore_ascii_case("Flags") {
                legacy.flags = variant_to_u32(&prop.text);
            } else if name.eq_ignore_ascii_case("BlendType") {
                legacy.blend_type = variant_to_u8(&prop.text);
            } else if name.eq_ignore_ascii_case("Opacity") {
                let raw = variant_to_u32(&prop.text);
                legacy.opacity = f32::from(u8::try_from(raw).unwrap_or(u8::MAX)) / 255.0;
            }
        }
    }

    if let Some(maps_node) = child_named(node, "Maps") {
        for map_type in MapType::ALL {
            if let Some(type_node) = maps_node
                .children
                .iter()
                .find(|child| child.name.eq_ignore_ascii_case(map_type.name()))
            {
                if let Some(uvw_attr) = attribute_named(type_node, "UVWVel")
                    && let bdt::Variant::FloatVec(values) = &uvw_attr.value
                    && values.len() >= 3
                {
                    legacy.uvw_velocity[map_type as usize].copy_from_slice(&values[..3]);
                }

                for map_child in &type_node.children {
                    if map_child.name.eq_ignore_ascii_case("Map") {
                        let mut map = Map::default();
                        if let Some(a) = attribute_named(map_child, "Name") {
                            map.name = a.value.to_string_value();
                        }
                        if let Some(a) = attribute_named(map_child, "Channel") {
                            map.channel = variant_to_i16(&a.value);
                        }
                        if let Some(a) = attribute_named(map_child, "Flags") {
                            map.flags = variant_to_u16(&a.value);
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

fn variant_to_u16(v: &bdt::Variant) -> u16 {
    match v {
        bdt::Variant::UInt(value) => u16::try_from(*value).unwrap_or_default(),
        bdt::Variant::Int(value) => u16::try_from(*value).unwrap_or_default(),
        _ => 0,
    }
}

fn child_named<'a>(node: &'a bdt::Node, name: &str) -> Option<&'a bdt::Node> {
    node.children
        .iter()
        .find(|child| child.name.eq_ignore_ascii_case(name))
}

fn attribute_named<'a>(node: &'a bdt::Node, name: &str) -> Option<&'a bdt::Attribute> {
    node.attributes
        .iter()
        .find(|attribute| attribute.name.eq_ignore_ascii_case(name))
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
/// CB data is stored as a compact BDT "string" node containing raw binary.
/// The BDT reader represents non-text payloads as [`bdt::Variant::Bytes`].
fn variant_to_bytes(v: &bdt::Variant) -> Vec<u8> {
    match v {
        bdt::Variant::String(s) | bdt::Variant::UString(s) => s.as_bytes().to_vec(),
        bdt::Variant::Bytes(bytes) => bytes.clone(),
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

#[cfg(test)]
mod tests {
    use alloc::vec;

    use super::*;

    fn assert_float_array_bits_eq(actual: &[f32; 3], expected: &[f32; 3]) {
        for (actual, expected) in actual.iter().zip(expected) {
            assert_eq!(actual.to_bits(), expected.to_bits());
        }
    }

    fn legacy_material_node() -> bdt::Node {
        let mut material = bdt::Node::new("mAtErIaL");
        material
            .attributes
            .push(bdt::Attribute::with_string("nAmE", "animated"));
        material
            .attributes
            .push(bdt::Attribute::new("vEr", bdt::Variant::UInt(4)));

        let mut maps = bdt::Node::new("mApS");
        let mut diffuse = bdt::Node::new("DiFfUsE");
        diffuse.attributes.push(bdt::Attribute::new(
            "uVwVeL",
            bdt::Variant::FloatVec(vec![0.025, -0.5, 0.125]),
        ));
        let mut map = bdt::Node::new("mAp");
        map.attributes
            .push(bdt::Attribute::with_string("nAmE", "texture.ddx"));
        map.attributes
            .push(bdt::Attribute::new("cHaNnEl", bdt::Variant::Int(1)));
        map.attributes
            .push(bdt::Attribute::new("fLaGs", bdt::Variant::UInt(0x1234)));
        diffuse.children.push(map);
        maps.children.push(diffuse);
        material.children.push(maps);
        material
    }

    #[test]
    fn legacy_parser_matches_engine_case_and_vector_rules() {
        let mut root = bdt::Node::new("Materials");
        root.children.push(bdt::Node::new("Metadata"));
        root.children.push(legacy_material_node());
        let data = bdt::CompactWriter::write(&root).unwrap();

        let materials = read_materials(&data).unwrap();
        assert_eq!(materials.len(), 1);
        assert_eq!(materials[0].name, "animated");
        let legacy = materials[0].legacy().unwrap();
        assert_float_array_bits_eq(
            &legacy.uvw_velocity[MapType::Diffuse as usize],
            &[0.025, -0.5, 0.125],
        );
        let map = &legacy.maps[MapType::Diffuse as usize][0];
        assert_eq!(map.channel, 1);
        assert_eq!(map.flags, 0x1234);
    }

    #[test]
    fn legacy_parser_requires_name_and_version() {
        let mut root = bdt::Node::new("Materials");
        root.children.push(bdt::Node::new("Material"));
        let data = bdt::CompactWriter::write(&root).unwrap();

        assert!(matches!(
            read_materials(&data),
            Err(Error::MissingMaterialAttribute("Name"))
        ));

        let mut material = bdt::Node::new("Material");
        material
            .attributes
            .push(bdt::Attribute::with_string("Name", "missing-version"));
        let mut root = bdt::Node::new("Materials");
        root.children.push(material);
        let data = bdt::CompactWriter::write(&root).unwrap();
        assert!(matches!(
            read_materials(&data),
            Err(Error::MissingMaterialAttribute("Ver"))
        ));
    }

    #[test]
    fn scalar_uvw_velocity_is_ignored_like_the_game() {
        let mut material = legacy_material_node();
        let maps = material.children.first_mut().unwrap();
        let diffuse = maps.children.first_mut().unwrap();
        diffuse.attributes[0].value = bdt::Variant::Float(7.0);
        let parsed = read_material(&material).unwrap();

        assert_float_array_bits_eq(
            &parsed.legacy().unwrap().uvw_velocity[MapType::Diffuse as usize],
            &[0.0; 3],
        );
    }
}
