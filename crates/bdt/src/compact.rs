//! Compact `BPackedHeader` format reader.
//!
//! This is the native `BBinaryDataTree` serialization format, used by material
//! chunks and other non-XMB packed data. The format uses:
//! - `BPackedHeader` (28 bytes): signature, CRC, section sizes
//! - `BPackedNode` (8 bytes): compact node with 16-bit indices
//! - `BPackedNameValue` (8 bytes): name/value pair with type flags
//!
//! Section layout after header:
//! [User sections (12 bytes each)]
//! [Node section]
//! [`NameValue` section]
//! [`NameData` section (null-terminated strings)]
//! [Padding to 16-byte boundary]
//! [`ValueData` section (16-byte aligned)]

use alloc::string::String;
use alloc::vec::Vec;
use zerocopy::Ref;

use crate::error::{Error, Result};
use crate::node::{Attribute, Node};
use crate::raw::{BPackedHeader, CompactNodeRaw, CompactNvRaw};
use crate::util::{assemble_tree, read_null_terminated_string};
use crate::variant::Variant;

/// `BPackedHeader` signature for little-endian data.
const PACKED_HEADER_SIG_LE: u8 = 0x3E;
/// `BPackedHeader` signature for big-endian data.
const PACKED_HEADER_SIG_BE: u8 = 0xE3;
const PACKED_HEADER_SIZE: usize = 28;

/// Check if data at the given offset starts with a compact format signature.
pub(crate) fn is_compact_signature(data: &[u8], offset: usize) -> bool {
    offset < data.len()
        && (data[offset] == PACKED_HEADER_SIG_LE || data[offset] == PACKED_HEADER_SIG_BE)
}

/// Read compact format (`BPackedHeader` with 0x3E/0xE3 signature).
pub(crate) fn read_compact(
    data: &[u8],
    header_offset: usize,
    big_endian: bool,
) -> Result<Option<Node>> {
    let header_data = data.get(header_offset..).ok_or(Error::UnexpectedEof)?;
    let (header, _): (Ref<_, BPackedHeader>, _) =
        Ref::from_prefix(header_data).map_err(|_| Error::UnexpectedEof)?;

    let expected_sig = if big_endian {
        PACKED_HEADER_SIG_BE
    } else {
        PACKED_HEADER_SIG_LE
    };
    if header.signature != expected_sig {
        return Err(Error::UnexpectedEof);
    }
    let num_user_sections = header.num_user_sections as usize;

    let node_section_size = header.node_section_size(big_endian);
    let nv_section_size = header.nv_section_size(big_endian);
    let name_data_size = header.name_data_size(big_endian);
    let value_data_size = header.value_data_size(big_endian);

    // Calculate section offsets
    let user_sections_offset = header_offset + PACKED_HEADER_SIZE;
    let node_offset = user_sections_offset + num_user_sections * 12;
    let nv_offset = node_offset + node_section_size;
    let name_data_offset = nv_offset + nv_section_size;
    let value_data_offset_unaligned = name_data_offset + name_data_size;
    let value_data_offset = if value_data_size > 0 {
        (value_data_offset_unaligned + 15) & !15
    } else {
        value_data_offset_unaligned
    };

    // Bounds check
    if value_data_offset + value_data_size > data.len() {
        return Err(Error::UnexpectedEof);
    }

    let node_count = node_section_size / 8;
    let nv_count = nv_section_size / 8;

    if node_count == 0 {
        return Ok(None);
    }

    // Parse packed nodes via zerocopy
    let node_bytes = data.get(node_offset..).ok_or(Error::UnexpectedEof)?;
    let (nodes_slice, _): (Ref<_, [CompactNodeRaw]>, _) =
        Ref::from_prefix_with_elems(node_bytes, node_count).map_err(|_| Error::UnexpectedEof)?;

    // Parse packed name-values via zerocopy
    let nv_bytes = data.get(nv_offset..).ok_or(Error::UnexpectedEof)?;
    let (nvs_slice, _): (Ref<_, [CompactNvRaw]>, _) =
        Ref::from_prefix_with_elems(nv_bytes, nv_count).map_err(|_| Error::UnexpectedEof)?;

    // Section slices
    let name_data = &data[name_data_offset..name_data_offset + name_data_size];
    let value_data = &data[value_data_offset..value_data_offset + value_data_size];

    // Build tree
    Ok(build_compact_tree(
        &nodes_slice,
        &nvs_slice,
        name_data,
        value_data,
        big_endian,
    ))
}

