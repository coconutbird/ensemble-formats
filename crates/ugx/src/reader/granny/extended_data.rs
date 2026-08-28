//! Granny2 type definition and variant data parsing.
//!
//! Parses `GrannyDataTypeDefinition[]` arrays (the Granny2 schema/type system)
//! and their associated variant data blobs from a serialized Granny2 chunk.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::constants::GRANNY_TYPE_DEF_STRIDE;
use crate::error::{Error, Result};
use crate::types::{GrannyMemberType, GrannyTypeMember, GrannyVariant};
use nostdio::{Cursor, ReadLe, read_null_terminated_string};

use super::{data_range, data_tail, pointer_offset};

/// Parse a `GrannyDataTypeDefinition[]` array starting at `offset` in `data`.
///
/// Each entry is 44 bytes. The array is terminated by an entry with `MemberType == 0` (End).
/// Nested reference types are parsed recursively.
pub(super) fn parse_type_def_array(data: &[u8], offset: usize) -> Result<Vec<GrannyTypeMember>> {
    let mut members = Vec::new();
    let mut pos = offset;

    // Guard against infinite recursion / corrupt data
    for _ in 0..256 {
        let entry = data_range(data, pos, GRANNY_TYPE_DEF_STRIDE, "Granny type definition")?;
        let mut cur = Cursor::new(entry);
        let member_type_raw = cur.read_u32_le()?;
        let name_pointer = cur.read_u64_le()?;
        let reference_type_pointer = cur.read_u64_le()?;
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
                pos = pos
                    .checked_add(GRANNY_TYPE_DEF_STRIDE)
                    .ok_or(Error::SizeOverflow("Granny type definition"))?;
                continue;
            }
        };

        let name = if name_pointer > 0 {
            let name_offset = pointer_offset(name_pointer, "Granny member name")?;
            data_tail(data, name_offset, "Granny member name")
                .map_or_else(|_| String::new(), read_null_terminated_string)
        } else {
            String::new()
        };

        // Recursively parse nested type definitions for Reference/Inline types
        let reference_type = if reference_type_pointer > 0 {
            let reference_type_offset =
                pointer_offset(reference_type_pointer, "Granny reference type")?;
            match member_type {
                GrannyMemberType::Inline
                | GrannyMemberType::Reference
                | GrannyMemberType::ReferenceToArray
                | GrannyMemberType::ArrayOfReferences
                | GrannyMemberType::VariantReference
                | GrannyMemberType::ReferenceToVariantArray => {
                    Some(parse_type_def_array(data, reference_type_offset)?)
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

        pos = pos
            .checked_add(GRANNY_TYPE_DEF_STRIDE)
            .ok_or(Error::SizeOverflow("Granny type definition"))?;
    }

    Ok(members)
}

/// Resolve a member's zero-as-one array width.
fn member_width(member: &GrannyTypeMember) -> Result<usize> {
    crate::checked_usize(
        u64::from(member.array_width.max(1)),
        "Granny member array width",
    )
}

/// Compute the total byte size of a type definition (sum of all member sizes).
fn compute_type_size(members: &[GrannyTypeMember]) -> Result<usize> {
    let mut total = 0usize;
    for member in members {
        let unit = match member.member_type {
            GrannyMemberType::Inline => {
                if let Some(ref nested) = member.reference_type {
                    compute_type_size(nested)?
                } else {
                    0
                }
            }
            other => other.unit_size().unwrap_or(0),
        };
        let member_size = unit
            .checked_mul(member_width(member)?)
            .ok_or(Error::SizeOverflow("Granny member size"))?;
        total = total
            .checked_add(member_size)
            .ok_or(Error::SizeOverflow("Granny type size"))?;
    }
    Ok(total)
}

/// Read a repeated numeric value from the variant-data cursor.
fn read_values<T>(
    data: &[u8],
    position: &mut usize,
    count: usize,
    mut read: impl FnMut(&mut Cursor<&[u8]>) -> Result<T>,
) -> Result<Vec<T>> {
    let mut cursor = Cursor::new(data_tail(data, *position, "Granny variant data")?);
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        values.push(read(&mut cursor)?);
    }
    crate::advance_position(position, cursor.position(), "Granny data cursor")?;
    Ok(values)
}

