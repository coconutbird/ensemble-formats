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
use crate::types::{
    GrannyBoneBinding, GrannyMemberType, GrannyTypeMember, GrannyVariant, Matrix4x4, UgxGeom,
};

/// Size of a single GrannyDataTypeDefinition on disk.
const GRANNY_TYPE_DEF_STRIDE: usize = 44;

/// Offset within a bone struct where ExtendedData {type_ptr, data_ptr} lives.
const GRANNY_BONE_EXTENDED_DATA_OFFSET: usize = 0x94;

/// Align to 16-byte boundary (matching original engine alignment).
fn align16(n: usize) -> usize {
    (n + 15) & !15
}

/// Build the granny bones chunk (0x703).
///
/// Produces a Granny2-compatible serialized chunk matching the original engine
/// layout:
///   1. File info header (0x94 bytes)
///   2. Skeleton pointer array (16-byte aligned)
///   3. Skeleton struct (16-byte aligned)
///   4. Bone array (16-byte aligned)
///   5. ExtendedData type definitions and data blobs
///   6. Mesh pointer array + mesh structs + bone bindings
///   7. Model pointer array + model struct + model mesh bindings
///   8. String table (all names collected at the end)
///
/// When bones have stored `local_transform` data (from a previous read), it is
/// written back verbatim for bit-perfect round-tripping. Otherwise, local
/// transforms are derived from inverse world matrices (lossy fallback).
pub(super) fn build_granny_data(geom: &UgxGeom) -> Result<Vec<u8>> {
    let bone_count = geom.granny_bones.len();
    let section_count = geom.sections.len();

    // ---- Determine mesh data ----
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

    // Allocate buffer through bone array end
    let mut buf = vec![0u8; bones_end];
    let mut strings = super::string_table::StringTable::new();

    // ---- File info header [0x00..0x94] ----
    // FileName string "gr2ugx"
    strings.add(0x10, "gr2ugx".to_string());

    // Skeleton RTA at +0x30
    {
        let mut cursor = MutCursor::new(&mut buf);
        cursor.seek(SeekFrom::Start(0x30))?;
        cursor.write_u32_le(1)?; // SkeletonCount
        cursor.write_u64_le(skel_ptr_array as u64)?;
    }
    // Meshes and Models RTAs (+0x54, +0x60) are patched later

    // ---- Skeleton pointer array ----
    buf[skel_ptr_array..skel_ptr_array + 8].copy_from_slice(&(skel_struct as u64).to_le_bytes());

    // ---- Skeleton struct ----
    strings.add(skel_struct, "GrannyRootBone".to_string());
    {
        let mut cursor = MutCursor::new(&mut buf);
        cursor.seek(SeekFrom::Start((skel_struct + 0x08) as u64))?;
        cursor.write_u32_le(bone_count as u32)?;
        cursor.write_u64_le(bones_start as u64)?;
        cursor.write_u32_le(geom.skeleton_lod_type)?; // LODType
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
        let mut cursor = MutCursor::new(&mut buf);
        for (i, bone) in geom.granny_bones.iter().enumerate() {
            let base = bones_start + i * GRANNY_BONE_SIZE;
            strings.add(base, bone.name.clone());

            cursor.seek(SeekFrom::Start((base + 0x08) as u64))?;
            cursor.write_i32_le(bone.parent_index)?;

            if let Some(ref lt) = bone.local_transform {
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
            } else {
                let fb = &fallback_transforms.as_ref().unwrap()[i];
                cursor.write_u32_le(fb.flags)?;
                for &v in &fb.position {
                    cursor.write_f32_le(v)?;
                }
                for &v in &fb.orientation {
                    cursor.write_f32_le(v)?;
                }
                for row in &fb.scale_shear {
                    for &v in row {
                        cursor.write_f32_le(v)?;
                    }
                }
            }

            for row in &bone.inverse_world_matrix.rows {
                for &val in row {
                    cursor.write_f32_le(val)?;
                }
            }
            cursor.write_f32_le(bone.lod_error)?;
        }
    }

    // ---- Phase 2: Extended data (appended after bones) ----
    // Collect unique type definitions and assign indices
    let mut unique_type_defs: Vec<Vec<GrannyTypeMember>> = Vec::new();
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

    // Emit type definitions (16-byte aligned) using the shared string table
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

    // Emit data blobs for each bone's extended data (16-byte aligned)
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

    // Patch bone ExtendedData pointers: bone+0x94 = {type_def_ptr, data_ptr}
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

    // Grow buffer to hold mesh ptrs + mesh structs + bone bindings
    buf.resize(after_bone_bindings, 0);

    // Patch file_info Meshes RTA at +0x54
    buf[0x54..0x58].copy_from_slice(&(mesh_count as u32).to_le_bytes());
    buf[0x58..0x60].copy_from_slice(&(mesh_ptrs_start as u64).to_le_bytes());

    // Mesh pointer array
    for i in 0..mesh_count {
        let ptr_pos = mesh_ptrs_start + i * 8;
        let mesh_struct_pos = mesh_structs_start + i * GRANNY_MESH_SIZE;
        buf[ptr_pos..ptr_pos + 8].copy_from_slice(&(mesh_struct_pos as u64).to_le_bytes());
    }

    // Mesh structs + bone binding arrays
    let mut current_bb_offset = bone_bindings_start;
    for i in 0..mesh_count {
        let mesh_struct_pos = mesh_structs_start + i * GRANNY_MESH_SIZE;
        let bb_count = mesh_bone_bindings[i].len();

        strings.add(mesh_struct_pos, mesh_names[i].clone());

        // +0x30: BoneBindingCount
        buf[mesh_struct_pos + 0x30..mesh_struct_pos + 0x34]
            .copy_from_slice(&(bb_count as u32).to_le_bytes());
        if bb_count > 0 {
            buf[mesh_struct_pos + 0x34..mesh_struct_pos + 0x3C]
                .copy_from_slice(&(current_bb_offset as u64).to_le_bytes());
        }

        // Write bone binding entries (name + OBB + triangle indices)
        for (j, binding) in mesh_bone_bindings[i].iter().enumerate() {
            let binding_pos = current_bb_offset + j * GRANNY_BONE_BINDING_SIZE;
            strings.add(binding_pos, binding.bone_name.clone());

            // OBBMin[3] at +0x08
            let mut cursor = MutCursor::new(&mut buf);
            cursor.seek(SeekFrom::Start((binding_pos + 0x08) as u64))?;
            for &v in &binding.obb_min {
                cursor.write_f32_le(v)?;
            }
            // OBBMax[3] at +0x14
            for &v in &binding.obb_max {
                cursor.write_f32_le(v)?;
            }
            // TriangleIndices RTA at +0x20: count(i32) + ptr(u64)
            // Triangle indices data is deferred — we write count=0/ptr=0 for now
            // (original UGX files typically have empty triangle indices)
            cursor.write_i32_le(binding.triangle_indices.len() as i32)?;
            cursor.write_u64_le(0)?; // ptr patched later if non-empty
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

    // Patch file_info Models RTA at +0x60
    buf[0x60..0x64].copy_from_slice(&1u32.to_le_bytes());
    buf[0x64..0x6C].copy_from_slice(&(model_ptr_array as u64).to_le_bytes());

    // Model pointer array -> model struct
    buf[model_ptr_array..model_ptr_array + 8].copy_from_slice(&(model_struct as u64).to_le_bytes());

    // Model struct
    strings.add(model_struct, "GrannyRootBone".to_string());
    // +0x08: Skeleton reference
    buf[model_struct + 0x08..model_struct + 0x10]
        .copy_from_slice(&(skel_struct as u64).to_le_bytes());

    // +0x10: InitialPlacement (identity transform: flags=0, pos=0, ori=[0,0,0,1], scale=I)
    {
        let mut cursor = MutCursor::new(&mut buf);
        cursor.seek(SeekFrom::Start((model_struct + 0x10) as u64))?;
        cursor.write_u32_le(0)?; // Flags
        for _ in 0..3 {
            cursor.write_f32_le(0.0)?;
        }
        cursor.write_f32_le(0.0)?;
        cursor.write_f32_le(0.0)?;
        cursor.write_f32_le(0.0)?;
        cursor.write_f32_le(1.0)?;
        cursor.write_f32_le(1.0)?;
        cursor.write_f32_le(0.0)?;
        cursor.write_f32_le(0.0)?;
        cursor.write_f32_le(0.0)?;
        cursor.write_f32_le(1.0)?;
        cursor.write_f32_le(0.0)?;
        cursor.write_f32_le(0.0)?;
        cursor.write_f32_le(0.0)?;
        cursor.write_f32_le(1.0)?;
    }

    // +0x54: MeshBindingCount + MeshBindings pointer
    buf[model_struct + 0x54..model_struct + 0x58]
        .copy_from_slice(&(mesh_count as u32).to_le_bytes());
    buf[model_struct + 0x58..model_struct + 0x60]
        .copy_from_slice(&(mesh_bindings_start as u64).to_le_bytes());

    // Model mesh bindings (pointers to mesh structs)
    for i in 0..mesh_count {
        let binding_pos = mesh_bindings_start + i * 8;
        let mesh_struct_pos = mesh_structs_start + i * GRANNY_MESH_SIZE;
        buf[binding_pos..binding_pos + 8].copy_from_slice(&(mesh_struct_pos as u64).to_le_bytes());
    }

    // ---- Phase 5: FileInfo type definition tree ----
    // Build and emit the Granny2 schema that describes all structures in the chunk.
    // The engine scans for contiguous 44-byte type def entries between the data
    // and the string table.
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
            GrannyMemberType::Int32 => {
                let width = if m.array_width == 0 {
                    1
                } else {
                    m.array_width as usize
                };
                if let Some(GrannyVariant::Int32(vals)) = value {
                    for j in 0..width {
                        let v = vals.get(j).copied().unwrap_or(0);
                        buf.extend_from_slice(&v.to_le_bytes());
                    }
                } else {
                    for _ in 0..width {
                        buf.extend_from_slice(&0i32.to_le_bytes());
                    }
                }
            }
            GrannyMemberType::UInt32 => {
                let width = if m.array_width == 0 {
                    1
                } else {
                    m.array_width as usize
                };
                if let Some(GrannyVariant::UInt32(vals)) = value {
                    for j in 0..width {
                        let v = vals.get(j).copied().unwrap_or(0);
                        buf.extend_from_slice(&v.to_le_bytes());
                    }
                } else {
                    for _ in 0..width {
                        buf.extend_from_slice(&0u32.to_le_bytes());
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
        emit_variant_data(buf, strings, dr.variant, dr.nested_type);
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
            emit_variant_data(buf, strings, elem, da.nested_type);
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

// ---------------------------------------------------------------------------
// Fallback local transform derivation (used when bones lack stored transforms)
// ---------------------------------------------------------------------------

struct FallbackTransform {
    flags: u32,
    position: [f32; 3],
    orientation: [f32; 4],
    scale_shear: [[f32; 3]; 3],
}

/// Derive local transforms from inverse world matrices for bones that lack
/// stored transform data (e.g. after a glTF round-trip).
fn compute_fallback_local_transforms(geom: &UgxGeom) -> Vec<FallbackTransform> {
    let bone_count = geom.granny_bones.len();
    let world_matrices: Vec<Matrix4x4> = geom
        .granny_bones
        .iter()
        .map(|bone| bone.inverse_world_matrix.inverse().unwrap_or_default())
        .collect();

    geom.granny_bones
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

            let mut orientation = rot_matrix.to_quaternion();
            // Granny stores quaternions in conjugate form (negated xyz).
            // Standard matrix decomposition gives q where v' = q*v*q⁻¹,
            // but Granny uses q⁻¹*v*q, so we conjugate (negate xyz).
            orientation[0] = -orientation[0];
            orientation[1] = -orientation[1];
            orientation[2] = -orientation[2];
            // Canonical sign: ensure w >= 0 (q and -q are the same rotation).
            if orientation[3] < 0.0 {
                orientation[0] = -orientation[0];
                orientation[1] = -orientation[1];
                orientation[2] = -orientation[2];
                orientation[3] = -orientation[3];
            }
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

            // Flag thresholds: matrix inversion + multiply + decomposition
            // can introduce drift up to ~1e-5 for values that should be zero
            // or identity. Use 1e-4 to comfortably absorb this drift without
            // incorrectly marking real transforms as identity (real bone
            // offsets are orders of magnitude larger).
            const FLAG_EPS: f32 = 1e-4;

            let mut flags = 0u32;
            if position[0].abs() > FLAG_EPS
                || position[1].abs() > FLAG_EPS
                || position[2].abs() > FLAG_EPS
            {
                flags |= GRANNY_HAS_POSITION;
            }
            if (orientation[0].abs() > FLAG_EPS)
                || (orientation[1].abs() > FLAG_EPS)
                || (orientation[2].abs() > FLAG_EPS)
                || ((orientation[3] - 1.0).abs() > FLAG_EPS)
            {
                flags |= GRANNY_HAS_ORIENTATION;
            }
            let is_identity_scale = (scale_shear[0][0] - 1.0).abs() < FLAG_EPS
                && scale_shear[0][1].abs() < FLAG_EPS
                && scale_shear[0][2].abs() < FLAG_EPS
                && scale_shear[1][0].abs() < FLAG_EPS
                && (scale_shear[1][1] - 1.0).abs() < FLAG_EPS
                && scale_shear[1][2].abs() < FLAG_EPS
                && scale_shear[2][0].abs() < FLAG_EPS
                && scale_shear[2][1].abs() < FLAG_EPS
                && (scale_shear[2][2] - 1.0).abs() < FLAG_EPS;
            if !is_identity_scale {
                flags |= GRANNY_HAS_SCALE_SHEAR;
            }

            FallbackTransform {
                flags,
                position,
                orientation,
                scale_shear,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// FileInfo type tree builder
// ---------------------------------------------------------------------------

/// Helper: create a simple scalar/string member with no nested type.
fn tm(member_type: GrannyMemberType, name: &str) -> GrannyTypeMember {
    GrannyTypeMember {
        member_type,
        name: String::from(name),
        reference_type: None,
        array_width: 0,
        extra: [0; 3],
    }
}

/// Helper: create a member with a nested reference type.
fn tm_ref(
    member_type: GrannyMemberType,
    name: &str,
    nested: Vec<GrannyTypeMember>,
) -> GrannyTypeMember {
    GrannyTypeMember {
        member_type,
        name: String::from(name),
        reference_type: Some(nested),
        array_width: 0,
        extra: [0; 3],
    }
}

/// Helper: create a Real32 member with a specific array width.
fn tm_real32_array(name: &str, width: u32) -> GrannyTypeMember {
    GrannyTypeMember {
        member_type: GrannyMemberType::Real32,
        name: String::from(name),
        reference_type: None,
        array_width: width,
        extra: [0; 3],
    }
}

/// Helper: create an Int32 member with a specific array width.
fn tm_int32_array(name: &str, width: u32) -> GrannyTypeMember {
    GrannyTypeMember {
        member_type: GrannyMemberType::Int32,
        name: String::from(name),
        reference_type: None,
        array_width: width,
        extra: [0; 3],
    }
}

/// Build the bone type definition matching the engine's hardcoded
/// `GrannyBoneTypeDef` at `0x1414621D0`.
///
/// ```text
/// Name              : String (8 bytes)
/// ParentIndex       : Int32  (4 bytes)
/// Transform         : Transform (68 bytes, opaque GrannyTransform)
/// InverseWorld4x4   : Real32 ×16 (64 bytes, 4×4 float matrix)
/// LODError          : Real32 (4 bytes)
/// ExtendedData      : VariantReference (16 bytes)
/// Total: 0xA4 = 164 bytes
/// ```
fn build_bone_type() -> Vec<GrannyTypeMember> {
    vec![
        tm(GrannyMemberType::StringMember, "Name"),
        tm(GrannyMemberType::Int32, "ParentIndex"),
        tm(GrannyMemberType::Transform, "Transform"),
        tm_real32_array("InverseWorldTransform", 16),
        tm(GrannyMemberType::Real32, "LODError"),
        tm(GrannyMemberType::VariantReference, "ExtendedData"),
    ]
}

/// Build the bone_binding type definition matching the engine's at `0x141460EA0`.
fn build_bone_binding_type() -> Vec<GrannyTypeMember> {
    vec![
        tm(GrannyMemberType::StringMember, "BoneName"),
        tm_real32_array("OBBMin", 3),
        tm_real32_array("OBBMax", 3),
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "TriangleIndices",
            vec![tm(GrannyMemberType::Int32, "Int32")],
        ),
    ]
}

/// Build the VertexData type definition matching the engine's at `0x14145E3E0`.
fn build_vertex_data_type() -> Vec<GrannyTypeMember> {
    vec![
        tm(GrannyMemberType::ReferenceToVariantArray, "Vertices"),
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "VertexComponentNames",
            vec![tm(GrannyMemberType::StringMember, "String")],
        ),
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "VertexAnnotationSets",
            vec![
                tm(GrannyMemberType::StringMember, "Name"),
                tm(
                    GrannyMemberType::ReferenceToVariantArray,
                    "VertexAnnotations",
                ),
                tm(GrannyMemberType::Int32, "IndicesMapFromVertexToAnnotation"),
                tm_ref(
                    GrannyMemberType::ReferenceToArray,
                    "VertexAnnotationIndices",
                    vec![tm(GrannyMemberType::Int32, "Int32")],
                ),
            ],
        ),
    ]
}

/// Build the TriTopology type definition matching the engine's at `0x14145F1A0`.
fn build_tri_topology_type() -> Vec<GrannyTypeMember> {
    let int32_elem = vec![tm(GrannyMemberType::Int32, "Int32")];
    let int16_elem = vec![tm(GrannyMemberType::Int16, "Int16")];
    vec![
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "Groups",
            vec![
                tm(GrannyMemberType::Int32, "MaterialIndex"),
                tm(GrannyMemberType::Int32, "TriFirst"),
                tm(GrannyMemberType::Int32, "TriCount"),
            ],
        ),
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "Indices",
            int32_elem.clone(),
        ),
        tm_ref(GrannyMemberType::ReferenceToArray, "Indices16", int16_elem),
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "VertexToVertexMap",
            int32_elem.clone(),
        ),
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "VertexToTriangleMap",
            int32_elem.clone(),
        ),
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "SideToNeighborMap",
            int32_elem.clone(),
        ),
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "PolygonIndexStarts",
            int32_elem.clone(),
        ),
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "PolygonIndices",
            int32_elem.clone(),
        ),
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "BonesForTriangle",
            int32_elem.clone(),
        ),
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "TriangleToBoneIndices",
            int32_elem,
        ),
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "TriAnnotationSets",
            vec![
                tm(GrannyMemberType::StringMember, "Name"),
                tm(GrannyMemberType::ReferenceToVariantArray, "TriAnnotations"),
                tm(GrannyMemberType::Int32, "IndicesMapFromTriToAnnotation"),
                tm_ref(
                    GrannyMemberType::ReferenceToArray,
                    "TriAnnotationIndices",
                    vec![tm(GrannyMemberType::Int32, "Int32")],
                ),
            ],
        ),
    ]
}

