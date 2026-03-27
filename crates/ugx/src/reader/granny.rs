//! Granny chunk (0x703) parser.
//!
//! Parses granny bones (inverse world matrices), granny meshes
//! (bone bindings per mesh), and bone ExtendedData (Granny2 variant system)
//! from the Granny2-compatible serialized chunk.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::constants::{
    GRANNY_BONE_BINDING_SIZE, GRANNY_BONE_INVERSE_WORLD_OFFSET, GRANNY_BONE_SIZE,
};
use crate::error::{Error, Result};
use crate::types::{
    GrannyBone, GrannyBoneBinding, GrannyLocalTransform, GrannyMemberType, GrannyMesh,
    GrannyTypeMember, GrannyVariant, Matrix4x4,
};
use nostdio::{ReadLe, SliceCursor, read_null_terminated_string};

/// Size of a single GrannyDataTypeDefinition on disk: 44 bytes (11 DWORDs).
const GRANNY_TYPE_DEF_STRIDE: usize = 44;

/// Offset within a bone struct where ExtendedData starts (type_ptr + data_ptr).
const GRANNY_BONE_EXTENDED_DATA_OFFSET: usize = 0x94;

/// Parse a `GrannyDataTypeDefinition[]` array starting at `offset` in `data`.
///
/// Each entry is 44 bytes. The array is terminated by an entry with `MemberType == 0` (End).
/// Nested reference types are parsed recursively.
fn parse_type_def_array(data: &[u8], offset: usize) -> Result<Vec<GrannyTypeMember>> {
    let mut members = Vec::new();
    let mut pos = offset;

    // Guard against infinite recursion / corrupt data
    for _ in 0..256 {
        if pos + GRANNY_TYPE_DEF_STRIDE > data.len() {
            break;
        }

        let mut cur = SliceCursor::new(&data[pos..]);
        let member_type_raw = cur.read_u32_le()?;
        let name_ptr = cur.read_u64_le()? as usize;
        let ref_type_ptr = cur.read_u64_le()? as usize;
        let array_width = cur.read_u32_le()?;
        let extra0 = cur.read_u32_le()?;
        let extra1 = cur.read_u32_le()?;
        let extra2 = cur.read_u32_le()?;
        // skip 2 unused u32s (we already read 7*4 + 8 + 8 = 44 bytes)

        let member_type = match GrannyMemberType::from_u32(member_type_raw) {
            Some(GrannyMemberType::End) => break, // End marker
            Some(t) => t,
            None => {
                // Unknown type — skip it but continue
                pos += GRANNY_TYPE_DEF_STRIDE;
                continue;
            }
        };

        let name = if name_ptr > 0 && name_ptr < data.len() {
            read_null_terminated_string(&data[name_ptr..])
        } else {
            String::new()
        };

        // Recursively parse nested type definitions for Reference/Inline types
        let reference_type = if ref_type_ptr > 0 && ref_type_ptr < data.len() {
            match member_type {
                GrannyMemberType::Inline
                | GrannyMemberType::Reference
                | GrannyMemberType::ReferenceToArray
                | GrannyMemberType::ArrayOfReferences
                | GrannyMemberType::VariantReference
                | GrannyMemberType::ReferenceToVariantArray => {
                    Some(parse_type_def_array(data, ref_type_ptr)?)
                }
                _ => None,
            }
        } else {
            None
        };

        members.push(GrannyTypeMember {
            member_type,
            name,
            reference_type,
            array_width,
            extra: [extra0, extra1, extra2],
        });

        pos += GRANNY_TYPE_DEF_STRIDE;
    }

    Ok(members)
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

/// Parse variant data described by `members` from `data` starting at `offset`.
///
/// Returns a `GrannyVariant::Struct` containing all parsed fields.
fn parse_variant_data(
    data: &[u8],
    offset: usize,
    members: &[GrannyTypeMember],
) -> Result<GrannyVariant> {
    let mut fields = Vec::new();
    let mut cur = offset;

    for m in members {
        let width = if m.array_width == 0 {
            1
        } else {
            m.array_width as usize
        };

        let value = match m.member_type {
            GrannyMemberType::Real32 => {
                let mut vals = Vec::with_capacity(width);
                let mut sc = SliceCursor::new(&data[cur..]);
                for _ in 0..width {
                    vals.push(sc.read_f32_le()?);
                }
                cur += sc.position();
                GrannyVariant::Real32(vals)
            }
            GrannyMemberType::Int8 | GrannyMemberType::BinormalInt8 => {
                let end = cur + width;
                let vals: Vec<i8> = if end <= data.len() {
                    data[cur..end].iter().map(|&b| b as i8).collect()
                } else {
                    Vec::new()
                };
                cur = end;
                GrannyVariant::Int8(vals)
            }
            GrannyMemberType::UInt8 | GrannyMemberType::NormalUInt8 => {
                let end = cur + width;
                let vals: Vec<u8> = if end <= data.len() {
                    data[cur..end].to_vec()
                } else {
                    Vec::new()
                };
                cur = end;
                GrannyVariant::UInt8(vals)
            }
            GrannyMemberType::Int16 | GrannyMemberType::BinormalInt16 => {
                let mut vals = Vec::with_capacity(width);
                let mut sc = SliceCursor::new(&data[cur..]);
                for _ in 0..width {
                    vals.push(sc.read_i16_le()?);
                }
                cur += sc.position();
                GrannyVariant::Int16(vals)
            }
            GrannyMemberType::UInt16
            | GrannyMemberType::NormalUInt16
            | GrannyMemberType::Real16 => {
                let mut vals = Vec::with_capacity(width);
                let mut sc = SliceCursor::new(&data[cur..]);
                for _ in 0..width {
                    vals.push(sc.read_u16_le()?);
                }
                cur += sc.position();
                GrannyVariant::UInt16(vals)
            }
            GrannyMemberType::Int32 => {
                let mut vals = Vec::with_capacity(width);
                let mut sc = SliceCursor::new(&data[cur..]);
                for _ in 0..width {
                    vals.push(sc.read_i32_le()?);
                }
                cur += sc.position();
                GrannyVariant::Int32(vals)
            }
            GrannyMemberType::UInt32 => {
                let mut vals = Vec::with_capacity(width);
                let mut sc = SliceCursor::new(&data[cur..]);
                for _ in 0..width {
                    vals.push(sc.read_u32_le()?);
                }
                cur += sc.position();
                GrannyVariant::UInt32(vals)
            }
            GrannyMemberType::StringMember => {
                // 8-byte pointer to null-terminated string
                let mut sc = SliceCursor::new(&data[cur..]);
                let str_ptr = sc.read_u64_le()? as usize;
                cur += sc.position();
                let s = if str_ptr > 0 && str_ptr < data.len() {
                    read_null_terminated_string(&data[str_ptr..])
                } else {
                    String::new()
                };
                GrannyVariant::StringVal(s)
            }
            GrannyMemberType::Reference => {
                // 8-byte pointer to nested data
                let mut sc = SliceCursor::new(&data[cur..]);
                let ref_ptr = sc.read_u64_le()? as usize;
                cur += sc.position();
                if ref_ptr > 0 && ref_ptr < data.len() {
                    if let Some(ref nested_type) = m.reference_type {
                        let nested = parse_variant_data(data, ref_ptr, nested_type)?;
                        GrannyVariant::Reference(Some(Box::new(nested)))
                    } else {
                        GrannyVariant::Reference(None)
                    }
                } else {
                    GrannyVariant::Reference(None)
                }
            }
            GrannyMemberType::VariantReference => {
                // 16 bytes: type_def_ptr (u64) + data_ptr (u64)
                let mut sc = SliceCursor::new(&data[cur..]);
                let type_ptr = sc.read_u64_le()? as usize;
                let data_ptr = sc.read_u64_le()? as usize;
                cur += sc.position();
                if type_ptr > 0 && type_ptr < data.len() && data_ptr > 0 && data_ptr < data.len() {
                    let nested_type = parse_type_def_array(data, type_ptr)?;
                    let nested = parse_variant_data(data, data_ptr, &nested_type)?;
                    GrannyVariant::VariantReference(Some(Box::new(nested)))
                } else {
                    GrannyVariant::VariantReference(None)
                }
            }
            GrannyMemberType::Inline => {
                if let Some(ref nested_type) = m.reference_type {
                    let nested = parse_variant_data(data, cur, nested_type)?;
                    let size = compute_type_size(nested_type);
                    cur += size * width;
                    nested
                } else {
                    GrannyVariant::Empty
                }
            }
            GrannyMemberType::Transform => {
                // 68 bytes raw
                let size = 68 * width;
                let end = cur + size;
                let raw = if end <= data.len() {
                    data[cur..end].to_vec()
                } else {
                    Vec::new()
                };
                cur = end;
                GrannyVariant::RawBytes(raw)
            }
            GrannyMemberType::ReferenceToArray => {
                // u32 count + u64 pointer
                let mut sc = SliceCursor::new(&data[cur..]);
                let count = sc.read_u32_le()? as usize;
                let arr_ptr = sc.read_u64_le()? as usize;
                cur += sc.position();
                if count > 0 && arr_ptr > 0 && arr_ptr < data.len() {
                    if let Some(ref nested_type) = m.reference_type {
                        let elem_size = compute_type_size(nested_type);
                        let mut elements = Vec::with_capacity(count);
                        for i in 0..count {
                            let elem_offset = arr_ptr + i * elem_size;
                            if elem_offset + elem_size <= data.len() {
                                elements.push(parse_variant_data(data, elem_offset, nested_type)?);
                            }
                        }
                        GrannyVariant::Reference(Some(Box::new(GrannyVariant::Struct(
                            elements
                                .into_iter()
                                .enumerate()
                                .map(|(i, v)| (format!("{}", i), v))
                                .collect(),
                        ))))
                    } else {
                        GrannyVariant::Reference(None)
                    }
                } else {
                    GrannyVariant::Reference(None)
                }
            }
            GrannyMemberType::EmptyReference | GrannyMemberType::End => GrannyVariant::Empty,
            _ => {
                // Unknown — skip based on unit size
                let size = m.member_type.unit_size().unwrap_or(0) * width;
                let end = cur + size;
                let raw = if end <= data.len() {
                    data[cur..end].to_vec()
                } else {
                    Vec::new()
                };
                cur = end;
                GrannyVariant::RawBytes(raw)
            }
        };

        fields.push((m.name.clone(), value));
    }

    Ok(GrannyVariant::Struct(fields))
}

/// Validate the Granny chunk (0x703) by reading `FromFileName` at +0x10.
///
/// The engine (`BGrannyModel::load`) rejects the chunk unless this string is `"gr2ugx"`.
/// Returns `Err(InvalidGrannyChunk)` if the chunk is present but invalid.
pub(super) fn validate_granny_chunk(data: &[u8]) -> Result<()> {
    if data.len() < 0x18 {
        return Err(Error::InvalidGrannyChunk {
            actual: String::from("<chunk too small>"),
        });
    }
    let mut sc = SliceCursor::new(&data[0x10..]);
    let ptr = sc.read_u64_le()? as usize;
    if ptr == 0 || ptr >= data.len() {
        return Err(Error::InvalidGrannyChunk {
            actual: String::from("<null or OOB pointer>"),
        });
    }
    let name = read_null_terminated_string(&data[ptr..]);
    if !name.eq_ignore_ascii_case("gr2ugx") {
        return Err(Error::InvalidGrannyChunk { actual: name });
    }
    Ok(())
}

/// Parse granny bones from granny chunk (0x703).
///
/// Granny file_info layout (verified from IDA and real file dump):
///   +0x30: u32 SkeletonCount
///   +0x34: u64 Skeletons -> skeleton pointer array
///
/// Skeleton struct layout:
///   +0x00: u64 Name
///   +0x08: u32 BoneCount
///   +0x0C: u64 Bones -> bone array
///
/// Bone struct (164 bytes each):
///   +0x00: u64 nameOffs
///   +0x08: i32 parent
///   +0x0C: transform LocalTransform (68 bytes)
///   +0x50: matrix_4x4 InverseWorld4x4 (64 bytes)
///   +0x90: f32 LODError
///   +0x94: variant ExtendedData (16 bytes)
pub(super) fn parse_granny_bones(granny: &[u8]) -> Result<(Vec<GrannyBone>, u32)> {
    if granny.len() < 0x40 {
        return Ok((Vec::new(), 0));
    }

    let mut sc = SliceCursor::new(&granny[0x30..]);
    let skeleton_count = sc.read_u32_le()? as usize;
    if skeleton_count == 0 {
        return Ok((Vec::new(), 0));
    }

    let skeleton_ptr_array_offs = sc.read_u64_le()? as usize;
    if skeleton_ptr_array_offs + 8 > granny.len() {
        return Ok((Vec::new(), 0));
    }

    let mut sc = SliceCursor::new(&granny[skeleton_ptr_array_offs..]);
    let skeleton_offs = sc.read_u64_le()? as usize;
    if skeleton_offs + 0x28 > granny.len() {
        return Ok((Vec::new(), 0));
    }

    let mut sc = SliceCursor::new(&granny[skeleton_offs + 0x08..]);
    let bones_len = sc.read_u32_le()? as usize;
    let bones_offs = sc.read_u64_le()? as usize;
    let skeleton_lod_type = sc.read_u32_le()?;

    if bones_len == 0 {
        return Ok((Vec::new(), skeleton_lod_type));
    }

    let mut bones = Vec::with_capacity(bones_len);

    for i in 0..bones_len {
        let bone_start = bones_offs + (i * GRANNY_BONE_SIZE);
        if bone_start + GRANNY_BONE_SIZE > granny.len() {
            break;
        }

        let mut sc = SliceCursor::new(&granny[bone_start..]);
        let name_offs = sc.read_u64_le()? as usize;
        let parent_index = sc.read_i32_le()?;

        let name = if name_offs < granny.len() {
            read_null_terminated_string(&granny[name_offs..])
        } else {
            String::new()
        };

        // Parse local transform at bone+0x0C (68 bytes: flags + pos + quat + scale_shear)
        let mut sc = SliceCursor::new(&granny[bone_start + 0x0C..]);
        let lt_flags = sc.read_u32_le()?;
        let mut lt_position = [0.0f32; 3];
        for v in &mut lt_position {
            *v = sc.read_f32_le()?;
        }
        let mut lt_orientation = [0.0f32; 4];
        for v in &mut lt_orientation {
            *v = sc.read_f32_le()?;
        }
        let mut lt_scale_shear = [[0.0f32; 3]; 3];
        for row in &mut lt_scale_shear {
            for v in row {
                *v = sc.read_f32_le()?;
            }
        }
        let local_transform = Some(GrannyLocalTransform {
            flags: lt_flags,
            position: lt_position,
            orientation: lt_orientation,
            scale_shear: lt_scale_shear,
        });

        // Parse inverse world matrix at bone+0x50 (64 bytes)
        let iw_start = bone_start + GRANNY_BONE_INVERSE_WORLD_OFFSET;
        if iw_start + 64 > granny.len() {
            break;
        }

        let mut iw_pos = iw_start;
        let inverse_world_matrix = Matrix4x4::read(granny, &mut iw_pos)?;

        // Parse LOD error at bone+0x90 (4 bytes)
        let mut sc = SliceCursor::new(&granny[bone_start + 0x90..]);
        let lod_error = sc.read_f32_le()?;

        // Parse ExtendedData at bone+0x94: {type_def_ptr (u64), data_ptr (u64)}
        let ext_offset = bone_start + GRANNY_BONE_EXTENDED_DATA_OFFSET;
        let (extended_data, extended_data_type) = if ext_offset + 16 <= granny.len() {
            let mut sc = SliceCursor::new(&granny[ext_offset..]);
            let type_ptr = sc.read_u64_le()? as usize;
            let data_ptr = sc.read_u64_le()? as usize;

            if type_ptr > 0 && type_ptr < granny.len() && data_ptr > 0 && data_ptr < granny.len() {
                let type_members = parse_type_def_array(granny, type_ptr)?;
                let variant = parse_variant_data(granny, data_ptr, &type_members)?;
                (Some(variant), Some(type_members))
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };

        bones.push(GrannyBone {
            name,
            parent_index,
            local_transform,
            inverse_world_matrix,
            lod_error,
            extended_data,
            extended_data_type,
        });
    }

    Ok((bones, skeleton_lod_type))
}

/// Parse Granny mesh data from the Granny chunk (0x703).
///
/// file_info:
///   +0x60: i32 ModelCount (must be 1)
///   +0x64: u64 Models ptr (to array of model pointers)
///
/// Model:
///   +0x54: i32 MeshBindingCount
///   +0x58: u64 MeshBindings ptr (to array of mesh pointers)
///
/// Mesh (76 bytes = 0x4C):
///   +0x00: u64 Name ptr
///   +0x30: i32 BoneBindingCount
///   +0x34: u64 BoneBindings ptr
///
/// bone_binding (44 bytes = 0x2C):
///   +0x00: u64 BoneName ptr
pub(super) fn parse_granny_meshes(granny: &[u8]) -> Result<Vec<GrannyMesh>> {
    if granny.len() < 0x70 {
        return Ok(Vec::new());
    }

    let mut sc = SliceCursor::new(&granny[0x60..]);
    let model_count = sc.read_u32_le()? as usize;
    if model_count == 0 {
        return Ok(Vec::new());
    }

    let models_ptr_offs = sc.read_u64_le()? as usize;
    if models_ptr_offs + 8 > granny.len() {
        return Ok(Vec::new());
    }

    let mut sc = SliceCursor::new(&granny[models_ptr_offs..]);
    let model_offs = sc.read_u64_le()? as usize;
    if model_offs + 0x60 > granny.len() {
        return Ok(Vec::new());
    }

    let mut sc = SliceCursor::new(&granny[model_offs + 0x54..]);
    let mesh_binding_count = sc.read_u32_le()? as usize;
    let mesh_bindings_ptr = sc.read_u64_le()? as usize;

    if mesh_binding_count == 0 {
        return Ok(Vec::new());
    }

    let mut meshes = Vec::with_capacity(mesh_binding_count);

    for i in 0..mesh_binding_count {
        let bp = mesh_bindings_ptr + i * 8;
        if bp + 8 > granny.len() {
            break;
        }

        let mut bsc = SliceCursor::new(&granny[bp..]);
        let mesh_offs = bsc.read_u64_le()? as usize;
        if mesh_offs + 0x3C > granny.len() {
            continue;
        }

        let mut nsc = SliceCursor::new(&granny[mesh_offs..]);
        let name_ptr = nsc.read_u64_le()? as usize;
        let name = if name_ptr < granny.len() {
            read_null_terminated_string(&granny[name_ptr..])
        } else {
            format!("mesh_{}", i)
        };

        let mut bbsc = SliceCursor::new(&granny[mesh_offs + 0x30..]);
        let bone_binding_count = bbsc.read_u32_le()? as usize;
        let bone_bindings_ptr = bbsc.read_u64_le()? as usize;

        let mut bone_bindings = Vec::with_capacity(bone_binding_count);

        for j in 0..bone_binding_count {
            let bb_start = bone_bindings_ptr + j * GRANNY_BONE_BINDING_SIZE;
            if bb_start + GRANNY_BONE_BINDING_SIZE > granny.len() {
                break;
            }

            let mut bbsc = SliceCursor::new(&granny[bb_start..]);
            let bone_name_ptr = bbsc.read_u64_le()? as usize;
            let bone_name = if bone_name_ptr < granny.len() {
                read_null_terminated_string(&granny[bone_name_ptr..])
            } else {
                String::new()
            };

            // OBBMin[3] at +0x08
            let obb_min = [
                bbsc.read_f32_le()?,
                bbsc.read_f32_le()?,
                bbsc.read_f32_le()?,
            ];

            // OBBMax[3] at +0x14
            let obb_max = [
                bbsc.read_f32_le()?,
                bbsc.read_f32_le()?,
                bbsc.read_f32_le()?,
            ];

            // TriangleIndices RTA at +0x20: count(i32) + ptr(u64)
            let tri_count = bbsc.read_i32_le()? as usize;
            let tri_ptr = bbsc.read_u64_le()? as usize;

            let triangle_indices =
                if tri_count > 0 && tri_ptr > 0 && tri_ptr + tri_count * 4 <= granny.len() {
                    let mut indices = Vec::with_capacity(tri_count);
                    let mut tsc = SliceCursor::new(&granny[tri_ptr..]);
                    for _ in 0..tri_count {
                        indices.push(tsc.read_i32_le()?);
                    }
                    indices
                } else {
                    Vec::new()
                };

            if !bone_name.is_empty() {
                bone_bindings.push(GrannyBoneBinding {
                    bone_name,
                    obb_min,
                    obb_max,
                    triangle_indices,
                });
            }
        }

        meshes.push(GrannyMesh {
            name,
            bone_bindings,
        });
    }

    Ok(meshes)
}
