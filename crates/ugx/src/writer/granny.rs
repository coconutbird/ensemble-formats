//! Granny bones chunk (0x703) builder.
//!
//! Produces a Granny2-compatible serialized chunk with file info header,
//! skeleton, bone array, mesh structs, bone bindings, string table,
//! and bone ExtendedData (type definitions + variant data).

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
use crate::types::{GrannyMemberType, GrannyTypeMember, GrannyVariant, Matrix4x4, UgxGeom};

/// Size of a single GrannyDataTypeDefinition on disk.
const GRANNY_TYPE_DEF_STRIDE: usize = 44;

/// Offset within a bone struct where ExtendedData {type_ptr, data_ptr} lives.
const GRANNY_BONE_EXTENDED_DATA_OFFSET: usize = 0x94;

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

    // ---- Emit bone ExtendedData (type definitions + data blobs) ----
    //
    // For each bone that has extended_data, we emit:
    //   1. A GrannyDataTypeDefinition[] array (type schema)
    //   2. A data blob described by that schema
    // Then patch the bone's ExtendedData pointers at bone+0x94.
    //
    // To deduplicate type definitions: bones that share the same type layout
    // (same member names/types) share the same type def array.

    // Collect unique type definitions and assign indices
    let mut unique_type_defs: Vec<Vec<GrannyTypeMember>> = Vec::new();
    let mut bone_ext_type_index: Vec<Option<usize>> = Vec::with_capacity(bone_count);

    for bone in &geom.granny_bones {
        if let Some(ref type_members) = bone.extended_data_type {
            // Check if we already have this type layout
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

    // Emit type definitions and collect their offsets
    let mut type_def_offsets: Vec<usize> = Vec::with_capacity(unique_type_defs.len());
    for type_members in &unique_type_defs {
        // Align to 4 bytes
        while !buf.len().is_multiple_of(4) {
            buf.push(0);
        }
        let type_def_offset = buf.len();
        type_def_offsets.push(type_def_offset);

        // Emit each member + end terminator
        // String pointers in type defs need fixups too
        let mut type_strings = super::string_table::StringTable::new();
        emit_type_def_array(&mut buf, &mut type_strings, type_members);

        // Patch type def name pointers
        type_strings.write(&mut buf);
    }

    // Emit data blobs for each bone's extended data
    let mut bone_data_offsets: Vec<Option<usize>> = vec![None; bone_count];
    for (i, bone) in geom.granny_bones.iter().enumerate() {
        if let (Some(variant), Some(type_members)) = (&bone.extended_data, &bone.extended_data_type)
        {
            // Align to 4 bytes
            while !buf.len().is_multiple_of(4) {
                buf.push(0);
            }
            let data_offset = buf.len();
            bone_data_offsets[i] = Some(data_offset);

            let mut data_strings = super::string_table::StringTable::new();
            emit_variant_data(&mut buf, &mut data_strings, variant, type_members);
            data_strings.write(&mut buf);
        }
    }

    // Patch bone ExtendedData pointers: bone+0x94 = {type_def_ptr, data_ptr}
    for i in 0..bone_count {
        let bone_base = bones_start + i * GRANNY_BONE_SIZE;
        let ext_offset = bone_base + GRANNY_BONE_EXTENDED_DATA_OFFSET;

        if let (Some(type_idx), Some(data_off)) = (bone_ext_type_index[i], bone_data_offsets[i]) {
            let type_off = type_def_offsets[type_idx];
            buf[ext_offset..ext_offset + 8].copy_from_slice(&(type_off as u64).to_le_bytes());
            buf[ext_offset + 8..ext_offset + 16].copy_from_slice(&(data_off as u64).to_le_bytes());
        }
        // If no extended data, the 16 bytes at bone+0x94 stay as zeros (already initialized)
    }

    Ok(buf)
}

// ---------------------------------------------------------------------------
// ExtendedData emission helpers
// ---------------------------------------------------------------------------

/// Check if two type definition arrays have the same layout.
fn type_defs_equal(a: &[GrannyTypeMember], b: &[GrannyTypeMember]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    for (ma, mb) in a.iter().zip(b.iter()) {
        if ma.member_type != mb.member_type
            || ma.name != mb.name
            || ma.array_width != mb.array_width
        {
            return false;
        }
        match (&ma.reference_type, &mb.reference_type) {
            (Some(ra), Some(rb)) => {
                if !type_defs_equal(ra, rb) {
                    return false;
                }
            }
            (None, None) => {}
            _ => return false,
        }
    }
    true
}

/// Emit a GrannyDataTypeDefinition[] array (with End terminator) into `buf`.
///
/// Two-pass approach: first emit all entries contiguously (engine traverses
/// with stride=44), then emit nested type arrays afterward and patch the
/// ReferenceType pointers.
fn emit_type_def_array(
    buf: &mut Vec<u8>,
    strings: &mut super::string_table::StringTable,
    members: &[GrannyTypeMember],
) {
    // Pass 1: emit all 44-byte entries contiguously + End terminator.
    // Collect (ref_type_pos, nested_members) for deferred emission.
    let mut deferred: Vec<(usize, &[GrannyTypeMember])> = Vec::new();

    for m in members {
        let entry_start = buf.len();
        // MemberType (u32)
        buf.extend_from_slice(&(m.member_type as u32).to_le_bytes());
        // Name (u64) — placeholder, patched by StringTable
        let name_pos = buf.len();
        buf.extend_from_slice(&0u64.to_le_bytes());
        if !m.name.is_empty() {
            strings.add(name_pos, m.name.clone());
        }
        // ReferenceType (u64) — placeholder, patched in pass 2
        let ref_type_pos = buf.len();
        buf.extend_from_slice(&0u64.to_le_bytes());
        // ArrayWidth (u32)
        buf.extend_from_slice(&m.array_width.to_le_bytes());
        // Extra[3] (12 bytes)
        for &e in &m.extra {
            buf.extend_from_slice(&e.to_le_bytes());
        }
        // Unused[2] (8 bytes)
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());

        debug_assert_eq!(buf.len() - entry_start, GRANNY_TYPE_DEF_STRIDE);

        if let Some(ref nested) = m.reference_type
            && !nested.is_empty()
        {
            deferred.push((ref_type_pos, nested));
        }
    }

    // End terminator (44 bytes of zeros)
    buf.extend_from_slice(&[0u8; GRANNY_TYPE_DEF_STRIDE]);

    // Pass 2: emit deferred nested type arrays and patch ReferenceType pointers.
    for (ref_type_pos, nested) in deferred {
        let nested_offset = buf.len();
        emit_type_def_array(buf, strings, nested);
        buf[ref_type_pos..ref_type_pos + 8].copy_from_slice(&(nested_offset as u64).to_le_bytes());
    }
}

