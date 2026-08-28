//! Material chunk (0x704) builder.
//!
//! Serializes materials as a `BBinaryDataTree` (BDT) packed document.
//! Supports both HW1 legacy format and HW2 Hogan shader-based format.

use alloc::format;
use alloc::vec::Vec;
use num_traits::ToPrimitive;

use crate::error::Result;
use crate::types::{MapType, Material, MaterialData, UgxGeom};

/// Append a floating-point name/value child.
fn push_float(parent: &mut bdt::Node, name: &str, value: f32) {
    let mut node = bdt::Node::new(name);
    node.text = bdt::Variant::Float(value);
    parent.children.push(node);
}

/// Append an unsigned-integer name/value child.
fn push_uint(parent: &mut bdt::Node, name: &str, value: u32) {
    let mut node = bdt::Node::new(name);
    node.text = bdt::Variant::UInt(value);
    parent.children.push(node);
}

/// Append a Hogan constant-buffer child node.
fn push_cb_node(parent: &mut bdt::Node, name: &str, data: &[u8]) {
    let mut node = bdt::Node::new(name);
    if data.is_empty() {
        node.text = bdt::Variant::UInt(0);
    } else {
        node.text = bdt::Variant::String(alloc::string::String::from_utf8_lossy(data).into_owned());
    }
    parent.children.push(node);
}

/// Build the material chunk (0x704) as a `BBinaryDataTree` packed document.
///
/// Tree structure (legacy):
/// ```text
/// <Materials>
///   <Material @Name="name" @Ver=4>
///     <NameValues>
///       <SpecPower> text=Float(...)
///       ...
///     <Maps>
///       <diffuse @UVWVel=Float(0.0)>
///         <Map @Name="texture_path" @Channel=Int(0) @Flags=UInt(7)>
///       ...
/// ```
///
/// Tree structure (Hogan / HW2):
/// ```text
/// <Materials>
///   <Material>
///     <HoganMaterial @shaderPermutationName0=... @ufxVersion=9 ...>
///       <VSCBData text=UInt(0)>
///       <PSCBData text=UInt(0)>
///       <textures text=String("...")>
/// ```
pub(super) fn build_material_data(geom: &UgxGeom) -> Result<Vec<u8>> {
    let mut root = bdt::Node::new("Materials");

    for mat in &geom.materials {
        root.children.push(build_material_node(mat));
    }

    let data = bdt::CompactWriter::write(&root)?;
    Ok(data)
}

/// Build a single material BDT node.
///
/// Matches on `MaterialData` to write either the Hogan or legacy format.
fn build_material_node(mat: &Material) -> bdt::Node {
    let mut node = bdt::Node::new("Material");

    match &mat.data {
        MaterialData::Hogan(hogan) => {
            // HW2 Hogan format: <Material> with <HoganMaterial> child.
            // Some HW2 files carry @Name on the <Material> node — preserve it.
            if !mat.name.is_empty() {
                node.attributes
                    .push(bdt::Attribute::with_string("Name", &mat.name));
            }
            node.children.push(build_hogan_node(hogan));
        }
        MaterialData::Legacy(legacy) => {
            // Legacy format: <Material @Name @Ver> with <NameValues> + <Maps>
            node.attributes
                .push(bdt::Attribute::with_string("Name", &mat.name));
            node.attributes.push(bdt::Attribute::new(
                "Ver",
                bdt::Variant::UInt(mat.material_version),
            ));
            build_legacy_children(legacy, &mut node);
        }
    }

    node
}