/// Build the mesh type definition matching the engine's at `0x141461090`.
fn build_mesh_type() -> Vec<GrannyTypeMember> {
    vec![
        tm(GrannyMemberType::StringMember, "Name"),
        tm_ref(
            GrannyMemberType::Reference,
            "PrimaryVertexData",
            build_vertex_data_type(),
        ),
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "MorphTargets",
            vec![
                tm(GrannyMemberType::StringMember, "ScalarName"),
                tm_ref(
                    GrannyMemberType::Reference,
                    "VertexData",
                    build_vertex_data_type(),
                ),
                tm(GrannyMemberType::Int32, "DataIsDeltas"),
            ],
        ),
        tm_ref(
            GrannyMemberType::Reference,
            "PrimaryTopology",
            build_tri_topology_type(),
        ),
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "MaterialBindings",
            // MaterialBinding -> Reference to Material (recursive with Texture)
            vec![tm_ref(
                GrannyMemberType::Reference,
                "Material",
                build_material_type(),
            )],
        ),
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "BoneBindings",
            build_bone_binding_type(),
        ),
        tm(GrannyMemberType::VariantReference, "ExtendedData"),
    ]
}

/// Build the skeleton type definition matching the engine's at `0x141462310`.
fn build_skeleton_type() -> Vec<GrannyTypeMember> {
    vec![
        tm(GrannyMemberType::StringMember, "Name"),
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "Bones",
            build_bone_type(),
        ),
        tm(GrannyMemberType::Int32, "LODType"),
        tm(GrannyMemberType::VariantReference, "ExtendedData"),
    ]
}

