//! `ExtendedData` emission helpers for Granny2 type definitions and variant data.

use alloc::string::String;
use alloc::vec::Vec;

use crate::constants::GRANNY_TYPE_DEF_STRIDE;
use crate::error::{Error, Result};
use crate::types::{GrannyMemberType, GrannyTypeMember, GrannyVariant};

/// Patch a little-endian pointer in an already-emitted buffer.
fn patch_pointer(buf: &mut [u8], position: usize, target: usize) -> Result<()> {
    let end = position
        .checked_add(core::mem::size_of::<u64>())
        .ok_or(Error::SizeOverflow("Granny pointer fixup"))?;
    let destination = buf
        .get_mut(position..end)
        .ok_or_else(|| Error::UnexpectedEof {
            context: "Granny pointer fixup".into(),
        })?;
    destination.copy_from_slice(
        &u64::try_from(target)
            .map_err(|_| Error::SizeOverflow("Granny pointer"))?
            .to_le_bytes(),
    );
    Ok(())
}

/// Convert zero-as-one Granny array widths to a target-sized count.
fn member_width(member: &GrannyTypeMember) -> Result<usize> {
    crate::checked_usize(
        u64::from(member.array_width.max(1)),
        "Granny member array width",
    )
}

/// Check if two type definition arrays have the same layout.
pub(super) fn type_defs_equal(a: &[GrannyTypeMember], b: &[GrannyTypeMember]) -> bool {
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

/// Emit a `GrannyDataTypeDefinition`[] array (with End terminator) into `buf`.
///
/// Two-pass approach: first emit all entries contiguously (engine traverses
/// with stride=44), then emit nested type arrays afterward and patch the
/// `ReferenceType` pointers.
pub(super) fn emit_type_def_array(
    buf: &mut Vec<u8>,
    strings: &mut crate::writer::string_table::StringTable,
    members: &[GrannyTypeMember],
) -> Result<()> {
    // Pass 1: emit all 44-byte entries contiguously + End terminator.
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

        if buf.len() - entry_start != GRANNY_TYPE_DEF_STRIDE {
            return Err(Error::SizeOverflow("Granny type-definition entry"));
        }

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
        emit_type_def_array(buf, strings, nested)?;
        patch_pointer(buf, ref_type_pos, nested_offset)?;
    }
    Ok(())
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

/// Extend a buffer with a checked number of zero bytes.
fn append_zeroes(buf: &mut Vec<u8>, count: usize) -> Result<()> {
    let new_len = buf
        .len()
        .checked_add(count)
        .ok_or(Error::SizeOverflow("Granny variant data"))?;
    buf.resize(new_len, 0);
    Ok(())
}

/// Patch a little-endian `u32` field in an already-emitted buffer.
fn patch_u32(buf: &mut [u8], position: usize, value: u32) -> Result<()> {
    let end = position
        .checked_add(core::mem::size_of::<u32>())
        .ok_or(Error::SizeOverflow("Granny count fixup"))?;
    let destination = buf
        .get_mut(position..end)
        .ok_or_else(|| Error::UnexpectedEof {
            context: "Granny count fixup".into(),
        })?;
    destination.copy_from_slice(&value.to_le_bytes());
    Ok(())
}

/// Emit numeric member data, returning whether the type was handled.
fn emit_numeric_member(
    buf: &mut Vec<u8>,
    value: Option<&GrannyVariant>,
    member_type: GrannyMemberType,
    width: usize,
) -> bool {
    match member_type {
        GrannyMemberType::Real32 => {
            let values = match value {
                Some(GrannyVariant::Real32(values)) => values.as_slice(),
                _ => &[],
            };
            for index in 0..width {
                buf.extend_from_slice(
                    &values.get(index).copied().unwrap_or_default().to_le_bytes(),
                );
            }
        }
        GrannyMemberType::Int8 | GrannyMemberType::BinormalInt8 => {
            let values = match value {
                Some(GrannyVariant::Int8(values)) => values.as_slice(),
                _ => &[],
            };
            for index in 0..width {
                buf.push(
                    values
                        .get(index)
                        .copied()
                        .unwrap_or_default()
                        .cast_unsigned(),
                );
            }
        }
        GrannyMemberType::UInt8 | GrannyMemberType::NormalUInt8 => {
            let values = match value {
                Some(GrannyVariant::UInt8(values)) => values.as_slice(),
                _ => &[],
            };
            for index in 0..width {
                buf.push(values.get(index).copied().unwrap_or_default());
            }
        }
        GrannyMemberType::Int16 | GrannyMemberType::BinormalInt16 => {
            let values = match value {
                Some(GrannyVariant::Int16(values)) => values.as_slice(),
                _ => &[],
            };
            for index in 0..width {
                buf.extend_from_slice(
                    &values.get(index).copied().unwrap_or_default().to_le_bytes(),
                );
            }
        }
        GrannyMemberType::UInt16 | GrannyMemberType::NormalUInt16 | GrannyMemberType::Real16 => {
            let values = match value {
                Some(GrannyVariant::UInt16(values)) => values.as_slice(),
                _ => &[],
            };
            for index in 0..width {
                buf.extend_from_slice(
                    &values.get(index).copied().unwrap_or_default().to_le_bytes(),
                );
            }
        }
        GrannyMemberType::Int32 => {
            let values = match value {
                Some(GrannyVariant::Int32(values)) => values.as_slice(),
                _ => &[],
            };
            for index in 0..width {
                buf.extend_from_slice(
                    &values.get(index).copied().unwrap_or_default().to_le_bytes(),
                );
            }
        }
        GrannyMemberType::UInt32 => {
            let values = match value {
                Some(GrannyVariant::UInt32(values)) => values.as_slice(),
                _ => &[],
            };
            for index in 0..width {
                buf.extend_from_slice(
                    &values.get(index).copied().unwrap_or_default().to_le_bytes(),
                );
            }
        }
        _ => return false,
    }
    true
}

/// Emit pointer-based, inline, and opaque member data.
fn emit_complex_member<'a>(
    buf: &mut Vec<u8>,
    strings: &mut crate::writer::string_table::StringTable,
    value: Option<&'a GrannyVariant>,
    member: &'a GrannyTypeMember,
    width: usize,
    deferred_refs: &mut Vec<DeferredRef<'a>>,
    deferred_arrays: &mut Vec<DeferredRefArray<'a>>,
) -> Result<()> {
    match member.member_type {
        GrannyMemberType::StringMember => {
            let string_position = buf.len();
            append_zeroes(buf, 8)?;
            if let Some(GrannyVariant::StringVal(value)) = value
                && !value.is_empty()
            {
                strings.add(string_position, value.clone());
            }
        }
        GrannyMemberType::Reference => {
            let pointer_position = buf.len();
            append_zeroes(buf, 8)?;
            if let Some(GrannyVariant::Reference(Some(nested))) = value
                && let Some(nested_type) = member.reference_type.as_deref()
            {
                deferred_refs.push(DeferredRef {
                    ptr_pos: pointer_position,
                    variant: nested,
                    nested_type,
                });
            }
        }
        GrannyMemberType::VariantReference => append_zeroes(buf, 16)?,
        GrannyMemberType::Inline => {
            if let Some(nested_type) = member.reference_type.as_deref() {
                if let Some(nested_value) = value {
                    emit_variant_data(buf, strings, nested_value, nested_type)?;
                } else {
                    append_zeroes(buf, compute_type_size(nested_type)?)?;
                }
            }
        }
        GrannyMemberType::Transform => {
            let size = 68usize
                .checked_mul(width)
                .ok_or(Error::SizeOverflow("Granny transform"))?;
            let raw = match value {
                Some(GrannyVariant::RawBytes(raw)) => raw.as_slice(),
                _ => &[],
            };
            let copy_len = raw.len().min(size);
            buf.extend_from_slice(&raw[..copy_len]);
            append_zeroes(buf, size - copy_len)?;
        }
        GrannyMemberType::ReferenceToArray => {
            let count_position = buf.len();
            append_zeroes(buf, 4)?;
            let pointer_position = buf.len();
            append_zeroes(buf, 8)?;
            if let Some(GrannyVariant::Reference(Some(nested))) = value
                && let GrannyVariant::Struct(elements) = nested.as_ref()
                && let Some(nested_type) = member.reference_type.as_deref()
            {
                patch_u32(
                    buf,
                    count_position,
                    crate::checked_u32(elements.len(), "Granny reference-array count")?,
                )?;
                deferred_arrays.push(DeferredRefArray {
                    ptr_pos: pointer_position,
                    elements,
                    nested_type,
                });
            }
        }
        GrannyMemberType::EmptyReference | GrannyMemberType::End => {}
        other => {
            let size = other
                .unit_size()
                .unwrap_or_default()
                .checked_mul(width)
                .ok_or(Error::SizeOverflow("Granny member data"))?;
            append_zeroes(buf, size)?;
        }
    }
    Ok(())
}

