//! ExtendedData emission helpers for Granny2 type definitions and variant data.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::constants::GRANNY_TYPE_DEF_STRIDE;
use crate::types::{GrannyMemberType, GrannyTypeMember, GrannyVariant};

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

/// Emit a GrannyDataTypeDefinition[] array (with End terminator) into `buf`.
///
/// Two-pass approach: first emit all entries contiguously (engine traverses
/// with stride=44), then emit nested type arrays afterward and patch the
/// ReferenceType pointers.
pub(super) fn emit_type_def_array(
    buf: &mut Vec<u8>,
    strings: &mut crate::writer::string_table::StringTable,
    members: &[GrannyTypeMember],
) {
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
/// and patch pointers back.
pub(super) fn emit_variant_data(
    buf: &mut Vec<u8>,
    strings: &mut crate::writer::string_table::StringTable,
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
                let count_pos = buf.len();
                buf.extend_from_slice(&0u32.to_le_bytes());
                let ptr_pos = buf.len();
                buf.extend_from_slice(&0u64.to_le_bytes());

                if let Some(GrannyVariant::Reference(Some(nested))) = value
                    && let GrannyVariant::Struct(elements) = nested.as_ref()
                    && let Some(ref nested_type) = m.reference_type
                {
                    buf[count_pos..count_pos + 4]
                        .copy_from_slice(&(elements.len() as u32).to_le_bytes());
                    deferred_arrs.push(DeferredRefArray {
                        ptr_pos,
                        elements,
                        nested_type,
                    });
                }
            }
            GrannyMemberType::EmptyReference | GrannyMemberType::End => {}
            _ => {
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
pub(super) fn compute_type_size(members: &[GrannyTypeMember]) -> usize {
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