/// Build the model type definition matching the engine's at `0x14145C980`.
fn build_model_type() -> Vec<GrannyTypeMember> {
    vec![
        tm(GrannyMemberType::StringMember, "Name"),
        tm_ref(
            GrannyMemberType::Reference,
            "Skeleton",
            build_skeleton_type(),
        ),
        tm(GrannyMemberType::Transform, "InitialPlacement"),
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "MeshBindings",
            vec![tm_ref(
                GrannyMemberType::Reference,
                "Mesh",
                build_mesh_type(),
            )],
        ),
        tm(GrannyMemberType::VariantReference, "ExtendedData"),
    ]
}

/// Build the ArtToolInfo type definition matching the engine's at `0x141461330`.
fn build_art_tool_info_type() -> Vec<GrannyTypeMember> {
    vec![
        tm(GrannyMemberType::StringMember, "FromArtToolName"),
        tm(GrannyMemberType::Int32, "ArtToolMajorRevision"),
        tm(GrannyMemberType::Int32, "ArtToolMinorRevision"),
        tm(GrannyMemberType::Int32, "ArtToolPointerSize"),
        tm(GrannyMemberType::Real32, "UnitsPerMeter"),
        tm_real32_array("Origin", 3),
        tm_real32_array("RightVector", 3),
        tm_real32_array("UpVector", 3),
        tm_real32_array("BackVector", 3),
        tm(GrannyMemberType::VariantReference, "ExtendedData"),
    ]
}

