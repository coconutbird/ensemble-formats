//! Granny bones chunk (0x703) builder.
//!
//! Produces a Granny2-compatible serialized chunk with file info header,
//! skeleton, bone array, mesh structs, bone bindings, string table,
//! and bone `ExtendedData` (type definitions + variant data).

mod extended_data;
mod fallback;
mod type_tree;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use nostdio::{Cursor, Seek, SeekFrom, WriteLe};

use crate::constants::{
    GRANNY_BONE_BINDING_SIZE, GRANNY_BONE_EXTENDED_DATA_OFFSET, GRANNY_BONE_SIZE, GRANNY_MESH_SIZE,
};
use crate::error::{Error, Result};
use crate::types::{GrannyBoneBinding, UgxGeom};

use extended_data::{emit_type_def_array, emit_variant_data, type_defs_equal};
use fallback::compute_fallback_local_transforms;
use type_tree::build_file_info_type_tree;

/// Align to 16-byte boundary (matching original engine alignment).
fn align16(value: usize) -> Result<usize> {
    value
        .checked_add(15)
        .map(|aligned| aligned & !15)
        .ok_or(Error::SizeOverflow("Granny alignment"))
}

/// Compute the end of a fixed-stride array using checked arithmetic.
fn array_end(start: usize, count: usize, stride: usize, context: &'static str) -> Result<usize> {
    count
        .checked_mul(stride)
        .and_then(|size| start.checked_add(size))
        .ok_or(Error::SizeOverflow(context))
}

/// Patch raw bytes into a previously allocated output range.
fn patch_bytes(buf: &mut [u8], position: usize, bytes: &[u8], context: &'static str) -> Result<()> {
    let end = position
        .checked_add(bytes.len())
        .ok_or(Error::SizeOverflow(context))?;
    buf.get_mut(position..end)
        .ok_or_else(|| Error::UnexpectedEof {
            context: String::from(context),
        })?
        .copy_from_slice(bytes);
    Ok(())
}

/// Patch a target-sized offset as a little-endian Granny pointer.
fn patch_pointer(buf: &mut [u8], position: usize, target: usize) -> Result<()> {
    patch_bytes(
        buf,
        position,
        &u64::try_from(target)
            .map_err(|_| Error::SizeOverflow("Granny pointer"))?
            .to_le_bytes(),
        "Granny pointer",
    )
}

/// Build the mesh names and binding lists used by the serialized mesh phase.
fn collect_mesh_data(geom: &UgxGeom) -> (Vec<Vec<GrannyBoneBinding>>, Vec<String>) {
    if !geom.granny_meshes.is_empty() {
        return (
            geom.granny_meshes
                .iter()
                .map(|mesh| mesh.bone_bindings.clone())
                .collect(),
            geom.granny_meshes
                .iter()
                .map(|mesh| mesh.name.clone())
                .collect(),
        );
    }

    let bindings = geom
        .sections
        .iter()
        .map(|section| {
            if section.bone_remap.is_empty() {
                usize::try_from(section.rigid_bone_index)
                    .ok()
                    .and_then(|index| geom.granny_bones.get(index))
                    .map_or_else(Vec::new, |bone| {
                        vec![GrannyBoneBinding {
                            bone_name: bone.name.clone(),
                            ..Default::default()
                        }]
                    })
            } else {
                section
                    .bone_remap
                    .iter()
                    .filter_map(|&index| geom.granny_bones.get(usize::from(index)))
                    .map(|bone| GrannyBoneBinding {
                        bone_name: bone.name.clone(),
                        ..Default::default()
                    })
                    .collect()
            }
        })
        .collect();
    let names = (0..geom.sections.len())
        .map(|index| format!("mesh_{index}"))
        .collect();
    (bindings, names)
}