/// Build legacy `NameValues` + Maps children for a material node.
fn build_legacy_children(mat: &crate::types::LegacyMaterialData, node: &mut bdt::Node) {
    let mut nv = bdt::Node::new("NameValues");

    push_float(&mut nv, "SpecPower", mat.spec_power);
    push_float(&mut nv, "SpecColorR", mat.spec_color[0]);
    push_float(&mut nv, "SpecColorG", mat.spec_color[1]);
    push_float(&mut nv, "SpecColorB", mat.spec_color[2]);
    push_float(&mut nv, "EnvReflectivity", mat.env_reflectivity);
    push_float(&mut nv, "EnvSharpness", mat.env_sharpness);
    push_float(&mut nv, "EnvFresnel", mat.env_fresnel);
    push_float(&mut nv, "EnvFresnelPower", mat.env_fresnel_power);
    push_uint(&mut nv, "AccessoryIndex", mat.accessory_index);
    push_uint(&mut nv, "Flags", mat.flags);
    push_uint(&mut nv, "BlendType", u32::from(mat.blend_type));
    let opacity = (mat.opacity.clamp(0.0, 1.0) * 255.0)
        .round()
        .to_u32()
        .unwrap_or_default();
    push_uint(&mut nv, "Opacity", opacity);

    node.children.push(nv);

    // Maps child — always write all 13 map type nodes (matching engine)
    let mut maps = bdt::Node::new("Maps");

    for map_type in MapType::ALL {
        let idx = map_type as usize;
        let uvw = mat.uvw_velocity[idx];

        let mut type_node = bdt::Node::new(map_type.name());
        type_node
            .attributes
            .push(bdt::Attribute::new("UVWVel", bdt::Variant::Float(uvw[0])));

        for map in &mat.maps[idx] {
            let mut map_node = bdt::Node::new("Map");
            map_node
                .attributes
                .push(bdt::Attribute::with_string("Name", &map.name));
            map_node.attributes.push(bdt::Attribute::new(
                "Channel",
                bdt::Variant::Int(i32::from(map.channel)),
            ));
            map_node.attributes.push(bdt::Attribute::new(
                "Flags",
                bdt::Variant::UInt(u32::from(map.flags)),
            ));
            type_node.children.push(map_node);
        }

        maps.children.push(type_node);
    }

    node.children.push(maps);
}

/// Build a `<HoganMaterial>` BDT node from HW2 Hogan data.
fn build_hogan_node(hogan: &crate::types::HoganMaterialData) -> bdt::Node {
    let mut node = bdt::Node::new("HoganMaterial");

    // Shader permutation attributes (name0/hash0 .. name3/hash3)
    for (i, perm) in hogan.shader_permutations.iter().enumerate() {
        node.attributes.push(bdt::Attribute::with_string(
            format!("shaderPermutationName{i}"),
            &perm.name,
        ));
        node.attributes.push(bdt::Attribute::new(
            format!("shaderPermutationHash{i}"),
            bdt::Variant::UInt(perm.hash),
        ));
    }

    node.attributes.push(bdt::Attribute::new(
        "ufxVersion",
        bdt::Variant::UInt(hogan.ufx_version),
    ));
    node.attributes.push(bdt::Attribute::new(
        "blendMode",
        bdt::Variant::UInt(hogan.blend_mode),
    ));
    node.attributes.push(bdt::Attribute::new(
        "shadowRequiresConsts",
        bdt::Variant::Bool(hogan.shadow_requires_consts),
    ));
    node.attributes.push(bdt::Attribute::new(
        "skinned",
        bdt::Variant::Bool(hogan.skinned),
    ));
    node.attributes.push(bdt::Attribute::new(
        "terrainBlending",
        bdt::Variant::Bool(hogan.terrain_blending),
    ));

    push_cb_node(&mut node, "VSCBData", &hogan.vs_cb_data);
    push_cb_node(&mut node, "PSCBData", &hogan.ps_cb_data);
    push_cb_node(&mut node, "HSCBData", &hogan.hs_cb_data);
    push_cb_node(&mut node, "DSCBData", &hogan.ds_cb_data);
    push_cb_node(&mut node, "GSCBData", &hogan.gs_cb_data);

    let mut tex = bdt::Node::new("textures");
    tex.text = bdt::Variant::String(hogan.textures.clone());
    node.children.push(tex);

    node
}