/// Build the ExporterInfo type definition matching the engine's at `0x1414611F0`.
fn build_exporter_info_type() -> Vec<GrannyTypeMember> {
    vec![
        tm(GrannyMemberType::StringMember, "ExporterName"),
        tm(GrannyMemberType::Int32, "ExporterMajorRevision"),
        tm(GrannyMemberType::Int32, "ExporterMinorRevision"),
        tm(GrannyMemberType::Int32, "ExporterCustomization"),
        tm(GrannyMemberType::Int32, "ExporterBuildNumber"),
        tm(GrannyMemberType::VariantReference, "ExtendedData"),
    ]
}

/// Build the Texture type definition matching the engine's at `0x1414623F0`.
fn build_texture_type() -> Vec<GrannyTypeMember> {
    vec![
        tm(GrannyMemberType::StringMember, "FromFileName"),
        tm(GrannyMemberType::Int32, "TextureType"),
        tm(GrannyMemberType::Int32, "Width"),
        tm(GrannyMemberType::Int32, "Height"),
        tm(GrannyMemberType::Int32, "Encoding"),
        tm(GrannyMemberType::Int32, "SubFormat"),
        tm_ref(
            GrannyMemberType::Inline,
            "Layout",
            vec![
                tm(GrannyMemberType::Int32, "BytesPerPixel"),
                tm_int32_array("ShiftForComponent", 4),
                tm_int32_array("BitsForComponent", 4),
            ],
        ),
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "Images",
            vec![tm_ref(
                GrannyMemberType::ReferenceToArray,
                "MIPLevels",
                vec![
                    tm(GrannyMemberType::Int32, "Stride"),
                    tm_ref(
                        GrannyMemberType::ReferenceToArray,
                        "PixelBytes",
                        vec![tm(GrannyMemberType::UInt8, "UInt8")],
                    ),
                ],
            )],
        ),
        tm(GrannyMemberType::VariantReference, "ExtendedData"),
    ]
}

