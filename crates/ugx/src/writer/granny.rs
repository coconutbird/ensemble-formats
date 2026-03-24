//! Granny bones chunk (0x703) builder.
//!
//! Produces a Granny2-compatible serialized chunk with file info header,
//! skeleton, bone array, mesh structs, bone bindings, and string table.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use ecf::io::{MutCursor, Seek, SeekFrom, WriteLe};

use crate::error::Result;
use crate::raw::{
    GRANNY_BONE_BINDING_SIZE, GRANNY_BONE_SIZE, GRANNY_HAS_ORIENTATION, GRANNY_HAS_POSITION,
    GRANNY_HAS_SCALE_SHEAR, GRANNY_MESH_SIZE,
};
use crate::types::{Matrix4x4, UgxGeom};

/// Build the granny bones chunk (0x703).
///
/// Produces a Granny2-compatible serialized chunk with:
/// - File info header with skeleton and mesh array pointers
/// - Skeleton struct with bone count and array pointer
/// - Bone array with names, parent indices, computed local transforms, and inverse world matrices
/// - Mesh pointer array with section names
/// - String table
///
/// Local transforms are computed from inverse world matrices: for each bone,
/// `local = parent_world_inverse * world` where `world = inverse(inverse_world)`.
pub(super) fn build_granny_data(geom: &UgxGeom) -> Result<Vec<u8>> {
    let bone_count = geom.granny_bones.len();
    let section_count = geom.sections.len();

    // ---- Compute local transforms from inverse world matrices ----
    let world_matrices: Vec<Matrix4x4> = geom
        .granny_bones
        .iter()
        .map(|bone| bone.inverse_world_matrix.inverse().unwrap_or_default())
        .collect();

    struct LocalTransform {
        flags: u32,
        position: [f32; 3],
        orientation: [f32; 4],
        scale_shear: [[f32; 3]; 3],
    }

    let local_transforms: Vec<LocalTransform> = geom
        .granny_bones
        .iter()
        .enumerate()
        .map(|(i, bone)| {
            let local_matrix =
                if bone.parent_index >= 0 && (bone.parent_index as usize) < bone_count {
                    let parent_idx = bone.parent_index as usize;
                    geom.granny_bones[parent_idx]
                        .inverse_world_matrix
                        .multiply(&world_matrices[i])
                } else {
                    world_matrices[i].clone()
                };

            let position = local_matrix.translation();
            let m = &local_matrix.rows;
            let sx = (m[0][0] * m[0][0] + m[1][0] * m[1][0] + m[2][0] * m[2][0]).sqrt();
            let sy = (m[0][1] * m[0][1] + m[1][1] * m[1][1] + m[2][1] * m[2][1]).sqrt();
            let sz = (m[0][2] * m[0][2] + m[1][2] * m[1][2] + m[2][2] * m[2][2]).sqrt();

            let rot_matrix = if sx > 1e-7 && sy > 1e-7 && sz > 1e-7 {
                Matrix4x4 {
                    rows: [
                        [m[0][0] / sx, m[0][1] / sy, m[0][2] / sz, 0.0],
                        [m[1][0] / sx, m[1][1] / sy, m[1][2] / sz, 0.0],
                        [m[2][0] / sx, m[2][1] / sy, m[2][2] / sz, 0.0],
                        [0.0, 0.0, 0.0, 1.0],
                    ],
                }
            } else {
                Matrix4x4::identity()
            };

            let orientation = rot_matrix.to_quaternion();
            let rt = rot_matrix.transpose();
            let scale_shear = [
                [
                    rt.rows[0][0] * m[0][0] + rt.rows[0][1] * m[1][0] + rt.rows[0][2] * m[2][0],
                    rt.rows[0][0] * m[0][1] + rt.rows[0][1] * m[1][1] + rt.rows[0][2] * m[2][1],
                    rt.rows[0][0] * m[0][2] + rt.rows[0][1] * m[1][2] + rt.rows[0][2] * m[2][2],
                ],
                [
                    rt.rows[1][0] * m[0][0] + rt.rows[1][1] * m[1][0] + rt.rows[1][2] * m[2][0],
                    rt.rows[1][0] * m[0][1] + rt.rows[1][1] * m[1][1] + rt.rows[1][2] * m[2][1],
                    rt.rows[1][0] * m[0][2] + rt.rows[1][1] * m[1][2] + rt.rows[1][2] * m[2][2],
                ],
                [
                    rt.rows[2][0] * m[0][0] + rt.rows[2][1] * m[1][0] + rt.rows[2][2] * m[2][0],
                    rt.rows[2][0] * m[0][1] + rt.rows[2][1] * m[1][1] + rt.rows[2][2] * m[2][1],
                    rt.rows[2][0] * m[0][2] + rt.rows[2][1] * m[1][2] + rt.rows[2][2] * m[2][2],
                ],
            ];

            let mut flags = 0u32;
            if position[0].abs() > 1e-7 || position[1].abs() > 1e-7 || position[2].abs() > 1e-7 {
                flags |= GRANNY_HAS_POSITION;
            }
            if (orientation[0].abs() > 1e-7)
                || (orientation[1].abs() > 1e-7)
                || (orientation[2].abs() > 1e-7)
                || ((orientation[3] - 1.0).abs() > 1e-7)
            {
                flags |= GRANNY_HAS_ORIENTATION;
            }
            let is_identity_scale = (scale_shear[0][0] - 1.0).abs() < 1e-5
                && scale_shear[0][1].abs() < 1e-5
                && scale_shear[0][2].abs() < 1e-5
                && scale_shear[1][0].abs() < 1e-5
                && (scale_shear[1][1] - 1.0).abs() < 1e-5
                && scale_shear[1][2].abs() < 1e-5
                && scale_shear[2][0].abs() < 1e-5
                && scale_shear[2][1].abs() < 1e-5
                && (scale_shear[2][2] - 1.0).abs() < 1e-5;
            if !is_identity_scale {
                flags |= GRANNY_HAS_SCALE_SHEAR;
            }

            LocalTransform {
                flags,
                position,
                orientation,
                scale_shear,
            }
        })
        .collect();

    // ---- Determine mesh data ----
    let use_stored_meshes = !geom.granny_meshes.is_empty();
    let mesh_count = if use_stored_meshes {
        geom.granny_meshes.len()
    } else {
        section_count
    };

    let mesh_bone_bindings: Vec<Vec<String>> = if use_stored_meshes {
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
                        vec![
                            geom.granny_bones[section.rigid_bone_index as usize]
                                .name
                                .clone(),
                        ]
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
                                Some(geom.granny_bones[idx].name.clone())
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

    // ---- Calculate struct offsets ----
    let header_size: usize = 0x94;
    let skeleton_ptr_array_offset = header_size;
    let skeleton_struct_offset = skeleton_ptr_array_offset + 8;
    let model_ptr_array_offset = skeleton_struct_offset + 0x18;
    let model_struct_offset = model_ptr_array_offset + 8;
    let model_struct_size = 0x60;
    let bones_start = (model_struct_offset + model_struct_size + 7) & !7;
    let bones_end = bones_start + bone_count * GRANNY_BONE_SIZE;
    let mesh_bindings_start = bones_end;
    let mesh_ptrs_start = mesh_bindings_start + mesh_count * 8;
    let mesh_structs_start = mesh_ptrs_start + mesh_count * 8;
    let bone_bindings_start = mesh_structs_start + mesh_count * GRANNY_MESH_SIZE;
    let strings_start = bone_bindings_start + total_bone_bindings * GRANNY_BONE_BINDING_SIZE;

    let mut buf = vec![0u8; strings_start];
    let mut cursor = MutCursor::new(&mut buf);

    // ---- File info header [0x00..0x94] ----
    cursor.seek(SeekFrom::Start(0x30))?;
    cursor.write_u32_le(1)?; // SkeletonCount
    cursor.write_u64_le(skeleton_ptr_array_offset as u64)?;

    cursor.seek(SeekFrom::Start(0x54))?;
    cursor.write_u32_le(mesh_count as u32)?;
    cursor.write_u64_le(mesh_ptrs_start as u64)?;

    cursor.seek(SeekFrom::Start(0x60))?;
    cursor.write_u32_le(1)?; // ModelCount
    cursor.write_u64_le(model_ptr_array_offset as u64)?;

    // ---- Skeleton pointer array ----
    cursor.seek(SeekFrom::Start(skeleton_ptr_array_offset as u64))?;
    cursor.write_u64_le(skeleton_struct_offset as u64)?;

    // ---- Skeleton struct ----
    cursor.seek(SeekFrom::Start((skeleton_struct_offset + 0x08) as u64))?;
    cursor.write_u32_le(bone_count as u32)?;
    cursor.write_u64_le(bones_start as u64)?;
    cursor.write_u32_le(0)?; // LODType

    // ---- Model pointer array ----
    cursor.seek(SeekFrom::Start(model_ptr_array_offset as u64))?;
    cursor.write_u64_le(model_struct_offset as u64)?;

    // ---- Model struct ----
    cursor.seek(SeekFrom::Start((model_struct_offset + 0x08) as u64))?;
    cursor.write_u64_le(skeleton_struct_offset as u64)?;

    // InitialPlacement (identity transform)
    cursor.seek(SeekFrom::Start((model_struct_offset + 0x10) as u64))?;
    cursor.write_u32_le(0)?; // Flags
    for _ in 0..3 {
        cursor.write_f32_le(0.0)?;
    } // Position
    cursor.write_f32_le(0.0)?;
    cursor.write_f32_le(0.0)?;
    cursor.write_f32_le(0.0)?;
    cursor.write_f32_le(1.0)?; // Orientation w=1
    // ScaleShear identity 3x3
    cursor.write_f32_le(1.0)?;
    cursor.write_f32_le(0.0)?;
    cursor.write_f32_le(0.0)?;
    cursor.write_f32_le(0.0)?;
    cursor.write_f32_le(1.0)?;
    cursor.write_f32_le(0.0)?;
    cursor.write_f32_le(0.0)?;
    cursor.write_f32_le(0.0)?;
    cursor.write_f32_le(1.0)?;

    // MeshBindingCount + MeshBindings
    cursor.seek(SeekFrom::Start((model_struct_offset + 0x54) as u64))?;
    cursor.write_u32_le(mesh_count as u32)?;
    cursor.write_u64_le(mesh_bindings_start as u64)?;

    // ---- Bone array ----
    let mut strings = super::string_table::StringTable::new();

    for (i, bone) in geom.granny_bones.iter().enumerate() {
        let base = bones_start + i * GRANNY_BONE_SIZE;
        let lt = &local_transforms[i];

        strings.add(base, bone.name.clone());

        cursor.seek(SeekFrom::Start((base + 0x08) as u64))?;
        cursor.write_i32_le(bone.parent_index)?;
        cursor.write_u32_le(lt.flags)?;
        for &v in &lt.position {
            cursor.write_f32_le(v)?;
        }
        for &v in &lt.orientation {
            cursor.write_f32_le(v)?;
        }
        for row in &lt.scale_shear {
            for &v in row {
                cursor.write_f32_le(v)?;
            }
        }
        for row in &bone.inverse_world_matrix.rows {
            for &val in row {
                cursor.write_f32_le(val)?;
            }
        }
        cursor.write_f32_le(geom.bounding_sphere.radius)?;
    }

    // ---- Model mesh bindings (for Model->MeshBindings) ----
    for i in 0..mesh_count {
        let binding_pos = mesh_bindings_start + i * 8;
        let mesh_struct_pos = mesh_structs_start + i * GRANNY_MESH_SIZE;
        cursor.seek(SeekFrom::Start(binding_pos as u64))?;
        cursor.write_u64_le(mesh_struct_pos as u64)?;
    }

    // ---- Mesh pointer array (for file_info->Meshes) ----
    for i in 0..mesh_count {
        let ptr_pos = mesh_ptrs_start + i * 8;
        let mesh_struct_pos = mesh_structs_start + i * GRANNY_MESH_SIZE;
        cursor.seek(SeekFrom::Start(ptr_pos as u64))?;
        cursor.write_u64_le(mesh_struct_pos as u64)?;
    }

    // ---- Full mesh structs (0x4C bytes each) ----
    let mut current_bone_binding_offset = bone_bindings_start;
    for i in 0..mesh_count {
        let mesh_struct_pos = mesh_structs_start + i * GRANNY_MESH_SIZE;
        let bone_binding_count = mesh_bone_bindings[i].len();

        strings.add(mesh_struct_pos, mesh_names[i].clone());

        // +0x30: BoneBindingCount
        cursor.seek(SeekFrom::Start((mesh_struct_pos + 0x30) as u64))?;
        cursor.write_u32_le(bone_binding_count as u32)?;

        // +0x34: BoneBindings pointer
        if bone_binding_count > 0 {
            cursor.write_u64_le(current_bone_binding_offset as u64)?;
        }

        current_bone_binding_offset += bone_binding_count * GRANNY_BONE_BINDING_SIZE;
    }

    // ---- Bone binding arrays for each mesh ----
    current_bone_binding_offset = bone_bindings_start;
    for bone_names in mesh_bone_bindings.iter().take(mesh_count) {
        if bone_names.is_empty() {
            continue;
        }
        for (j, bone_name) in bone_names.iter().enumerate() {
            let binding_pos = current_bone_binding_offset + j * GRANNY_BONE_BINDING_SIZE;
            strings.add(binding_pos, bone_name.clone());
        }
        current_bone_binding_offset += bone_names.len() * GRANNY_BONE_BINDING_SIZE;
    }

    // ---- String table fixups ----
    strings.add(0x10, "gr2ugx".to_string());
    strings.add(skeleton_struct_offset, "GrannyRootBone".to_string());
    strings.add(model_struct_offset, "GrannyRootBone".to_string());

    // ---- Build string table and patch offsets ----
    strings.write(&mut buf);

    Ok(buf)
}