/// Copy raw bytes from the variant-data cursor and advance it safely.
fn read_raw_bytes(data: &[u8], position: &mut usize, size: usize) -> Result<Vec<u8>> {
    let bytes = data_range(data, *position, size, "Granny variant bytes")?.to_vec();
    *position = position
        .checked_add(size)
        .ok_or(Error::SizeOverflow("Granny variant bytes"))?;
    Ok(bytes)
}

/// Read a serialized pointer from the variant-data cursor.
fn read_pointer(data: &[u8], position: &mut usize) -> Result<u64> {
    let mut cursor = Cursor::new(data_tail(data, *position, "Granny pointer")?);
    let pointer = cursor.read_u64_le()?;
    crate::advance_position(position, cursor.position(), "Granny data cursor")?;
    Ok(pointer)
}

/// Parse scalar and fixed-width numeric member types.
fn parse_numeric_member(
    data: &[u8],
    position: &mut usize,
    member_type: GrannyMemberType,
    width: usize,
) -> Result<Option<GrannyVariant>> {
    let value = match member_type {
        GrannyMemberType::Real32 => {
            GrannyVariant::Real32(read_values(data, position, width, |cursor| {
                Ok(cursor.read_f32_le()?)
            })?)
        }
        GrannyMemberType::Int8 | GrannyMemberType::BinormalInt8 => {
            let bytes = read_raw_bytes(data, position, width)?;
            GrannyVariant::Int8(bytes.into_iter().map(u8::cast_signed).collect())
        }
        GrannyMemberType::UInt8 | GrannyMemberType::NormalUInt8 => {
            GrannyVariant::UInt8(read_raw_bytes(data, position, width)?)
        }
        GrannyMemberType::Int16 | GrannyMemberType::BinormalInt16 => {
            GrannyVariant::Int16(read_values(data, position, width, |cursor| {
                Ok(cursor.read_i16_le()?)
            })?)
        }
        GrannyMemberType::UInt16 | GrannyMemberType::NormalUInt16 | GrannyMemberType::Real16 => {
            GrannyVariant::UInt16(read_values(data, position, width, |cursor| {
                Ok(cursor.read_u16_le()?)
            })?)
        }
        GrannyMemberType::Int32 => {
            GrannyVariant::Int32(read_values(data, position, width, |cursor| {
                Ok(cursor.read_i32_le()?)
            })?)
        }
        GrannyMemberType::UInt32 => {
            GrannyVariant::UInt32(read_values(data, position, width, |cursor| {
                Ok(cursor.read_u32_le()?)
            })?)
        }
        _ => return Ok(None),
    };
    Ok(Some(value))
}

/// Parse a `ReferenceToArray` member and its nested elements.
fn parse_reference_array(
    data: &[u8],
    position: &mut usize,
    member: &GrannyTypeMember,
) -> Result<GrannyVariant> {
    let mut cursor = Cursor::new(data_tail(data, *position, "Granny reference array")?);
    let count = crate::checked_usize(
        u64::from(cursor.read_u32_le()?),
        "Granny reference-array count",
    )?;
    let array_pointer = cursor.read_u64_le()?;
    crate::advance_position(position, cursor.position(), "Granny data cursor")?;

    let Some(nested_type) = member.reference_type.as_ref() else {
        return Ok(GrannyVariant::Reference(None));
    };
    if count == 0 || array_pointer == 0 {
        return Ok(GrannyVariant::Reference(None));
    }

    let array_offset = pointer_offset(array_pointer, "Granny reference array")?;
    let element_size = compute_type_size(nested_type)?;
    let mut elements = Vec::with_capacity(count);
    for index in 0..count {
        let relative = index
            .checked_mul(element_size)
            .ok_or(Error::SizeOverflow("Granny reference-array offset"))?;
        let element_offset = array_offset
            .checked_add(relative)
            .ok_or(Error::SizeOverflow("Granny reference-array offset"))?;
        data_range(
            data,
            element_offset,
            element_size,
            "Granny reference-array element",
        )?;
        elements.push((
            format!("{index}"),
            parse_variant_data(data, element_offset, nested_type)?,
        ));
    }

    Ok(GrannyVariant::Reference(Some(Box::new(
        GrannyVariant::Struct(elements),
    ))))
}