/// `BPackedNameValue` flag constants (from binaryDataTree.h).
mod nv_flags {
    pub const TYPE_IS_UNSIGNED: u16 = 0x0001;
    pub const DIRECT_ENCODING: u16 = 0x0002;
    pub const TYPE_SHIFT: u16 = 2;
    pub const TYPE_MASK: u16 = 0x001C; // 3 bits
    pub const TYPE_SIZE_LOG2_SHIFT: u16 = 5;
    pub const TYPE_SIZE_LOG2_MASK: u16 = 0x00E0; // 3 bits
    pub const LAST_NAME_VALUE: u16 = 0x0100;
    pub const SIZE_SHIFT: u16 = 9;
    pub const SIZE_MASK: u16 = 0xFE00; // 7 bits
}

/// Type class enum (from binaryDataTree.h).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
enum TypeClass {
    Null = 0,
    Bool = 1,
    Int = 2,
    Float = 3,
    String = 4,
}

impl TypeClass {
    fn from_flags(flags: u16) -> Self {
        match (flags & nv_flags::TYPE_MASK) >> nv_flags::TYPE_SHIFT {
            1 => TypeClass::Bool,
            2 => TypeClass::Int,
            3 => TypeClass::Float,
            4 => TypeClass::String,
            _ => TypeClass::Null,
        }
    }
}

/// Build tree from compact packed data.
fn build_compact_tree(
    compact_nodes: &[CompactNodeRaw],
    nvs: &[CompactNvRaw],
    name_data: &[u8],
    value_data: &[u8],
    big_endian: bool,
) -> Option<Node> {
    if compact_nodes.is_empty() {
        return None;
    }

    let mut tree_nodes: Vec<Node> = Vec::with_capacity(compact_nodes.len());
    let mut child_indices: Vec<Vec<usize>> = Vec::with_capacity(compact_nodes.len());

    for (i, pn) in compact_nodes.iter().enumerate() {
        let nv_start = pn.name_value_ofs(big_endian) as usize;
        let mut num_nv = pn.num_name_values as usize;

        // Handle extended count (0xFF means count by scanning for cLastNameValueMask)
        if pn.num_name_values == 0xFF {
            num_nv = 0;
            let scan_start = nv_start + 255;
            if scan_start < nvs.len() {
                let mut idx = scan_start;
                while idx < nvs.len() {
                    num_nv += 1;
                    if nvs[idx].flags(big_endian) & nv_flags::LAST_NAME_VALUE != 0 {
                        break;
                    }
                    idx += 1;
                }
                num_nv += 255;
            } else {
                num_nv = 255;
            }
        }

        // First name-value is the node name + text
        let (name, text) = if num_nv > 0 && nv_start < nvs.len() {
            let nv = &nvs[nv_start];
            let name = read_null_terminated_string(name_data, nv.name_ofs(big_endian) as usize);
            let text = decode_compact_value(*nv, value_data, big_endian);
            (name, text)
        } else {
            (String::new(), Variant::Null)
        };

        // Remaining name-values are attributes
        let mut attributes = Vec::new();
        if num_nv > 1 {
            let attr_start = nv_start + 1;
            let attr_end = (nv_start + num_nv).min(nvs.len());
            for nv in &nvs[attr_start..attr_end] {
                let attr_name =
                    read_null_terminated_string(name_data, nv.name_ofs(big_endian) as usize);
                let attr_value = decode_compact_value(*nv, value_data, big_endian);
                attributes.push(Attribute {
                    name: attr_name,
                    value: attr_value,
                });
            }
        }

        // Compute child indices for this node
        let mut num_children = pn.num_children as usize;
        if pn.num_children == 0xFF {
            num_children = 0;
            let first_child = pn.child_node_index(big_endian) as usize;
            let scan_start = first_child + 255;
            if scan_start < compact_nodes.len() {
                let mut idx = scan_start;
                while idx < compact_nodes.len()
                    && compact_nodes[idx].parent_index(big_endian) as usize == i
                {
                    num_children += 1;
                    idx += 1;
                }
                num_children += 255;
            } else {
                num_children = 255;
            }
        }
        let first_child = pn.child_node_index(big_endian) as usize;
        child_indices.push((0..num_children).map(|ci| first_child + ci).collect());

        tree_nodes.push(Node {
            name,
            text,
            attributes,
            children: Vec::new(),
        });
    }

    // Find root (parent == 0xFFFF)
    let root = compact_nodes
        .iter()
        .enumerate()
        .find(|(_, pn)| pn.parent_index(big_endian) == 0xFFFF)
        .map_or(0, |(i, _)| i);

    assemble_tree(tree_nodes, &child_indices, root)
}

