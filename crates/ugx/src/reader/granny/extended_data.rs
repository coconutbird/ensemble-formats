//! Granny2 type definition and variant data parsing.
//!
//! Parses `GrannyDataTypeDefinition[]` arrays (the Granny2 schema/type system)
//! and their associated variant data blobs from a serialized Granny2 chunk.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::constants::GRANNY_TYPE_DEF_STRIDE;
use crate::error::Result;
use crate::types::{GrannyMemberType, GrannyTypeMember, GrannyVariant};
use nostdio::{ReadLe, SliceCursor, read_null_terminated_string};

/// Parse a `GrannyDataTypeDefinition[]` array starting at `offset` in `data`.
///
/// Each entry is 44 bytes. The array is terminated by an entry with `MemberType == 0` (End).
/// Nested reference types are parsed recursively.
pub(super) fn parse_type_def_array(data: &[u8], offset: usize) -> Result<Vec<GrannyTypeMember>> {
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
pub(super) fn parse_variant_data(
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