/// Parse pointer-based, inline, and opaque member types.
fn parse_complex_member(
    data: &[u8],
    position: &mut usize,
    member: &GrannyTypeMember,
    width: usize,
) -> Result<GrannyVariant> {
    match member.member_type {
        GrannyMemberType::StringMember => {
            let pointer = read_pointer(data, position)?;
            let value = if pointer == 0 {
                String::new()
            } else {
                let offset = pointer_offset(pointer, "Granny string")?;
                data_tail(data, offset, "Granny string")
                    .map_or_else(|_| String::new(), read_null_terminated_string)
            };
            Ok(GrannyVariant::StringVal(value))
        }
        GrannyMemberType::Reference => {
            let pointer = read_pointer(data, position)?;
            let Some(nested_type) = member.reference_type.as_ref() else {
                return Ok(GrannyVariant::Reference(None));
            };
            if pointer == 0 {
                return Ok(GrannyVariant::Reference(None));
            }
            let offset = pointer_offset(pointer, "Granny reference")?;
            data_tail(data, offset, "Granny reference")?;
            let nested = parse_variant_data(data, offset, nested_type)?;
            Ok(GrannyVariant::Reference(Some(Box::new(nested))))
        }
        GrannyMemberType::VariantReference => {
            let mut cursor = Cursor::new(data_tail(data, *position, "Granny variant reference")?);
            let type_pointer = cursor.read_u64_le()?;
            let data_pointer = cursor.read_u64_le()?;
            crate::advance_position(position, cursor.position(), "Granny data cursor")?;
            if type_pointer == 0 || data_pointer == 0 {
                return Ok(GrannyVariant::VariantReference(None));
            }
            let type_offset = pointer_offset(type_pointer, "Granny variant type")?;
            let data_offset = pointer_offset(data_pointer, "Granny variant value")?;
            let nested_type = parse_type_def_array(data, type_offset)?;
            let nested = parse_variant_data(data, data_offset, &nested_type)?;
            Ok(GrannyVariant::VariantReference(Some(Box::new(nested))))
        }
        GrannyMemberType::Inline => {
            let Some(nested_type) = member.reference_type.as_ref() else {
                return Ok(GrannyVariant::Empty);
            };
            let nested = parse_variant_data(data, *position, nested_type)?;
            let size = compute_type_size(nested_type)?
                .checked_mul(width)
                .ok_or(Error::SizeOverflow("Granny inline data"))?;
            *position = position
                .checked_add(size)
                .ok_or(Error::SizeOverflow("Granny inline data"))?;
            Ok(nested)
        }
        GrannyMemberType::Transform => {
            let size = 68usize
                .checked_mul(width)
                .ok_or(Error::SizeOverflow("Granny transform"))?;
            Ok(GrannyVariant::RawBytes(read_raw_bytes(
                data, position, size,
            )?))
        }
        GrannyMemberType::ReferenceToArray => parse_reference_array(data, position, member),
        GrannyMemberType::EmptyReference | GrannyMemberType::End => Ok(GrannyVariant::Empty),
        other => {
            let size = other
                .unit_size()
                .unwrap_or(0)
                .checked_mul(width)
                .ok_or(Error::SizeOverflow("Granny member data"))?;
            Ok(GrannyVariant::RawBytes(read_raw_bytes(
                data, position, size,
            )?))
        }
    }
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
    let mut position = offset;

    for member in members {
        let width = member_width(member)?;
        let value = if let Some(value) =
            parse_numeric_member(data, &mut position, member.member_type, width)?
        {
            value
        } else {
            parse_complex_member(data, &mut position, member, width)?
        };

        fields.push((member.name.clone(), value));
    }

    Ok(GrannyVariant::Struct(fields))
}