/// Build the Material type definition matching the engine's at `0x141461E60`.
fn build_material_type() -> Vec<GrannyTypeMember> {
    vec![
        tm(GrannyMemberType::StringMember, "Name"),
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "Maps",
            vec![
                tm(GrannyMemberType::StringMember, "Usage"),
                // Material.Maps[].Map is a circular Reference back to Material.
                // We break the cycle by omitting the nested type (empty ref).
                tm_ref(GrannyMemberType::Reference, "Map", Vec::new()),
            ],
        ),
        tm_ref(GrannyMemberType::Reference, "Texture", build_texture_type()),
        tm(GrannyMemberType::VariantReference, "ExtendedData"),
    ]
}

/// Build the TrackGroup type definition matching the engine's at `0x14145CDA0`.
fn build_track_group_type() -> Vec<GrannyTypeMember> {
    vec![
        tm(GrannyMemberType::StringMember, "Name"),
        // VectorTracks, TransformTracks, etc. have deep nesting into curve data.
        // We include the member names but use empty nested refs for curves.
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "VectorTracks",
            Vec::new(),
        ),
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "TransformTracks",
            Vec::new(),
        ),
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "TransformLODErrors",
            Vec::new(),
        ),
        tm_ref(GrannyMemberType::ReferenceToArray, "TextTracks", Vec::new()),
        tm(GrannyMemberType::Transform, "InitialPlacement"),
        tm(GrannyMemberType::Int32, "AccumulationFlags"),
        tm_real32_array("LoopTranslation", 3),
        tm_ref(GrannyMemberType::Reference, "PeriodicLoop", Vec::new()),
        tm(GrannyMemberType::VariantReference, "ExtendedData"),
    ]
}