/// Write the file header, skeleton structure, and fixed bone array.
fn write_skeleton_and_bones(
    geom: &UgxGeom,
    buf: &mut Vec<u8>,
    strings: &mut super::string_table::StringTable,
    skeleton_pointer_array: usize,
    skeleton_structure: usize,
    bones_start: usize,
) -> Result<()> {
    use crate::types::raw::{GrannyBoneRaw, GrannySkeletonRaw};
    use zerocopy::IntoBytes;

    strings.add(0x10, "gr2ugx".to_string());
    let mut cursor = Cursor::new(&mut *buf);
    cursor.seek(SeekFrom::Start(0x30))?;
    cursor.write_u32_le(1)?;
    cursor.write_u64_le(
        u64::try_from(skeleton_pointer_array)
            .map_err(|_| Error::SizeOverflow("skeleton pointer array"))?,
    )?;

    patch_pointer(buf, skeleton_pointer_array, skeleton_structure)?;
    strings.add(skeleton_structure, "GrannyRootBone".to_string());
    let skeleton = GrannySkeletonRaw {
        name_ptr: [0; 8],
        bone_count: crate::checked_u32(geom.granny_bones.len(), "Granny bone count")?.to_le_bytes(),
        bones_ptr: u64::try_from(bones_start)
            .map_err(|_| Error::SizeOverflow("Granny bone-array pointer"))?
            .to_le_bytes(),
        lod_type: geom.skeleton_lod_type.to_le_bytes(),
        _pad: [0; 16],
    };
    patch_bytes(
        buf,
        skeleton_structure,
        skeleton.as_bytes(),
        "Granny skeleton",
    )?;

    let fallback_transforms = geom
        .granny_bones
        .iter()
        .any(|bone| bone.local_transform.is_none())
        .then(|| compute_fallback_local_transforms(geom));

    for (bone_index, bone) in geom.granny_bones.iter().enumerate() {
        let base = array_end(
            bones_start,
            bone_index,
            GRANNY_BONE_SIZE,
            "Granny bone offset",
        )?;
        strings.add(base, bone.name.clone());

        let (flags, position, orientation, scale_shear) =
            if let Some(transform) = bone.local_transform.as_ref() {
                (
                    transform.flags,
                    &transform.position,
                    &transform.orientation,
                    &transform.scale_shear,
                )
            } else {
                let transform = fallback_transforms
                    .as_ref()
                    .and_then(|transforms| transforms.get(bone_index))
                    .ok_or_else(|| Error::UnsupportedFormat("missing fallback transform".into()))?;
                (
                    transform.flags,
                    &transform.position,
                    &transform.orientation,
                    &transform.scale_shear,
                )
            };

        let mut raw = GrannyBoneRaw::zeroed();
        raw.parent_index = bone.parent_index.to_le_bytes();
        raw.transform_flags = flags.to_le_bytes();
        for (component_index, &value) in position.iter().enumerate() {
            raw.position[component_index] = value.to_le_bytes();
        }
        for (component_index, &value) in orientation.iter().enumerate() {
            raw.orientation[component_index] = value.to_le_bytes();
        }
        for (row_index, row) in scale_shear.iter().enumerate() {
            for (column_index, &value) in row.iter().enumerate() {
                raw.scale_shear[row_index * 3 + column_index] = value.to_le_bytes();
            }
        }
        for (row_index, row) in bone.inverse_world_matrix.rows.iter().enumerate() {
            for (column_index, &value) in row.iter().enumerate() {
                raw.inverse_world[row_index * 4 + column_index] = value.to_le_bytes();
            }
        }
        raw.lod_error = bone.lod_error.to_le_bytes();
        patch_bytes(buf, base, raw.as_bytes(), "Granny bone")?;
    }
    Ok(())
}