/// Emit variant data described by `members` into `buf`.
///
/// Two-pass approach: first emit all top-level fields contiguously (flat
/// record), then emit deferred nested data (Reference, `ReferenceToArray`)
/// and patch pointers back.
pub(super) fn emit_variant_data(
    buf: &mut Vec<u8>,
    strings: &mut crate::writer::string_table::StringTable,
    variant: &GrannyVariant,
    members: &[GrannyTypeMember],
) -> Result<()> {
    let GrannyVariant::Struct(fields) = variant else {
        return Ok(());
    };

    // Pass 1: emit all flat fields; collect deferred refs.
    let mut deferred_refs: Vec<DeferredRef<'_>> = Vec::new();
    let mut deferred_arrs: Vec<DeferredRefArray<'_>> = Vec::new();

    for (index, member) in members.iter().enumerate() {
        let value = fields.get(index).map(|(_, value)| value);
        let width = member_width(member)?;
        if !emit_numeric_member(buf, value, member.member_type, width) {
            emit_complex_member(
                buf,
                strings,
                value,
                member,
                width,
                &mut deferred_refs,
                &mut deferred_arrs,
            )?;
        }
    }

    // Pass 2: emit deferred Reference data and patch pointers.
    for deferred in deferred_refs {
        while !buf.len().is_multiple_of(4) {
            buf.push(0);
        }
        let nested_offset = buf.len();
        emit_variant_data(buf, strings, deferred.variant, deferred.nested_type)?;
        patch_pointer(buf, deferred.ptr_pos, nested_offset)?;
    }

    // Pass 2b: emit deferred ReferenceToArray data and patch pointers.
    for deferred in deferred_arrs {
        while !buf.len().is_multiple_of(4) {
            buf.push(0);
        }
        let arr_offset = buf.len();
        patch_pointer(buf, deferred.ptr_pos, arr_offset)?;
        for (_, element) in deferred.elements {
            emit_variant_data(buf, strings, element, deferred.nested_type)?;
        }
    }
    Ok(())
}

/// Compute the total byte size of a type definition (sum of all member sizes).
pub(super) fn compute_type_size(members: &[GrannyTypeMember]) -> Result<usize> {
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