/// Deferred pointer fixup for Reference/ReferenceToArray fields.
struct DeferredRef<'a> {
    ptr_pos: usize,
    variant: &'a GrannyVariant,
    nested_type: &'a [GrannyTypeMember],
}

struct DeferredRefArray<'a> {
    ptr_pos: usize,
    elements: &'a [(String, GrannyVariant)],
    nested_type: &'a [GrannyTypeMember],
}

/// Emit variant data described by `members` into `buf`.
///
/// Two-pass approach: first emit all top-level fields contiguously (flat
/// record), then emit deferred nested data (Reference, ReferenceToArray)
/// and patch pointers back. This ensures the reader can traverse the flat
/// record without hitting interleaved nested data.
fn emit_variant_data(
    buf: &mut Vec<u8>,
    strings: &mut super::string_table::StringTable,
    variant: &GrannyVariant,
    members: &[GrannyTypeMember],
) {
    let fields = match variant {
        GrannyVariant::Struct(fields) => fields,
        _ => return,
    };

    // Pass 1: emit all flat fields; collect deferred refs.
    let mut deferred_refs: Vec<DeferredRef> = Vec::new();
    let mut deferred_arrs: Vec<DeferredRefArray> = Vec::new();

    for (i, m) in members.iter().enumerate() {
        let value = fields.get(i).map(|(_, v)| v);

        match m.member_type {
            GrannyMemberType::Real32 => {
                let width = if m.array_width == 0 {
                    1
                } else {
                    m.array_width as usize
                };
                if let Some(GrannyVariant::Real32(vals)) = value {
                    for j in 0..width {
                        let v = vals.get(j).copied().unwrap_or(0.0);
                        buf.extend_from_slice(&v.to_le_bytes());
                    }
                } else {
                    for _ in 0..width {
                        buf.extend_from_slice(&0.0f32.to_le_bytes());
                    }
                }
            }
            GrannyMemberType::Int8 | GrannyMemberType::BinormalInt8 => {
                let width = if m.array_width == 0 {
                    1
                } else {
                    m.array_width as usize
                };
                if let Some(GrannyVariant::Int8(vals)) = value {
                    for j in 0..width {
                        buf.push(vals.get(j).copied().unwrap_or(0) as u8);
                    }
                } else {
                    buf.extend_from_slice(&vec![0u8; width]);
                }
            }
            GrannyMemberType::UInt8 | GrannyMemberType::NormalUInt8 => {
                let width = if m.array_width == 0 {
                    1
                } else {
                    m.array_width as usize
                };
                if let Some(GrannyVariant::UInt8(vals)) = value {
                    for j in 0..width {
                        buf.push(vals.get(j).copied().unwrap_or(0));
                    }
                } else {
                    buf.extend_from_slice(&vec![0u8; width]);
                }
            }
            GrannyMemberType::Int16 | GrannyMemberType::BinormalInt16 => {
                let width = if m.array_width == 0 {
                    1
                } else {
                    m.array_width as usize
                };
                if let Some(GrannyVariant::Int16(vals)) = value {
                    for j in 0..width {
                        let v = vals.get(j).copied().unwrap_or(0);
                        buf.extend_from_slice(&v.to_le_bytes());
                    }
                } else {
                    for _ in 0..width {
                        buf.extend_from_slice(&0i16.to_le_bytes());
                    }
                }
            }
            GrannyMemberType::UInt16
            | GrannyMemberType::NormalUInt16
            | GrannyMemberType::Real16 => {
                let width = if m.array_width == 0 {
                    1
                } else {
                    m.array_width as usize
                };
                if let Some(GrannyVariant::UInt16(vals)) = value {
                    for j in 0..width {
                        let v = vals.get(j).copied().unwrap_or(0);
                        buf.extend_from_slice(&v.to_le_bytes());
                    }
                } else {
                    for _ in 0..width {
                        buf.extend_from_slice(&0u16.to_le_bytes());
                    }
                }
            }
            GrannyMemberType::StringMember => {
                let str_pos = buf.len();
                buf.extend_from_slice(&0u64.to_le_bytes());
                if let Some(GrannyVariant::StringVal(s)) = value
                    && !s.is_empty()
                {
                    strings.add(str_pos, s.clone());
                }
            }
            GrannyMemberType::Reference => {
                // 8-byte pointer placeholder
                let ptr_pos = buf.len();
                buf.extend_from_slice(&0u64.to_le_bytes());
                if let Some(GrannyVariant::Reference(Some(nested))) = value
                    && let Some(ref nested_type) = m.reference_type
                {
                    deferred_refs.push(DeferredRef {
                        ptr_pos,
                        variant: nested,
                        nested_type,
                    });
                }
            }
            GrannyMemberType::VariantReference => {
                // 16 bytes: type_def_ptr + data_ptr (leave as zeros)
                buf.extend_from_slice(&0u64.to_le_bytes());
                buf.extend_from_slice(&0u64.to_le_bytes());
            }
            GrannyMemberType::Inline => {
                if let Some(ref nested_type) = m.reference_type {
                    if let Some(nested_val) = value {
                        emit_variant_data(buf, strings, nested_val, nested_type);
                    } else {
                        let size = compute_type_size(nested_type);
                        buf.extend_from_slice(&vec![0u8; size]);
                    }
                }
            }
            GrannyMemberType::Transform => {
                let size = 68
                    * (if m.array_width == 0 {
                        1
                    } else {
                        m.array_width as usize
                    });
                if let Some(GrannyVariant::RawBytes(raw)) = value {
                    buf.extend_from_slice(raw);
                    if raw.len() < size {
                        buf.extend_from_slice(&vec![0u8; size - raw.len()]);
                    }
                } else {
                    buf.extend_from_slice(&vec![0u8; size]);
                }
            }
            GrannyMemberType::ReferenceToArray => {
                // u32 count + u64 pointer placeholders
                let count_pos = buf.len();
                buf.extend_from_slice(&0u32.to_le_bytes());
                let ptr_pos = buf.len();
                buf.extend_from_slice(&0u64.to_le_bytes());

                if let Some(GrannyVariant::Reference(Some(nested))) = value
                    && let GrannyVariant::Struct(elements) = nested.as_ref()
                    && let Some(ref nested_type) = m.reference_type
                {
                    // Patch count now (it's part of the flat record)
                    buf[count_pos..count_pos + 4]
                        .copy_from_slice(&(elements.len() as u32).to_le_bytes());
                    deferred_arrs.push(DeferredRefArray {
                        ptr_pos,
                        elements,
                        nested_type,
                    });
                }
            }
            GrannyMemberType::EmptyReference | GrannyMemberType::End => {
                // No data to emit
            }
            _ => {
                // Unknown type — emit zeros based on unit size
                let size = m.member_type.unit_size().unwrap_or(0)
                    * (if m.array_width == 0 {
                        1
                    } else {
                        m.array_width as usize
                    });
                buf.extend_from_slice(&vec![0u8; size]);
            }
        }
    }

    // Pass 2: emit deferred Reference data and patch pointers.
    for dr in deferred_refs {
        while !buf.len().is_multiple_of(4) {
            buf.push(0);
        }
        let nested_offset = buf.len();
        let mut nested_strings = super::string_table::StringTable::new();
        emit_variant_data(buf, &mut nested_strings, dr.variant, dr.nested_type);
        nested_strings.write(buf);
        buf[dr.ptr_pos..dr.ptr_pos + 8].copy_from_slice(&(nested_offset as u64).to_le_bytes());
    }

    // Pass 2b: emit deferred ReferenceToArray data and patch pointers.
    for da in deferred_arrs {
        while !buf.len().is_multiple_of(4) {
            buf.push(0);
        }
        let arr_offset = buf.len();
        buf[da.ptr_pos..da.ptr_pos + 8].copy_from_slice(&(arr_offset as u64).to_le_bytes());
        for (_, elem) in da.elements {
            let mut elem_strings = super::string_table::StringTable::new();
            emit_variant_data(buf, &mut elem_strings, elem, da.nested_type);
            elem_strings.write(buf);
        }
    }
}

/// Compute the total byte size of a type definition (sum of all member sizes).
fn compute_type_size(members: &[GrannyTypeMember]) -> usize {
    let mut total = 0;
    for m in members {
        let unit = match m.member_type {
            GrannyMemberType::Inline => {
                if let Some(ref nested) = m.reference_type {
                    compute_type_size(nested)
                } else {
                    0
                }
            }
            other => other.unit_size().unwrap_or(0),
        };
        let width = if m.array_width == 0 {
            1
        } else {
            m.array_width as usize
        };
        total += unit * width;
    }
    total
}