/// Decode a compact `BPackedNameValue` to a Variant.
fn decode_compact_value(nv: CompactNvRaw, value_data: &[u8], big_endian: bool) -> Variant {
    let flags = nv.flags(big_endian);
    let value = nv.value(big_endian);
    let type_class = TypeClass::from_flags(flags);
    let is_direct = (flags & nv_flags::DIRECT_ENCODING) != 0;
    let is_unsigned = (flags & nv_flags::TYPE_IS_UNSIGNED) != 0;
    let type_size_log2 =
        ((flags & nv_flags::TYPE_SIZE_LOG2_MASK) >> nv_flags::TYPE_SIZE_LOG2_SHIFT) as usize;
    let data_size = ((flags & nv_flags::SIZE_MASK) >> nv_flags::SIZE_SHIFT) as usize;

    // Get pointer to value bytes
    let value_bytes: &[u8] = if is_direct {
        &[]
    } else {
        let offset = value as usize;
        if offset < value_data.len() {
            &value_data[offset..]
        } else {
            &[]
        }
    };

    match type_class {
        TypeClass::Null => Variant::Null,
        TypeClass::Bool => {
            if is_direct {
                Variant::Bool(value != 0)
            } else if !value_bytes.is_empty() {
                Variant::Bool(value_bytes[0] != 0)
            } else {
                Variant::Bool(false)
            }
        }
        TypeClass::Int => decode_compact_int(
            is_direct,
            is_unsigned,
            type_size_log2,
            value,
            value_bytes,
            big_endian,
        ),
        TypeClass::Float => decode_compact_float(
            is_direct,
            type_size_log2,
            data_size,
            value,
            value_bytes,
            big_endian,
        ),
        TypeClass::String => decode_compact_string(
            is_direct,
            type_size_log2,
            data_size,
            value,
            value_data,
            big_endian,
        ),
    }
}

/// Decode an integer value from a compact name-value entry.
///
/// Handles direct (inline) and indirect (offset into value data) encodings,
/// with sign extension based on `is_unsigned` and `type_size_log2`.
fn decode_compact_int(
    is_direct: bool,
    is_unsigned: bool,
    type_size_log2: usize,
    value: u32,
    value_bytes: &[u8],
    big_endian: bool,
) -> Variant {
    if is_direct {
        if is_unsigned {
            Variant::UInt(value)
        } else {
            let type_size = 1usize << type_size_log2;
            let v = match type_size {
                1 => i32::from(value.to_le_bytes()[0].cast_signed()),
                2 => i32::from(i16::from_le_bytes(
                    value.to_le_bytes()[..2]
                        .try_into()
                        .expect("two-byte slice has a fixed length"),
                )),
                _ => value.cast_signed(),
            };
            Variant::Int(v)
        }
    } else if value_bytes.len() >= (1 << type_size_log2) {
        let type_size = 1usize << type_size_log2;
        if is_unsigned {
            let v = match type_size {
                1 => u32::from(value_bytes[0]),
                2 => {
                    if big_endian {
                        u32::from(u16::from_be_bytes([value_bytes[0], value_bytes[1]]))
                    } else {
                        u32::from(u16::from_le_bytes([value_bytes[0], value_bytes[1]]))
                    }
                }
                4 => {
                    if big_endian {
                        u32::from_be_bytes([
                            value_bytes[0],
                            value_bytes[1],
                            value_bytes[2],
                            value_bytes[3],
                        ])
                    } else {
                        u32::from_le_bytes([
                            value_bytes[0],
                            value_bytes[1],
                            value_bytes[2],
                            value_bytes[3],
                        ])
                    }
                }
                _ => value,
            };
            Variant::UInt(v)
        } else {
            let v = match type_size {
                1 => i32::from(value_bytes[0].cast_signed()),
                2 => {
                    if big_endian {
                        i32::from(i16::from_be_bytes([value_bytes[0], value_bytes[1]]))
                    } else {
                        i32::from(i16::from_le_bytes([value_bytes[0], value_bytes[1]]))
                    }
                }
                4 => {
                    if big_endian {
                        i32::from_be_bytes([
                            value_bytes[0],
                            value_bytes[1],
                            value_bytes[2],
                            value_bytes[3],
                        ])
                    } else {
                        i32::from_le_bytes([
                            value_bytes[0],
                            value_bytes[1],
                            value_bytes[2],
                            value_bytes[3],
                        ])
                    }
                }
                _ => value.cast_signed(),
            };
            Variant::Int(v)
        }
    } else {
        Variant::Int(0)
    }
}

