//! Granny chunk (0x703) parser.
//!
//! Parses granny bones (inverse world matrices), granny meshes
//! (bone bindings per mesh), and bone ExtendedData (Granny2 variant system)
//! from the Granny2-compatible serialized chunk.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::bytes::{
    read_f32_le, read_i32_le, read_null_terminated_string, read_u32_le, read_u64_le,
};
use crate::error::Result;
use crate::raw::{GRANNY_BONE_BINDING_SIZE, GRANNY_BONE_INVERSE_WORLD_OFFSET, GRANNY_BONE_SIZE};
use crate::types::{
    GrannyBone, GrannyMemberType, GrannyMesh, GrannyTypeMember, GrannyVariant, Matrix4x4,
};

/// Size of a single GrannyDataTypeDefinition on disk: 44 bytes (11 DWORDs).
const GRANNY_TYPE_DEF_STRIDE: usize = 44;

/// Offset within a bone struct where ExtendedData starts (type_ptr + data_ptr).
const GRANNY_BONE_EXTENDED_DATA_OFFSET: usize = 0x94;

// ---------------------------------------------------------------------------
// Granny2 type definition parser
// ---------------------------------------------------------------------------

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

        let mut p = pos;
        let member_type_raw = read_u32_le(data, &mut p)?;
        let name_ptr = read_u64_le(data, &mut p)? as usize;
        let ref_type_ptr = read_u64_le(data, &mut p)? as usize;
        let array_width = read_u32_le(data, &mut p)?;
        let extra0 = read_u32_le(data, &mut p)?;
        let extra1 = read_u32_le(data, &mut p)?;
        let extra2 = read_u32_le(data, &mut p)?;
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
            read_null_terminated_string(&data[name_ptr..]).unwrap_or_default()
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

