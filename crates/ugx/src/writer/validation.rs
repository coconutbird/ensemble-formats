//! Preflight checks for game-compatible UGX serialization.

use alloc::format;

use crate::error::{Error, Result};
use crate::types::{MaterialData, UgxGeom, UgxVersion};

pub(super) fn validate_for_write(geom: &UgxGeom, version: UgxVersion) -> Result<()> {
    crate::checked_u32(geom.vertex_buffer.len(), "vertex-buffer chunk size")?;
    let index_bytes = geom
        .index_buffer
        .len()
        .checked_mul(core::mem::size_of::<u16>())
        .ok_or(Error::SizeOverflow("index-buffer chunk size"))?;
    crate::checked_u32(index_bytes, "index-buffer chunk size")?;

    validate_parallel_bone_data(geom)?;
    validate_materials(geom, version)?;
    for (section_index, section) in geom.sections.iter().enumerate() {
        validate_section(geom, version, section_index, section)?;
    }
    validate_accessories(geom)?;
    Ok(())
}

fn validate_parallel_bone_data(geom: &UgxGeom) -> Result<()> {
    if !geom.sections.is_empty() && geom.bones.is_empty() {
        return Err(Error::UnsupportedFormat(
            "geometry with sections requires cached and Granny bone data".into(),
        ));
    }
    if geom.bones.len() != geom.granny_bones.len() {
        return Err(Error::UnsupportedFormat(format!(
            "cached bone count {} does not match Granny bone count {}",
            geom.bones.len(),
            geom.granny_bones.len()
        )));
    }
    if geom.bones.len() != geom.bone_bounds.len() {
        return Err(Error::UnsupportedFormat(format!(
            "cached bone count {} does not match bone-bound count {}",
            geom.bones.len(),
            geom.bone_bounds.len()
        )));
    }
    Ok(())
}

fn validate_materials(geom: &UgxGeom, version: UgxVersion) -> Result<()> {
    if version != UgxVersion::Hw1 {
        return Ok(());
    }

    for (index, material) in geom.materials.iter().enumerate() {
        if material.material_version != 4 {
            return Err(Error::UnsupportedFormat(format!(
                "HW1 material {index} has version {}, expected 4",
                material.material_version
            )));
        }
        if !matches!(material.data, MaterialData::Legacy(_)) {
            return Err(Error::UnsupportedFormat(format!(
                "HW1 material {index} uses the HW2 Hogan material format"
            )));
        }
    }
    Ok(())
}

fn validate_section(
    geom: &UgxGeom,
    version: UgxVersion,
    section_index: usize,
    section: &crate::Section,
) -> Result<()> {
    let material_index = usize::try_from(section.material_index).map_err(|_| {
        Error::UnsupportedFormat(format!(
            "section {section_index} has negative material index {}",
            section.material_index
        ))
    })?;
    if material_index >= geom.materials.len() {
        return Err(Error::UnsupportedFormat(format!(
            "section {section_index} references material {material_index}, but only {} materials exist",
            geom.materials.len()
        )));
    }

    let index_offset = encoded_usize(section.ib_offset, section_index, "index-buffer offset")?;
    let triangle_count = encoded_usize(section.num_tris, section_index, "triangle count")?;
    let index_count = triangle_count
        .checked_mul(3)
        .ok_or(Error::SizeOverflow("section index count"))?;
    let index_end = index_offset
        .checked_add(index_count)
        .ok_or(Error::SizeOverflow("section index-buffer range"))?;
    if index_end > geom.index_buffer.len() {
        return Err(Error::UnsupportedFormat(format!(
            "section {section_index} index-buffer range ends at {index_end}, but the buffer has {} indices",
            geom.index_buffer.len()
        )));
    }

    let vertex_offset = encoded_usize(section.vb_offset, section_index, "vertex-buffer offset")?;
    let vertex_bytes = encoded_usize(section.vb_bytes, section_index, "vertex-buffer byte count")?;
    let vertex_end = vertex_offset
        .checked_add(vertex_bytes)
        .ok_or(Error::SizeOverflow("section vertex-buffer range"))?;
    if vertex_end > geom.vertex_buffer.len() {
        return Err(Error::UnsupportedFormat(format!(
            "section {section_index} vertex-buffer range ends at {vertex_end}, but the buffer has {} bytes",
            geom.vertex_buffer.len()
        )));
    }

    let vertex_count = encoded_usize(section.num_verts, section_index, "vertex count")?;
    let vertex_stride = encoded_usize(section.vert_size, section_index, "vertex stride")?;
    if vertex_count != 0 && vertex_stride == 0 {
        return Err(Error::UnsupportedFormat(format!(
            "section {section_index} has vertices but a zero vertex stride"
        )));
    }
    let required_vertex_bytes = vertex_count
        .checked_mul(vertex_stride)
        .ok_or(Error::SizeOverflow("section vertex byte count"))?;
    if required_vertex_bytes > vertex_bytes {
        return Err(Error::UnsupportedFormat(format!(
            "section {section_index} needs {required_vertex_bytes} vertex bytes but declares {vertex_bytes}"
        )));
    }

    if version == UgxVersion::Hw1 {
        let packer = section.base_vert_packer.as_ref().ok_or_else(|| {
            Error::UnsupportedFormat(format!(
                "HW1 section {section_index} is missing its vertex packer"
            ))
        })?;
        if packer.pack_order.is_empty() {
            return Err(Error::UnsupportedFormat(format!(
                "HW1 section {section_index} has an empty vertex pack order"
            )));
        }
        if packer.vertex_size() != vertex_stride {
            return Err(Error::UnsupportedFormat(format!(
                "HW1 section {section_index} vertex stride {vertex_stride} does not match its packer stride {}",
                packer.vertex_size()
            )));
        }
    } else if let Some(packer) = &section.external_vert_packer {
        if packer.pack_order.is_empty() {
            return Err(Error::UnsupportedFormat(format!(
                "HW2 section {section_index} has an empty external vertex pack order"
            )));
        }
        if packer.vertex_size() != vertex_stride {
            return Err(Error::UnsupportedFormat(format!(
                "HW2 section {section_index} vertex stride {vertex_stride} does not match its external packer stride {}",
                packer.vertex_size()
            )));
        }
    }

    Ok(())
}

fn validate_accessories(geom: &UgxGeom) -> Result<()> {
    for (position, &encoded_index) in geom.valid_accessories.iter().enumerate() {
        let index = usize::try_from(encoded_index).map_err(|_| {
            Error::UnsupportedFormat(format!(
                "valid-accessory entry {position} is negative ({encoded_index})"
            ))
        })?;
        if index >= geom.accessories.len() {
            return Err(Error::UnsupportedFormat(format!(
                "valid-accessory entry {position} references {index}, but only {} accessories exist",
                geom.accessories.len()
            )));
        }
    }
    Ok(())
}

fn encoded_usize(value: i32, section_index: usize, field: &'static str) -> Result<usize> {
    usize::try_from(value).map_err(|_| {
        Error::UnsupportedFormat(format!(
            "section {section_index} has negative {field} ({value})"
        ))
    })
}
