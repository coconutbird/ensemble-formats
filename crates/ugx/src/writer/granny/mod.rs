//! Granny bones chunk (0x703) builder.
//!
//! Produces a Granny2-compatible serialized chunk with file info header,
//! skeleton, bone array, mesh structs, bone bindings, string table,
//! and bone ExtendedData (type definitions + variant data).

mod extended_data;
mod fallback;
mod type_tree;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use nostdio::{MutCursor, Seek, SeekFrom, WriteLe};

use crate::constants::{
    GRANNY_BONE_BINDING_SIZE, GRANNY_BONE_EXTENDED_DATA_OFFSET, GRANNY_BONE_SIZE, GRANNY_MESH_SIZE,
};
use crate::error::Result;
use crate::types::{GrannyBoneBinding, UgxGeom};

use extended_data::{emit_type_def_array, emit_variant_data, type_defs_equal};
use fallback::compute_fallback_local_transforms;
use type_tree::build_file_info_type_tree;

/// Align to 16-byte boundary (matching original engine alignment).
fn align16(n: usize) -> usize {
    (n + 15) & !15
}

/// Build the granny bones chunk (0x703).
pub(super) fn build_granny_data(geom: &UgxGeom) -> Result<Vec<u8>> {
    let bone_count = geom.granny_bones.len();
    let section_count = geom.sections.len();
    let use_stored_meshes = !geom.granny_meshes.is_empty();
    let mesh_count = if use_stored_meshes {
        geom.granny_meshes.len()
    } else {
        section_count
    };

    let mesh_bone_bindings: Vec<Vec<GrannyBoneBinding>> = if use_stored_meshes {
        geom.granny_meshes
            .iter()
            .map(|m| m.bone_bindings.clone())
            .collect()
    } else {
        geom.sections
            .iter()
            .map(|section| {
                if section.bone_remap.is_empty() {
                    if section.rigid_bone_index >= 0
                        && (section.rigid_bone_index as usize) < bone_count
                    {
                        vec![GrannyBoneBinding {
                            bone_name: geom.granny_bones[section.rigid_bone_index as usize]
                                .name
                                .clone(),
                            ..Default::default()
                        }]
                    } else {
                        vec![]
                    }
                } else {
                    section
                        .bone_remap
                        .iter()
                        .filter_map(|&idx| {
                            let idx = idx as usize;
                            if idx < geom.granny_bones.len() {
                                Some(GrannyBoneBinding {
                                    bone_name: geom.granny_bones[idx].name.clone(),
                                    ..Default::default()
                                })
                            } else {
                                None
                            }
                        })
                        .collect()
                }
            })
            .collect()
    };

    let mesh_names: Vec<String> = if use_stored_meshes {
        geom.granny_meshes.iter().map(|m| m.name.clone()).collect()
    } else {
        (0..section_count).map(|i| format!("mesh_{}", i)).collect()
    };

    let total_bone_bindings: usize = mesh_bone_bindings.iter().map(|v| v.len()).sum();

    // ---- Phase 1: Fixed structure offsets (16-byte aligned) ----
    let header_size: usize = 0x94;
    let skel_ptr_array = align16(header_size);
    let skel_struct = align16(skel_ptr_array + 8);
    let bones_start = align16(skel_struct + 0x28);
    let bones_end = bones_start + bone_count * GRANNY_BONE_SIZE;
    let mut buf = vec![0u8; bones_end];
    let mut strings = super::string_table::StringTable::new();

    // ---- File info header [0x00..0x94] ----
    strings.add(0x10, "gr2ugx".to_string());
    {
        let mut cursor = MutCursor::new(&mut buf);
        cursor.seek(SeekFrom::Start(0x30))?;
        cursor.write_u32_le(1)?;
        cursor.write_u64_le(skel_ptr_array as u64)?;
    }

    // ---- Skeleton pointer array ----
    buf[skel_ptr_array..skel_ptr_array + 8].copy_from_slice(&(skel_struct as u64).to_le_bytes());

    // ---- Skeleton struct ----
    strings.add(skel_struct, "GrannyRootBone".to_string());
    {
        use crate::types::raw::GrannySkeletonRaw;
        use zerocopy::IntoBytes;
        let skel = GrannySkeletonRaw {
            name_ptr: [0; 8],
            bone_count: (bone_count as u32).to_le_bytes(),
            bones_ptr: (bones_start as u64).to_le_bytes(),
            lod_type: geom.skeleton_lod_type.to_le_bytes(),
            _pad: [0; 16],
        };
        buf[skel_struct..skel_struct + core::mem::size_of::<GrannySkeletonRaw>()]
            .copy_from_slice(skel.as_bytes());
    }

    // ---- Bone array ----
    let fallback_transforms = if geom
        .granny_bones
        .iter()
        .any(|b| b.local_transform.is_none())
    {
        Some(compute_fallback_local_transforms(geom))
    } else {
        None
    };

    {
        use crate::types::raw::GrannyBoneRaw;
        use zerocopy::IntoBytes;

        for (i, bone) in geom.granny_bones.iter().enumerate() {
            let base = bones_start + i * GRANNY_BONE_SIZE;
            strings.add(base, bone.name.clone());

            let (flags, position, orientation, scale_shear) =
                if let Some(ref lt) = bone.local_transform {
                    (lt.flags, &lt.position, &lt.orientation, &lt.scale_shear)
                } else {
                    let fb = &fallback_transforms.as_ref().unwrap()[i];
                    (fb.flags, &fb.position, &fb.orientation, &fb.scale_shear)
                };

            let mut raw = GrannyBoneRaw::zeroed();
            raw.parent_index = bone.parent_index.to_le_bytes();
            raw.transform_flags = flags.to_le_bytes();
            for (j, &v) in position.iter().enumerate() {
                raw.position[j] = v.to_le_bytes();
            }
            for (j, &v) in orientation.iter().enumerate() {
                raw.orientation[j] = v.to_le_bytes();
            }
            for (j, row) in scale_shear.iter().enumerate() {
                for (k, &v) in row.iter().enumerate() {
                    raw.scale_shear[j * 3 + k] = v.to_le_bytes();
                }
            }
            for (j, row) in bone.inverse_world_matrix.rows.iter().enumerate() {
                for (k, &v) in row.iter().enumerate() {
                    raw.inverse_world[j * 4 + k] = v.to_le_bytes();
                }
            }
            raw.lod_error = bone.lod_error.to_le_bytes();
            buf[base..base + GRANNY_BONE_SIZE].copy_from_slice(raw.as_bytes());
        }
    }

    // ---- Phase 2: Extended data (appended after bones) ----
    let mut unique_type_defs: Vec<Vec<crate::types::GrannyTypeMember>> = Vec::new();
    let mut bone_ext_type_index: Vec<Option<usize>> = Vec::with_capacity(bone_count);
    for bone in &geom.granny_bones {
        if let Some(ref type_members) = bone.extended_data_type {
            let idx = unique_type_defs
                .iter()
                .position(|existing| type_defs_equal(existing, type_members));
            if let Some(idx) = idx {
                bone_ext_type_index.push(Some(idx));
            } else {
                bone_ext_type_index.push(Some(unique_type_defs.len()));
                unique_type_defs.push(type_members.clone());
            }
        } else {
            bone_ext_type_index.push(None);
        }
    }

    let mut type_def_offsets: Vec<usize> = Vec::with_capacity(unique_type_defs.len());
    #[cfg(feature = "std")]
    {
        for (ti, td) in unique_type_defs.iter().enumerate() {
            let nested = td.iter().filter(|m| m.reference_type.is_some()).count();
            std::eprintln!(
                "  [granny] unique_type[{ti}]: {} members, {nested} nested refs",
                td.len()
            );
        }
    }
    for (ti, type_members) in unique_type_defs.iter().enumerate() {
        let aligned = align16(buf.len());
        buf.resize(aligned, 0);
        let type_def_offset = buf.len();
        type_def_offsets.push(type_def_offset);
        let before = buf.len();
        emit_type_def_array(&mut buf, &mut strings, type_members);
        #[cfg(feature = "std")]
        std::eprintln!(
            "  [granny] type[{ti}] at 0x{type_def_offset:04X}: {} bytes emitted",
            buf.len() - before
        );
    }

    let mut bone_data_offsets: Vec<Option<usize>> = vec![None; bone_count];
    for (i, bone) in geom.granny_bones.iter().enumerate() {
        if let (Some(variant), Some(type_members)) = (&bone.extended_data, &bone.extended_data_type)
        {
            let aligned = align16(buf.len());
            buf.resize(aligned, 0);
            let data_offset = buf.len();
            bone_data_offsets[i] = Some(data_offset);
            let before = buf.len();
            emit_variant_data(&mut buf, &mut strings, variant, type_members);
            #[cfg(feature = "std")]
            if i < 5 || buf.len() - before > 100 {
                std::eprintln!(
                    "  [granny] bone[{i}] data at 0x{data_offset:04X}: {} bytes",
                    buf.len() - before
                );
            }
        }
    }

    #[cfg(feature = "std")]
    {
        let ext_count = bone_data_offsets.iter().filter(|x| x.is_some()).count();
        std::eprintln!(
            "  [granny] {ext_count}/{bone_count} bones have extended data, buf now {} bytes",
            buf.len()
        );
    }

    for i in 0..bone_count {
        let bone_base = bones_start + i * GRANNY_BONE_SIZE;
        let ext_offset = bone_base + GRANNY_BONE_EXTENDED_DATA_OFFSET;
        if let (Some(type_idx), Some(data_off)) = (bone_ext_type_index[i], bone_data_offsets[i]) {
            let type_off = type_def_offsets[type_idx];
            buf[ext_offset..ext_offset + 8].copy_from_slice(&(type_off as u64).to_le_bytes());
            buf[ext_offset + 8..ext_offset + 16].copy_from_slice(&(data_off as u64).to_le_bytes());
        }
    }

    // ---- Phase 3: Mesh structures (appended after extended data) ----
    let mesh_ptrs_start = align16(buf.len());
    buf.resize(mesh_ptrs_start, 0);

    let mesh_structs_start = align16(mesh_ptrs_start + mesh_count * 8);
    let bone_bindings_start = mesh_structs_start + mesh_count * GRANNY_MESH_SIZE;
    let after_bone_bindings = bone_bindings_start + total_bone_bindings * GRANNY_BONE_BINDING_SIZE;
    buf.resize(after_bone_bindings, 0);

    // Patch file_info Meshes RTA at +0x54
    buf[0x54..0x58].copy_from_slice(&(mesh_count as u32).to_le_bytes());
    buf[0x58..0x60].copy_from_slice(&(mesh_ptrs_start as u64).to_le_bytes());

    for i in 0..mesh_count {
        let ptr_pos = mesh_ptrs_start + i * 8;
        let mesh_struct_pos = mesh_structs_start + i * GRANNY_MESH_SIZE;
        buf[ptr_pos..ptr_pos + 8].copy_from_slice(&(mesh_struct_pos as u64).to_le_bytes());
    }

    let mut current_bb_offset = bone_bindings_start;
    for i in 0..mesh_count {
        let mesh_struct_pos = mesh_structs_start + i * GRANNY_MESH_SIZE;
        let bb_count = mesh_bone_bindings[i].len();

        strings.add(mesh_struct_pos, mesh_names[i].clone());

        buf[mesh_struct_pos + 0x30..mesh_struct_pos + 0x34]
            .copy_from_slice(&(bb_count as u32).to_le_bytes());
        if bb_count > 0 {
            buf[mesh_struct_pos + 0x34..mesh_struct_pos + 0x3C]
                .copy_from_slice(&(current_bb_offset as u64).to_le_bytes());
        }

        for (j, binding) in mesh_bone_bindings[i].iter().enumerate() {
            let binding_pos = current_bb_offset + j * GRANNY_BONE_BINDING_SIZE;
            strings.add(binding_pos, binding.bone_name.clone());

            let mut cursor = MutCursor::new(&mut buf);
            cursor.seek(SeekFrom::Start((binding_pos + 0x08) as u64))?;
            for &v in &binding.obb_min {
                cursor.write_f32_le(v)?;
            }
            for &v in &binding.obb_max {
                cursor.write_f32_le(v)?;
            }
            cursor.write_i32_le(binding.triangle_indices.len() as i32)?;
            cursor.write_u64_le(0)?;
        }
        current_bb_offset += bb_count * GRANNY_BONE_BINDING_SIZE;
    }

    // ---- Phase 4: Model structures (appended after mesh data) ----
    let model_ptr_array = align16(buf.len());
    buf.resize(model_ptr_array, 0);

    let model_struct = align16(model_ptr_array + 8);
    let model_struct_size = 0x70;
    let mesh_bindings_start = model_struct + model_struct_size;
    let after_model = mesh_bindings_start + mesh_count * 8;
    buf.resize(after_model, 0);

    buf[0x60..0x64].copy_from_slice(&1u32.to_le_bytes());
    buf[0x64..0x6C].copy_from_slice(&(model_ptr_array as u64).to_le_bytes());

    buf[model_ptr_array..model_ptr_array + 8].copy_from_slice(&(model_struct as u64).to_le_bytes());

    strings.add(model_struct, "GrannyRootBone".to_string());
    buf[model_struct + 0x08..model_struct + 0x10]
        .copy_from_slice(&(skel_struct as u64).to_le_bytes());

    // +0x10: InitialPlacement (identity transform)
    {
        let identity_transform: [u8; 68] = {
            let mut t = [0u8; 68];
            t[28..32].copy_from_slice(&1.0f32.to_le_bytes());
            t[32..36].copy_from_slice(&1.0f32.to_le_bytes());
            t[44..48].copy_from_slice(&1.0f32.to_le_bytes());
            t[56..60].copy_from_slice(&1.0f32.to_le_bytes());
            t
        };
        let off = model_struct + 0x10;
        buf[off..off + 68].copy_from_slice(&identity_transform);
    }

    buf[model_struct + 0x54..model_struct + 0x58]
        .copy_from_slice(&(mesh_count as u32).to_le_bytes());
    buf[model_struct + 0x58..model_struct + 0x60]
        .copy_from_slice(&(mesh_bindings_start as u64).to_le_bytes());

    for i in 0..mesh_count {
        let binding_pos = mesh_bindings_start + i * 8;
        let mesh_struct_pos = mesh_structs_start + i * GRANNY_MESH_SIZE;
        buf[binding_pos..binding_pos + 8].copy_from_slice(&(mesh_struct_pos as u64).to_le_bytes());
    }

    // ---- Phase 5: FileInfo type definition tree ----
    let before_tt = buf.len();
    let file_info_type = build_file_info_type_tree();
    emit_type_def_array(&mut buf, &mut strings, &file_info_type);
    #[cfg(feature = "std")]
    std::eprintln!(
        "  [granny] FileInfo type tree: {} bytes emitted at 0x{:04X}",
        buf.len() - before_tt,
        before_tt
    );

    // ---- Phase 6: Final string table (all names) ----
    strings.write(&mut buf);

    Ok(buf)
}