/// Emit deduplicated bone extended-data schemas and values, then patch bones.
fn write_bone_extended_data(
    geom: &UgxGeom,
    buf: &mut Vec<u8>,
    strings: &mut super::string_table::StringTable,
    bones_start: usize,
) -> Result<()> {
    let mut unique_type_defs: Vec<Vec<crate::types::GrannyTypeMember>> = Vec::new();
    let mut bone_ext_type_index = Vec::with_capacity(geom.granny_bones.len());
    for bone in &geom.granny_bones {
        let type_index = bone.extended_data_type.as_ref().map(|type_members| {
            unique_type_defs
                .iter()
                .position(|existing| type_defs_equal(existing, type_members))
                .unwrap_or_else(|| {
                    let index = unique_type_defs.len();
                    unique_type_defs.push(type_members.clone());
                    index
                })
        });
        bone_ext_type_index.push(type_index);
    }

    let mut type_def_offsets = Vec::with_capacity(unique_type_defs.len());
    for type_members in &unique_type_defs {
        let aligned = align16(buf.len())?;
        buf.resize(aligned, 0);
        type_def_offsets.push(buf.len());
        emit_type_def_array(buf, strings, type_members)?;
    }

    let mut bone_data_offsets = vec![None; geom.granny_bones.len()];
    for (bone_index, bone) in geom.granny_bones.iter().enumerate() {
        if let (Some(variant), Some(type_members)) = (&bone.extended_data, &bone.extended_data_type)
        {
            let aligned = align16(buf.len())?;
            buf.resize(aligned, 0);
            bone_data_offsets[bone_index] = Some(buf.len());
            emit_variant_data(buf, strings, variant, type_members)?;
        }
    }

    for (bone_index, (type_index, data_offset)) in bone_ext_type_index
        .into_iter()
        .zip(bone_data_offsets)
        .enumerate()
    {
        let (Some(type_index), Some(data_offset)) = (type_index, data_offset) else {
            continue;
        };
        let type_offset = *type_def_offsets
            .get(type_index)
            .ok_or(Error::SizeOverflow("Granny type-definition index"))?;
        let bone_base = array_end(
            bones_start,
            bone_index,
            GRANNY_BONE_SIZE,
            "Granny bone offset",
        )?;
        let extended_offset = bone_base
            .checked_add(GRANNY_BONE_EXTENDED_DATA_OFFSET)
            .ok_or(Error::SizeOverflow("Granny extended-data offset"))?;
        patch_pointer(buf, extended_offset, type_offset)?;
        patch_pointer(
            buf,
            extended_offset
                .checked_add(8)
                .ok_or(Error::SizeOverflow("Granny extended-data offset"))?,
            data_offset,
        )?;
    }
    Ok(())
}

/// Emit one mesh's contiguous bone-binding records.
fn write_bone_binding_records(
    buf: &mut Vec<u8>,
    strings: &mut super::string_table::StringTable,
    start: usize,
    bindings: &[GrannyBoneBinding],
) -> Result<usize> {
    for (binding_index, binding) in bindings.iter().enumerate() {
        let binding_position = array_end(
            start,
            binding_index,
            GRANNY_BONE_BINDING_SIZE,
            "Granny bone binding",
        )?;
        strings.add(binding_position, binding.bone_name.clone());
        let data_position = binding_position
            .checked_add(0x08)
            .ok_or(Error::SizeOverflow("Granny bone binding"))?;
        let mut cursor = Cursor::new(&mut *buf);
        cursor.seek(SeekFrom::Start(
            u64::try_from(data_position).map_err(|_| Error::SizeOverflow("Granny bone binding"))?,
        ))?;
        for &value in &binding.obb_min {
            cursor.write_f32_le(value)?;
        }
        for &value in &binding.obb_max {
            cursor.write_f32_le(value)?;
        }
        cursor.write_i32_le(crate::checked_i32(
            binding.triangle_indices.len(),
            "Granny triangle-index count",
        )?)?;
        cursor.write_u64_le(0)?;
    }
    array_end(
        start,
        bindings.len(),
        GRANNY_BONE_BINDING_SIZE,
        "Granny bone bindings",
    )
}