// ---------------------------------------------------------------------------
// Granny2 variant data parser
// ---------------------------------------------------------------------------

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
                let mut p = cur;
                for _ in 0..width {
                    vals.push(read_f32_le(data, &mut p)?);
                }
                cur = p;
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
                let mut p = cur;
                for _ in 0..width {
                    let v = read_u32_le(data, &mut p).map(|x| x as i16).unwrap_or(0);
                    // Actually i16, read 2 bytes
                    vals.push(v);
                }
                // Fix: read as proper i16
                let mut p2 = cur;
                vals.clear();
                for _ in 0..width {
                    if p2 + 2 <= data.len() {
                        let v = i16::from_le_bytes([data[p2], data[p2 + 1]]);
                        vals.push(v);
                        p2 += 2;
                    }
                }
                cur = p2;
                GrannyVariant::Int16(vals)
            }
            GrannyMemberType::UInt16
            | GrannyMemberType::NormalUInt16
            | GrannyMemberType::Real16 => {
                let mut vals = Vec::with_capacity(width);
                let mut p = cur;
                for _ in 0..width {
                    if p + 2 <= data.len() {
                        let v = u16::from_le_bytes([data[p], data[p + 1]]);
                        vals.push(v);
                        p += 2;
                    }
                }
                cur = p;
                GrannyVariant::UInt16(vals)
            }
            GrannyMemberType::StringMember => {
                // 8-byte pointer to null-terminated string
                let mut p = cur;
                let str_ptr = read_u64_le(data, &mut p)? as usize;
                cur = p;
                let s = if str_ptr > 0 && str_ptr < data.len() {
                    read_null_terminated_string(&data[str_ptr..]).unwrap_or_default()
                } else {
                    String::new()
                };
                GrannyVariant::StringVal(s)
            }
            GrannyMemberType::Reference => {
                // 8-byte pointer to nested data
                let mut p = cur;
                let ref_ptr = read_u64_le(data, &mut p)? as usize;
                cur = p;
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
                let mut p = cur;
                let type_ptr = read_u64_le(data, &mut p)? as usize;
                let data_ptr = read_u64_le(data, &mut p)? as usize;
                cur = p;
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
                let mut p = cur;
                let count = read_u32_le(data, &mut p)? as usize;
                let arr_ptr = read_u64_le(data, &mut p)? as usize;
                cur = p;
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
pub(super) fn parse_granny_bones(granny: &[u8]) -> Result<Vec<GrannyBone>> {
    if granny.len() < 0x40 {
        return Ok(Vec::new());
    }

    let mut p = 0x30usize;
    let skeleton_count = read_u32_le(granny, &mut p)? as usize;
    if skeleton_count == 0 {
        return Ok(Vec::new());
    }

    let skeleton_ptr_array_offs = read_u64_le(granny, &mut p)? as usize;
    if skeleton_ptr_array_offs + 8 > granny.len() {
        return Ok(Vec::new());
    }

    let mut p = skeleton_ptr_array_offs;
    let skeleton_offs = read_u64_le(granny, &mut p)? as usize;
    if skeleton_offs + 0x14 > granny.len() {
        return Ok(Vec::new());
    }

    let mut p = skeleton_offs + 0x08;
    let bones_len = read_u32_le(granny, &mut p)? as usize;
    let bones_offs = read_u64_le(granny, &mut p)? as usize;

    if bones_len == 0 {
        return Ok(Vec::new());
    }

    let mut bones = Vec::with_capacity(bones_len);

    for i in 0..bones_len {
        let bone_start = bones_offs + (i * GRANNY_BONE_SIZE);
        if bone_start + GRANNY_BONE_SIZE > granny.len() {
            break;
        }

        let mut p = bone_start;
        let name_offs = read_u64_le(granny, &mut p)? as usize;
        let parent_index = read_i32_le(granny, &mut p)?;

        let name = if name_offs < granny.len() {
            read_null_terminated_string(&granny[name_offs..])?
        } else {
            String::new()
        };

        let mut p = bone_start + GRANNY_BONE_INVERSE_WORLD_OFFSET;
        if p + 64 > granny.len() {
            break;
        }

        let mut rows = [[0.0f32; 4]; 4];
        for row in &mut rows {
            for col in row {
                *col = read_f32_le(granny, &mut p)?;
            }
        }
        let inverse_world_matrix = Matrix4x4 { rows };

        // Parse ExtendedData at bone+0x94: {type_def_ptr (u64), data_ptr (u64)}
        let ext_offset = bone_start + GRANNY_BONE_EXTENDED_DATA_OFFSET;
        let (extended_data, extended_data_type) = if ext_offset + 16 <= granny.len() {
            let mut ep = ext_offset;
            let type_ptr = read_u64_le(granny, &mut ep)? as usize;
            let data_ptr = read_u64_le(granny, &mut ep)? as usize;

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
            inverse_world_matrix,
            extended_data,
            extended_data_type,
        });
    }

    Ok(bones)
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

    let mut p = 0x60usize;
    let model_count = read_u32_le(granny, &mut p)? as usize;
    if model_count == 0 {
        return Ok(Vec::new());
    }

    let models_ptr_offs = read_u64_le(granny, &mut p)? as usize;
    if models_ptr_offs + 8 > granny.len() {
        return Ok(Vec::new());
    }

    let mut p = models_ptr_offs;
    let model_offs = read_u64_le(granny, &mut p)? as usize;
    if model_offs + 0x60 > granny.len() {
        return Ok(Vec::new());
    }

    let mut p = model_offs + 0x54;
    let mesh_binding_count = read_u32_le(granny, &mut p)? as usize;
    let mesh_bindings_ptr = read_u64_le(granny, &mut p)? as usize;

    if mesh_binding_count == 0 {
        return Ok(Vec::new());
    }

    let mut meshes = Vec::with_capacity(mesh_binding_count);

    for i in 0..mesh_binding_count {
        let mut bp = mesh_bindings_ptr + i * 8;
        if bp + 8 > granny.len() {
            break;
        }

        let mesh_offs = read_u64_le(granny, &mut bp)? as usize;
        if mesh_offs + 0x3C > granny.len() {
            continue;
        }

        let mut np = mesh_offs;
        let name_ptr = read_u64_le(granny, &mut np)? as usize;
        let name = if name_ptr < granny.len() {
            read_null_terminated_string(&granny[name_ptr..])?
        } else {
            format!("mesh_{}", i)
        };

        let mut bbp = mesh_offs + 0x30;
        let bone_binding_count = read_u32_le(granny, &mut bbp)? as usize;
        let bone_bindings_ptr = read_u64_le(granny, &mut bbp)? as usize;

        let mut bone_bindings = Vec::with_capacity(bone_binding_count);

        for j in 0..bone_binding_count {
            let mut bb_p = bone_bindings_ptr + j * GRANNY_BONE_BINDING_SIZE;
            if bb_p + 8 > granny.len() {
                break;
            }

            let bone_name_ptr = read_u64_le(granny, &mut bb_p)? as usize;
            let bone_name = if bone_name_ptr < granny.len() {
                read_null_terminated_string(&granny[bone_name_ptr..])?
            } else {
                String::new()
            };

            if !bone_name.is_empty() {
                bone_bindings.push(bone_name);
            }
        }

        meshes.push(GrannyMesh {
            name,
            bone_bindings,
        });
    }

    Ok(meshes)
}
