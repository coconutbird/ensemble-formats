//! Material chunk (0x704) builder.
//!
//! Serializes materials as a BBinaryDataTree (BDT) packed document.

use alloc::vec::Vec;

use crate::error::Result;
use crate::types::{MapType, Material, UgxGeom};

/// Build the material chunk (0x704) as a BBinaryDataTree packed document.
///
/// Tree structure:
/// ```text
/// <Materials>
///   <Material @Name="name" @Ver=4>
///     <NameValues>
///       <SpecPower> text=Float(...)
///       <Flags> text=UInt(...)
///       <BlendType> text=UInt(...)
///       <Opacity> text=UInt(0-255)
///     <Maps>
///       <diffuse @UVWVel=Float(0.0)>
///         <Map @Name="texture_path" @Channel=Int(0) @Flags=UInt(7)>
///       ...
/// ```
pub(super) fn build_material_data(geom: &UgxGeom) -> Result<Vec<u8>> {
    let mut root = bdt::Node::new("Materials");

    for mat in &geom.materials {
        root.children.push(build_material_node(mat));
    }

    let data = bdt::Writer::write(&root, bdt::Endian::Little)?;
    Ok(data)
}

/// Build a single material BDT node.
fn build_material_node(mat: &Material) -> bdt::Node {
    let mut node = bdt::Node::new("Material");
    node.attributes
        .push(bdt::Attribute::with_string("Name", &mat.name));
    node.attributes
        .push(bdt::Attribute::new("Ver", bdt::Variant::Int(4)));

    // NameValues child with material properties
    let mut nv = bdt::Node::new("NameValues");

    let mut spec_node = bdt::Node::new("SpecPower");
    spec_node.text = bdt::Variant::Float(mat.spec_power);
    nv.children.push(spec_node);

    let mut flags_node = bdt::Node::new("Flags");
    flags_node.text = bdt::Variant::UInt(mat.flags);
    nv.children.push(flags_node);

    let mut blend_node = bdt::Node::new("BlendType");
    blend_node.text = bdt::Variant::UInt(mat.blend_type as u32);
    nv.children.push(blend_node);

    let mut opacity_node = bdt::Node::new("Opacity");
    opacity_node.text = bdt::Variant::UInt((mat.opacity * 255.0) as u32);
    nv.children.push(opacity_node);

    node.children.push(nv);

    // Maps child with texture map slots
    let mut maps = bdt::Node::new("Maps");

    for map_type in MapType::ALL {
        let idx = map_type as usize;
        let uvw = mat.uvw_velocity[idx];
        let has_maps = !mat.maps[idx].is_empty();
        let has_uvw = uvw[0] != 0.0 || uvw[1] != 0.0 || uvw[2] != 0.0;

        // Only write map type nodes that have data
        if !has_maps && !has_uvw {
            continue;
        }

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
                bdt::Variant::Int(map.channel as i32),
            ));
            map_node.attributes.push(bdt::Attribute::new(
                "Flags",
                bdt::Variant::UInt(map.flags as u32),
            ));
            type_node.children.push(map_node);
        }

        maps.children.push(type_node);
    }

    node.children.push(maps);

    node
}