/// Emit mesh pointer, mesh structure, and bone-binding arrays.
fn write_mesh_structures(
    buf: &mut Vec<u8>,
    strings: &mut super::string_table::StringTable,
    mesh_bone_bindings: &[Vec<GrannyBoneBinding>],
    mesh_names: &[String],
) -> Result<(usize, usize)> {
    let mesh_count = mesh_bone_bindings.len();
    let total_bone_bindings = mesh_bone_bindings
        .iter()
        .try_fold(0usize, |total, bindings| {
            total
                .checked_add(bindings.len())
                .ok_or(Error::SizeOverflow("Granny bone-binding count"))
        })?;
    let mesh_ptrs_start = align16(buf.len())?;
    buf.resize(mesh_ptrs_start, 0);
    let mesh_pointer_end = array_end(
        mesh_ptrs_start,
        mesh_count,
        core::mem::size_of::<u64>(),
        "Granny mesh pointers",
    )?;
    let mesh_structs_start = align16(mesh_pointer_end)?;
    let bone_bindings_start = array_end(
        mesh_structs_start,
        mesh_count,
        GRANNY_MESH_SIZE,
        "Granny mesh structures",
    )?;
    let after_bone_bindings = array_end(
        bone_bindings_start,
        total_bone_bindings,
        GRANNY_BONE_BINDING_SIZE,
        "Granny bone bindings",
    )?;
    buf.resize(after_bone_bindings, 0);

    patch_bytes(
        buf,
        0x54,
        &crate::checked_u32(mesh_count, "Granny mesh count")?.to_le_bytes(),
        "Granny mesh count",
    )?;
    patch_pointer(buf, 0x58, mesh_ptrs_start)?;

    for mesh_index in 0..mesh_count {
        let pointer_position = array_end(
            mesh_ptrs_start,
            mesh_index,
            core::mem::size_of::<u64>(),
            "Granny mesh pointer",
        )?;
        let mesh_position = array_end(
            mesh_structs_start,
            mesh_index,
            GRANNY_MESH_SIZE,
            "Granny mesh structure",
        )?;
        patch_pointer(buf, pointer_position, mesh_position)?;
    }

    let mut current_binding_offset = bone_bindings_start;
    for (mesh_index, bindings) in mesh_bone_bindings.iter().enumerate() {
        let mesh_position = array_end(
            mesh_structs_start,
            mesh_index,
            GRANNY_MESH_SIZE,
            "Granny mesh structure",
        )?;
        let mesh_name = mesh_names
            .get(mesh_index)
            .ok_or(Error::SizeOverflow("Granny mesh-name index"))?;
        strings.add(mesh_position, mesh_name.clone());
        patch_bytes(
            buf,
            mesh_position
                .checked_add(0x30)
                .ok_or(Error::SizeOverflow("Granny mesh binding count"))?,
            &crate::checked_u32(bindings.len(), "Granny bone-binding count")?.to_le_bytes(),
            "Granny bone-binding count",
        )?;
        if !bindings.is_empty() {
            patch_pointer(
                buf,
                mesh_position
                    .checked_add(0x34)
                    .ok_or(Error::SizeOverflow("Granny bone-binding pointer"))?,
                current_binding_offset,
            )?;
        }

        current_binding_offset =
            write_bone_binding_records(buf, strings, current_binding_offset, bindings)?;
    }
    Ok((mesh_ptrs_start, mesh_structs_start))
}