/// Build the Animation type definition matching the engine's at `0x141462890`.
fn build_animation_type() -> Vec<GrannyTypeMember> {
    vec![
        tm(GrannyMemberType::StringMember, "Name"),
        tm(GrannyMemberType::Real32, "Duration"),
        tm(GrannyMemberType::Real32, "TimeStep"),
        tm(GrannyMemberType::Real32, "Oversampling"),
        tm_ref(
            GrannyMemberType::ArrayOfReferences,
            "TrackGroups",
            build_track_group_type(),
        ),
        tm(GrannyMemberType::Int32, "DefaultLoopCount"),
        tm(GrannyMemberType::Int32, "Flags"),
        tm(GrannyMemberType::VariantReference, "ExtendedData"),
    ]
}

/// Build the complete FileInfo type definition tree matching the engine's
/// hardcoded `GrannyFileInfoTypeDef` at `0x14145C7D0` → `0x141461B60`.
///
/// This constructs the Granny2 schema matching the `file_info` struct layout:
/// ```text
/// struct file_info {
///     art_tool_info *ArtToolInfo;       // Reference
///     exporter_info *ExporterInfo;      // Reference
///     char const *FromFileName;         // String
///     texture **Textures;              // ArrayOfReferences
///     material **Materials;            // ArrayOfReferences
///     skeleton **Skeletons;            // ArrayOfReferences
///     vertex_data **VertexDatas;       // ArrayOfReferences
///     tri_topology **TriTopologies;    // ArrayOfReferences
///     mesh **Meshes;                   // ArrayOfReferences
///     model **Models;                  // ArrayOfReferences
///     track_group **TrackGroups;       // ArrayOfReferences
///     animation **Animations;          // ArrayOfReferences
///     variant ExtendedData;            // VariantReference
/// };
/// ```
fn build_file_info_type_tree() -> Vec<GrannyTypeMember> {
    vec![
        tm_ref(
            GrannyMemberType::Reference,
            "ArtToolInfo",
            build_art_tool_info_type(),
        ),
        tm_ref(
            GrannyMemberType::Reference,
            "ExporterInfo",
            build_exporter_info_type(),
        ),
        tm(GrannyMemberType::StringMember, "FromFileName"),
        tm_ref(
            GrannyMemberType::ArrayOfReferences,
            "Textures",
            build_texture_type(),
        ),
        tm_ref(
            GrannyMemberType::ArrayOfReferences,
            "Materials",
            build_material_type(),
        ),
        tm_ref(
            GrannyMemberType::ArrayOfReferences,
            "Skeletons",
            build_skeleton_type(),
        ),
        tm_ref(
            GrannyMemberType::ArrayOfReferences,
            "VertexDatas",
            build_vertex_data_type(),
        ),
        tm_ref(
            GrannyMemberType::ArrayOfReferences,
            "TriTopologies",
            build_tri_topology_type(),
        ),
        tm_ref(
            GrannyMemberType::ArrayOfReferences,
            "Meshes",
            build_mesh_type(),
        ),
        tm_ref(
            GrannyMemberType::ArrayOfReferences,
            "Models",
            build_model_type(),
        ),
        tm_ref(
            GrannyMemberType::ArrayOfReferences,
            "TrackGroups",
            build_track_group_type(),
        ),
        tm_ref(
            GrannyMemberType::ArrayOfReferences,
            "Animations",
            build_animation_type(),
        ),
        tm(GrannyMemberType::VariantReference, "ExtendedData"),
    ]
}
