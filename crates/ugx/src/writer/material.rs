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

    let data = bdt::CompactWriter::write(&root)?;
    Ok(data)
}

/// Build a single material BDT node.
fn build_material_node(mat: &Material) -> bdt::Node {
    let mut node = bdt::Node::new("Material");
    node.attributes
        .push(bdt::Attribute::with_string("Name", &mat.name));
    node.attributes
        .push(bdt::Attribute::new("Ver", bdt::Variant::UInt(4)));

    // NameValues child with all material properties (matching engine order)
    let mut nv = bdt::Node::new("NameValues");

    fn push_float(nv: &mut bdt::Node, name: &str, val: f32) {
        let mut n = bdt::Node::new(name);
        n.text = bdt::Variant::Float(val);
        nv.children.push(n);
    }
    fn push_uint(nv: &mut bdt::Node, name: &str, val: u32) {
        let mut n = bdt::Node::new(name);
        n.text = bdt::Variant::UInt(val);
        nv.children.push(n);
    }

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
    push_uint(&mut nv, "BlendType", mat.blend_type as u32);
    push_uint(&mut nv, "Opacity", (mat.opacity * 255.0) as u32);

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