/// Emit the single model structure and its mesh-binding pointer array.
fn write_model_structures(
    buf: &mut Vec<u8>,
    strings: &mut super::string_table::StringTable,
    mesh_count: usize,
    mesh_structs_start: usize,
    skeleton_structure: usize,
) -> Result<()> {
    let model_pointer_array = align16(buf.len())?;
    buf.resize(model_pointer_array, 0);
    let model_structure = align16(
        model_pointer_array
            .checked_add(8)
            .ok_or(Error::SizeOverflow("Granny model layout"))?,
    )?;
    let mesh_bindings_start = model_structure
        .checked_add(0x70)
        .ok_or(Error::SizeOverflow("Granny model layout"))?;
    let after_model = array_end(
        mesh_bindings_start,
        mesh_count,
        core::mem::size_of::<u64>(),
        "Granny model mesh bindings",
    )?;
    buf.resize(after_model, 0);

    patch_bytes(buf, 0x60, &1u32.to_le_bytes(), "Granny model count")?;
    patch_pointer(buf, 0x64, model_pointer_array)?;
    patch_pointer(buf, model_pointer_array, model_structure)?;
    strings.add(model_structure, "GrannyRootBone".to_string());
    patch_pointer(
        buf,
        model_structure
            .checked_add(0x08)
            .ok_or(Error::SizeOverflow("Granny model skeleton pointer"))?,
        skeleton_structure,
    )?;

    let mut identity_transform = [0u8; 68];
    for position in [28usize, 32, 44, 56] {
        patch_bytes(
            &mut identity_transform,
            position,
            &1.0f32.to_le_bytes(),
            "Granny identity transform",
        )?;
    }
    patch_bytes(
        buf,
        model_structure
            .checked_add(0x10)
            .ok_or(Error::SizeOverflow("Granny initial placement"))?,
        &identity_transform,
        "Granny initial placement",
    )?;
    patch_bytes(
        buf,
        model_structure
            .checked_add(0x54)
            .ok_or(Error::SizeOverflow("Granny model mesh count"))?,
        &crate::checked_u32(mesh_count, "Granny model mesh count")?.to_le_bytes(),
        "Granny model mesh count",
    )?;
    patch_pointer(
        buf,
        model_structure
            .checked_add(0x58)
            .ok_or(Error::SizeOverflow("Granny model mesh bindings"))?,
        mesh_bindings_start,
    )?;

    for mesh_index in 0..mesh_count {
        let binding_position = array_end(
            mesh_bindings_start,
            mesh_index,
            core::mem::size_of::<u64>(),
            "Granny model mesh binding",
        )?;
        let mesh_position = array_end(
            mesh_structs_start,
            mesh_index,
            GRANNY_MESH_SIZE,
            "Granny mesh structure",
        )?;
        patch_pointer(buf, binding_position, mesh_position)?;
    }
    Ok(())
}

/// Build the granny bones chunk (0x703).
pub(super) fn build_granny_data(geom: &UgxGeom) -> Result<Vec<u8>> {
    let bone_count = geom.granny_bones.len();
    let (mesh_bone_bindings, mesh_names) = collect_mesh_data(geom);
    let mesh_count = mesh_bone_bindings.len();

    // ---- Phase 1: Fixed structure offsets (16-byte aligned) ----
    let header_size: usize = 0x94;
    let skel_ptr_array = align16(header_size)?;
    let skel_struct = align16(
        skel_ptr_array
            .checked_add(8)
            .ok_or(Error::SizeOverflow("Granny skeleton layout"))?,
    )?;
    let bones_start = align16(
        skel_struct
            .checked_add(0x28)
            .ok_or(Error::SizeOverflow("Granny skeleton layout"))?,
    )?;
    let bones_end = array_end(
        bones_start,
        bone_count,
        GRANNY_BONE_SIZE,
        "Granny bone array",
    )?;
    let mut buf = vec![0u8; bones_end];
    let mut strings = super::string_table::StringTable::new();

    write_skeleton_and_bones(
        geom,
        &mut buf,
        &mut strings,
        skel_ptr_array,
        skel_struct,
        bones_start,
    )?;

    // ---- Phase 2: Extended data (appended after bones) ----
    write_bone_extended_data(geom, &mut buf, &mut strings, bones_start)?;

    // ---- Phase 3: Mesh structures (appended after extended data) ----
    let (_, mesh_structs_start) =
        write_mesh_structures(&mut buf, &mut strings, &mesh_bone_bindings, &mesh_names)?;

    // ---- Phase 4: Model structures (appended after mesh data) ----
    write_model_structures(
        &mut buf,
        &mut strings,
        mesh_count,
        mesh_structs_start,
        skel_struct,
    )?;

    // ---- Phase 5: FileInfo type definition tree ----
    let file_info_type = build_file_info_type_tree();
    emit_type_def_array(&mut buf, &mut strings, &file_info_type)?;

    // ---- Phase 6: Final string table (all names) ----
    strings.write(&mut buf)?;

    Ok(buf)
}