/// Decode a float or double value from a compact name-value entry.
///
/// Direct values are reinterpreted as `f32` via `from_bits`. Indirect values
/// with a byte count larger than one element are decoded as float vectors;
/// `type_size_log2 == 3` with no array byte count indicates a 64-bit double.
fn decode_compact_float(
    is_direct: bool,
    type_size_log2: usize,
    data_size: usize,
    value: u32,
    value_bytes: &[u8],
    big_endian: bool,
) -> Variant {
    if is_direct {
        Variant::Float(f32::from_bits(value))
    } else if type_size_log2 == 2 && data_size > core::mem::size_of::<f32>() {
        if !data_size.is_multiple_of(core::mem::size_of::<f32>()) {
            return Variant::FloatVec(Vec::new());
        }
        let Some(bytes) = value_bytes.get(..data_size) else {
            return Variant::FloatVec(Vec::new());
        };
        let values = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|component| {
                let bytes = [component[0], component[1], component[2], component[3]];
                if big_endian {
                    f32::from_be_bytes(bytes)
                } else {
                    f32::from_le_bytes(bytes)
                }
            })
            .collect();
        Variant::FloatVec(values)
    } else if type_size_log2 == 3 && (data_size == 0 || data_size == 8) && value_bytes.len() >= 8 {
        // Double
        let v = if big_endian {
            f64::from_be_bytes([
                value_bytes[0],
                value_bytes[1],
                value_bytes[2],
                value_bytes[3],
                value_bytes[4],
                value_bytes[5],
                value_bytes[6],
                value_bytes[7],
            ])
        } else {
            f64::from_le_bytes([
                value_bytes[0],
                value_bytes[1],
                value_bytes[2],
                value_bytes[3],
                value_bytes[4],
                value_bytes[5],
                value_bytes[6],
                value_bytes[7],
            ])
        };
        Variant::Double(v)
    } else if value_bytes.len() >= 4 {
        let v = if big_endian {
            f32::from_be_bytes([
                value_bytes[0],
                value_bytes[1],
                value_bytes[2],
                value_bytes[3],
            ])
        } else {
            f32::from_le_bytes([
                value_bytes[0],
                value_bytes[1],
                value_bytes[2],
                value_bytes[3],
            ])
        };
        Variant::Float(v)
    } else {
        Variant::Float(0.0)
    }
}

/// Decode a string value from a compact name-value entry.
///
/// Direct strings pack up to 4 bytes inline. Indirect strings are read from
/// the value data section as null-terminated UTF-8 or UTF-16 depending on
/// `type_size_log2`.
fn decode_compact_string(
    is_direct: bool,
    type_size_log2: usize,
    data_size: usize,
    value: u32,
    value_data: &[u8],
    big_endian: bool,
) -> Variant {
    if is_direct {
        // Direct string: up to 4 bytes in value
        let bytes = value.to_le_bytes();
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(4);
        Variant::String(String::from_utf8_lossy(&bytes[..end]).into_owned())
    } else {
        // String in value data at offset
        let offset = value as usize;
        // Determine actual size (may be in size field or have sentinel)
        let mut actual_size = data_size;
        if actual_size == 127 && offset >= 4 {
            // Extended size: stored as u32 at offset - 4
            actual_size = if big_endian {
                u32::from_be_bytes([
                    value_data[offset - 4],
                    value_data[offset - 3],
                    value_data[offset - 2],
                    value_data[offset - 1],
                ]) as usize
            } else {
                u32::from_le_bytes([
                    value_data[offset - 4],
                    value_data[offset - 3],
                    value_data[offset - 2],
                    value_data[offset - 1],
                ]) as usize
            };
        }

        if type_size_log2 > 0 {
            // Wide string (UTF-16)
            let byte_count = actual_size;
            if offset + byte_count <= value_data.len() {
                let char_count = byte_count / 2;
                let mut chars = Vec::with_capacity(char_count);
                for i in 0..char_count {
                    let c = if big_endian {
                        u16::from_be_bytes([
                            value_data[offset + i * 2],
                            value_data[offset + i * 2 + 1],
                        ])
                    } else {
                        u16::from_le_bytes([
                            value_data[offset + i * 2],
                            value_data[offset + i * 2 + 1],
                        ])
                    };
                    if c == 0 {
                        break;
                    }
                    chars.push(c);
                }
                Variant::String(String::from_utf16_lossy(&chars))
            } else {
                Variant::String(String::new())
            }
        } else {
            // Narrow string (ASCII/UTF-8)
            if offset < value_data.len() {
                let end = value_data[offset..]
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(actual_size.min(value_data.len() - offset));
                Variant::String(
                    String::from_utf8_lossy(&value_data[offset..offset + end]).into_owned(),
                )
            } else {
                Variant::String(String::new())
            }
        }
    }
}
