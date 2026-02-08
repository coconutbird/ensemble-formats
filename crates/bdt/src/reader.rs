//! BBinaryDataTree packed document reader.
//!
//! Reads the packed binary tree format used by Ensemble Studios games.
//! Supports both little-endian (PC/DE) and big-endian (Xbox 360) formats.
//!
//! Two serialization formats are supported:
//!
//! 1. **Compact format** (BPackedHeader): Used by BBinaryDataTree directly (material chunks,
//!    etc.). Identified by signature byte 0x3E (LE) or 0xE3 (BE). Uses 8-byte BPackedNode
//!    and 8-byte BPackedNameValue structs with section-based layout.
//!
//! 2. **XMX variant format**: Used by XMB files after the 4-byte XMB signature. Uses 48-byte
//!    nodes with 64-bit BPackedArray pointers and the XMX variant type encoding.

use byteorder::{BigEndian, LittleEndian, ReadBytesExt};
use std::io::Cursor;

use crate::error::{Error, Result};
use crate::types::{Attribute, Node};
use crate::variant::{
    unpack_float24, unpack_fract24, unpack_int24, Variant, OFFSET_FLAG, UNSIGNED_FLAG,
};

/// BPackedHeader signature for little-endian data.
const PACKED_HEADER_SIG_LE: u8 = 0x3E;
/// BPackedHeader signature for big-endian data.
const PACKED_HEADER_SIG_BE: u8 = 0xE3;

/// Packed document reader for BBinaryDataTree format.
pub struct PackedReader;

impl PackedReader {
    /// Parse little-endian packed data (PC/Definitive Edition format).
    ///
    /// Auto-detects the format:
    /// - If data starts with 0x3E: compact BPackedHeader format
    /// - Otherwise: XMX variant format (pad + BPackedArrays)
    pub fn read_le(data: &[u8]) -> Result<Option<Node>> {
        Self::read_le_at(data, 0)
    }

    /// Parse little-endian packed data with a header offset.
    ///
    /// The header starts at `header_offset` within `data`. All internal pointers
    /// (node pointers, attribute pointers, etc.) are absolute offsets from `data[0]`.
    ///
    /// This is useful when the packed data is preceded by a container-specific
    /// prefix (e.g., XMB's 4-byte signature).
    pub fn read_le_at(data: &[u8], header_offset: usize) -> Result<Option<Node>> {
        // Auto-detect format based on signature byte
        if header_offset < data.len() && data[header_offset] == PACKED_HEADER_SIG_LE {
            return read_compact(data, header_offset, false);
        }

        if data.len() < header_offset + 36 {
            return Ok(None);
        }

        let mut cursor = Cursor::new(data);
        cursor.set_position(header_offset as u64);

        // PC header: pad(4) + nodes BPackedArray(16) + variant BPackedArray(16)
        let _padding = cursor.read_u32::<LittleEndian>()?;

        // Nodes BPackedArray
        let nodes_size = cursor.read_u32::<LittleEndian>()?;
        let _nodes_pad = cursor.read_u32::<LittleEndian>()?;
        let nodes_ptr = cursor.read_u64::<LittleEndian>()? as usize;

        // Variant data BPackedArray
        let variant_data_size = cursor.read_u32::<LittleEndian>()?;
        let _variant_pad = cursor.read_u32::<LittleEndian>()?;
        let variant_data_ptr = cursor.read_u64::<LittleEndian>()? as usize;

        if nodes_size == 0 || nodes_size == 0xFFFFFFFF {
            return Ok(None);
        }

        // Read variant data
        let variant_data = if variant_data_size > 0 && variant_data_ptr < data.len() {
            let end = (variant_data_ptr + variant_data_size as usize).min(data.len());
            &data[variant_data_ptr..end]
        } else {
            &[]
        };

        // Read nodes (48 bytes each)
        const NODE_SIZE: usize = 48;

        // Bounds check before allocating
        let expected_end = nodes_ptr
            .checked_add(
                (nodes_size as usize)
                    .checked_mul(NODE_SIZE)
                    .ok_or(Error::UnexpectedEof)?,
            )
            .ok_or(Error::UnexpectedEof)?;
        if expected_end > data.len() {
            return Err(Error::UnexpectedEof);
        }

        let mut packed_nodes = Vec::with_capacity(nodes_size as usize);

        for i in 0..nodes_size as usize {
            let node_offset = nodes_ptr + i * NODE_SIZE;

            cursor.set_position(node_offset as u64);

            let parent_node = cursor.read_u32::<LittleEndian>()?;
            let name_variant = cursor.read_u32::<LittleEndian>()?;
            let text_variant = cursor.read_u32::<LittleEndian>()?;
            let _padding = cursor.read_u32::<LittleEndian>()?;

            // Attributes BPackedArray
            let attrs_size = cursor.read_u32::<LittleEndian>()?;
            let _attrs_pad = cursor.read_u32::<LittleEndian>()?;
            let attrs_ptr = cursor.read_u64::<LittleEndian>()? as usize;

            // Children BPackedArray
            let children_size = cursor.read_u32::<LittleEndian>()?;
            let _children_pad = cursor.read_u32::<LittleEndian>()?;
            let children_ptr = cursor.read_u64::<LittleEndian>()? as usize;

            // Read attributes
            let mut attributes = Vec::new();
            if attrs_size != 0xFFFFFFFF && attrs_size > 0 && attrs_ptr < data.len() {
                for j in 0..attrs_size as usize {
                    let attr_offset = attrs_ptr + j * 8;
                    if attr_offset + 8 > data.len() {
                        break;
                    }
                    cursor.set_position(attr_offset as u64);
                    let name_var = cursor.read_u32::<LittleEndian>()?;
                    let value_var = cursor.read_u32::<LittleEndian>()?;
                    attributes.push((name_var, value_var));
                }
            }

            // Read children indices
            let mut children = Vec::new();
            if children_size != 0xFFFFFFFF && children_size > 0 && children_ptr < data.len() {
                for j in 0..children_size as usize {
                    let child_offset = children_ptr + j * 4;
                    if child_offset + 4 > data.len() {
                        break;
                    }
                    cursor.set_position(child_offset as u64);
                    let child_idx = cursor.read_u32::<LittleEndian>()?;
                    children.push(child_idx);
                }
            }

            packed_nodes.push(PackedNodeRead {
                parent_node,
                name_variant,
                text_variant,
                attributes,
                children,
            });
        }

        // Build tree
        build_tree(&packed_nodes, variant_data, false)
    }

    /// Parse big-endian packed data (Xbox 360 format).
    ///
    /// Expected layout: nodes_size(4) + nodes_ptr(4) + variant_size(4) + variant_ptr(4) = 16 bytes header.
    pub fn read_be(data: &[u8]) -> Result<Option<Node>> {
        Self::read_be_at(data, 0)
    }

    /// Parse big-endian packed data with a header offset.
    ///
    /// The header starts at `header_offset` within `data`. All internal pointers
    /// are absolute offsets from `data[0]`.
    pub fn read_be_at(data: &[u8], header_offset: usize) -> Result<Option<Node>> {
        // Auto-detect compact format
        if header_offset < data.len() && data[header_offset] == PACKED_HEADER_SIG_BE {
            return read_compact(data, header_offset, true);
        }

        if data.len() < header_offset + 16 {
            return Ok(None);
        }

        let mut cursor = Cursor::new(data);
        cursor.set_position(header_offset as u64);

        // Xbox 360 header: nodes_size(4) + nodes_ptr(4) + variant_size(4) + variant_ptr(4)
        let nodes_size = cursor.read_u32::<BigEndian>()?;
        let nodes_ptr = cursor.read_u32::<BigEndian>()? as usize;
        let variant_data_size = cursor.read_u32::<BigEndian>()?;
        let variant_data_ptr = cursor.read_u32::<BigEndian>()? as usize;

        if nodes_size == 0 {
            return Ok(None);
        }

        // Read variant data
        let variant_data = if variant_data_size > 0 && variant_data_ptr < data.len() {
            let end = (variant_data_ptr + variant_data_size as usize).min(data.len());
            &data[variant_data_ptr..end]
        } else {
            &[]
        };

        // Read nodes (28 bytes each)
        const NODE_SIZE: usize = 28;

        // Bounds check before allocating
        let expected_end = nodes_ptr
            .checked_add(
                (nodes_size as usize)
                    .checked_mul(NODE_SIZE)
                    .ok_or(Error::UnexpectedEof)?,
            )
            .ok_or(Error::UnexpectedEof)?;
        if expected_end > data.len() {
            return Err(Error::UnexpectedEof);
        }

        let mut packed_nodes = Vec::with_capacity(nodes_size as usize);

        for i in 0..nodes_size as usize {
            let node_offset = nodes_ptr + i * NODE_SIZE;

            cursor.set_position(node_offset as u64);

            let parent_node = cursor.read_u32::<BigEndian>()?;
            let name_variant = cursor.read_u32::<BigEndian>()?;
            let text_variant = cursor.read_u32::<BigEndian>()?;
            let attrs_size = cursor.read_u32::<BigEndian>()?;
            let attrs_ptr = cursor.read_u32::<BigEndian>()? as usize;
            let children_size = cursor.read_u32::<BigEndian>()?;
            let children_ptr = cursor.read_u32::<BigEndian>()? as usize;

            // Read attributes
            let mut attributes = Vec::new();
            if attrs_ptr != 0xFFFFFFFF && attrs_size > 0 && attrs_ptr < data.len() {
                for j in 0..attrs_size as usize {
                    let attr_offset = attrs_ptr + j * 8;
                    if attr_offset + 8 > data.len() {
                        break;
                    }
                    cursor.set_position(attr_offset as u64);
                    let name_var = cursor.read_u32::<BigEndian>()?;
                    let value_var = cursor.read_u32::<BigEndian>()?;
                    attributes.push((name_var, value_var));
                }
            }

            // Read children indices
            let mut children = Vec::new();
            if children_ptr != 0xFFFFFFFF && children_size > 0 && children_ptr < data.len() {
                for j in 0..children_size as usize {
                    let child_offset = children_ptr + j * 4;
                    if child_offset + 4 > data.len() {
                        break;
                    }
                    cursor.set_position(child_offset as u64);
                    let child_idx = cursor.read_u32::<BigEndian>()?;
                    children.push(child_idx);
                }
            }

            packed_nodes.push(PackedNodeRead {
                parent_node,
                name_variant,
                text_variant,
                attributes,
                children,
            });
        }

        // Build tree
        build_tree(&packed_nodes, variant_data, true)
    }
}

// ============================================================================
// Internal types and helpers
// ============================================================================

/// Packed node structure for reading.
struct PackedNodeRead {
    #[allow(dead_code)]
    parent_node: u32,
    name_variant: u32,
    text_variant: u32,
    attributes: Vec<(u32, u32)>,
    children: Vec<u32>,
}

/// Build tree from packed nodes (XMX variant format).
fn build_tree(
    packed_nodes: &[PackedNodeRead],
    variant_data: &[u8],
    big_endian: bool,
) -> Result<Option<Node>> {
    if packed_nodes.is_empty() {
        return Ok(None);
    }

    let mut nodes: Vec<Node> = Vec::with_capacity(packed_nodes.len());
    let mut child_indices: Vec<Vec<usize>> = Vec::with_capacity(packed_nodes.len());

    for pn in packed_nodes {
        let name = decode_variant_string(pn.name_variant, variant_data)?;
        let text = decode_variant_to_variant(pn.text_variant, variant_data, big_endian)?;

        let mut attributes = Vec::with_capacity(pn.attributes.len());
        for (name_var, value_var) in &pn.attributes {
            let attr_name = decode_variant_string(*name_var, variant_data)?;
            let attr_value = decode_variant_to_variant(*value_var, variant_data, big_endian)?;
            attributes.push(Attribute {
                name: attr_name,
                value: attr_value,
            });
        }

        child_indices.push(pn.children.iter().map(|&c| c as usize).collect());
        nodes.push(Node {
            name,
            text,
            attributes,
            children: Vec::new(),
        });
    }

    // Find root (node with parent 0xFFFFFFFF or parent pointing to itself)
    let root = packed_nodes
        .iter()
        .enumerate()
        .find(|(i, pn)| pn.parent_node == 0xFFFFFFFF || pn.parent_node as usize == *i)
        .map(|(i, _)| i)
        .unwrap_or(0);

    Ok(assemble_tree(nodes, &child_indices, root))
}

/// Assemble a tree from flat nodes by cloning children into parents.
///
/// `child_indices[i]` lists the child node indices for node `i`.
/// Children are attached in reverse order so leaf nodes are fully built
/// before their parents clone them.
fn assemble_tree(mut nodes: Vec<Node>, child_indices: &[Vec<usize>], root: usize) -> Option<Node> {
    if nodes.is_empty() {
        return None;
    }

    for i in (0..nodes.len()).rev() {
        for &child_idx in &child_indices[i] {
            if child_idx < nodes.len() {
                let child = nodes[child_idx].clone();
                nodes[i].children.push(child);
            }
        }
    }

    Some(nodes[root].clone())
}

// ============================================================================
// Variant decoding
// ============================================================================

fn decode_variant_string(variant_value: u32, variant_data: &[u8]) -> Result<String> {
    let type_bits = (variant_value >> 24) as u8;
    let data_bits = variant_value & 0xFFFFFF;
    let variant_type = type_bits & 0x0F;
    let is_offset = (type_bits & 0x80) != 0;

    match variant_type {
        0 => Ok(String::new()),
        8 => {
            if is_offset {
                let offset = data_bits as usize;
                read_null_terminated_string(variant_data, offset)
            } else {
                decode_direct_string(data_bits)
            }
        }
        _ => Ok(format!("<type:{}>", variant_type)),
    }
}

fn decode_variant_to_variant(
    variant_value: u32,
    variant_data: &[u8],
    big_endian: bool,
) -> Result<Variant> {
    let type_bits = (variant_value >> 24) as u8;
    let data_bits = variant_value & 0xFFFFFF;
    let variant_type = type_bits & 0x0F;
    let is_offset = (type_bits & OFFSET_FLAG) != 0;
    let is_unsigned = (type_bits & UNSIGNED_FLAG) != 0;

    match variant_type {
        0 => Ok(Variant::Null),
        1 => Ok(Variant::Float(unpack_float24(data_bits))),
        2 => {
            if is_offset && data_bits as usize + 4 <= variant_data.len() {
                let bytes = &variant_data[data_bits as usize..data_bits as usize + 4];
                let v = if big_endian {
                    f32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
                } else {
                    f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
                };
                Ok(Variant::Float(v))
            } else {
                Ok(Variant::Float(0.0))
            }
        }
        3 => {
            if is_unsigned {
                Ok(Variant::UInt(data_bits))
            } else {
                Ok(Variant::Int(unpack_int24(data_bits)))
            }
        }
        4 => {
            if is_offset && data_bits as usize + 4 <= variant_data.len() {
                let bytes = &variant_data[data_bits as usize..data_bits as usize + 4];
                let v = if big_endian {
                    i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
                } else {
                    i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
                };
                Ok(Variant::Int(v))
            } else {
                Ok(Variant::Int(0))
            }
        }
        5 => Ok(Variant::Float(unpack_fract24(data_bits) as f32)),
        6 => {
            if is_offset && data_bits as usize + 8 <= variant_data.len() {
                let bytes = &variant_data[data_bits as usize..data_bits as usize + 8];
                let v = if big_endian {
                    f64::from_be_bytes([
                        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6],
                        bytes[7],
                    ])
                } else {
                    f64::from_le_bytes([
                        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6],
                        bytes[7],
                    ])
                };
                Ok(Variant::Double(v))
            } else {
                Ok(Variant::Double(0.0))
            }
        }
        7 => Ok(Variant::Bool(data_bits != 0)),
        8 => {
            if is_offset {
                Ok(Variant::String(read_null_terminated_string(
                    variant_data,
                    data_bits as usize,
                )?))
            } else {
                Ok(Variant::String(decode_direct_string(data_bits)?))
            }
        }
        9 => {
            if is_offset {
                Ok(Variant::String(read_null_terminated_wstring(
                    variant_data,
                    data_bits as usize,
                    big_endian,
                )?))
            } else {
                Ok(Variant::String(String::new()))
            }
        }
        10 => {
            let vec_size = 1 + ((type_bits >> 4) & 0x03);
            if is_offset && data_bits as usize + (vec_size as usize * 4) <= variant_data.len() {
                let offset = data_bits as usize;
                let mut vec = Vec::with_capacity(vec_size as usize);
                for i in 0..vec_size as usize {
                    let bytes = &variant_data[offset + i * 4..offset + i * 4 + 4];
                    let v = if big_endian {
                        f32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
                    } else {
                        f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
                    };
                    vec.push(v);
                }
                Ok(Variant::FloatVec(vec))
            } else {
                Ok(Variant::FloatVec(vec![0.0; vec_size as usize]))
            }
        }
        _ => Ok(Variant::Null),
    }
}

// ============================================================================
// String helpers
// ============================================================================

fn read_null_terminated_string(data: &[u8], offset: usize) -> Result<String> {
    if offset >= data.len() {
        return Ok(String::new());
    }
    let end = data[offset..]
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(data.len() - offset);
    Ok(String::from_utf8_lossy(&data[offset..offset + end]).into_owned())
}

fn read_null_terminated_wstring(data: &[u8], offset: usize, big_endian: bool) -> Result<String> {
    if offset >= data.len() {
        return Ok(String::new());
    }
    let mut chars = Vec::new();
    let mut i = offset;
    while i + 1 < data.len() {
        let c = if big_endian {
            u16::from_be_bytes([data[i], data[i + 1]])
        } else {
            u16::from_le_bytes([data[i], data[i + 1]])
        };
        if c == 0 {
            break;
        }
        chars.push(c);
        i += 2;
    }
    Ok(String::from_utf16_lossy(&chars))
}

fn decode_direct_string(data_bits: u32) -> Result<String> {
    let mut bytes = Vec::new();
    let b0 = (data_bits & 0xFF) as u8;
    let b1 = ((data_bits >> 8) & 0xFF) as u8;
    let b2 = ((data_bits >> 16) & 0xFF) as u8;
    if b0 != 0 {
        bytes.push(b0);
    }
    if b1 != 0 {
        bytes.push(b1);
    }
    if b2 != 0 {
        bytes.push(b2);
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

// ============================================================================
// Compact BPackedHeader format reader
// ============================================================================
//
// This is the native BBinaryDataTree serialization format, used by material
// chunks and other non-XMB packed data. The format uses:
// - BPackedHeader (28 bytes): signature, CRC, section sizes
// - BPackedNode (8 bytes): compact node with 16-bit indices
// - BPackedNameValue (8 bytes): name/value pair with type flags
//
// Section layout after header:
// [User sections (12 bytes each)]
// [Node section]
// [NameValue section]
// [NameData section (null-terminated strings)]
// [Padding to 16-byte boundary]
// [ValueData section (16-byte aligned)]

/// BPackedNameValue flag constants (from binaryDataTree.h).
mod nv_flags {
    pub const TYPE_IS_UNSIGNED: u16 = 0x0001;
    pub const DIRECT_ENCODING: u16 = 0x0002;
    pub const TYPE_SHIFT: u16 = 2;
    pub const TYPE_MASK: u16 = 0x001C; // 3 bits
    pub const TYPE_SIZE_LOG2_SHIFT: u16 = 5;
    pub const TYPE_SIZE_LOG2_MASK: u16 = 0x00E0; // 3 bits
    #[allow(dead_code)]
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
            0 => TypeClass::Null,
            1 => TypeClass::Bool,
            2 => TypeClass::Int,
            3 => TypeClass::Float,
            4 => TypeClass::String,
            _ => TypeClass::Null,
        }
    }
}

/// Compact packed node (8 bytes).
struct CompactNode {
    parent_index: u16,
    child_node_index: u16,
    name_value_ofs: u16,
    num_name_values: u8,
    num_children: u8,
}

/// Compact packed name-value (8 bytes).
struct CompactNameValue {
    value: u32,
    name_ofs: u16,
    flags: u16,
}

/// Read compact format (BPackedHeader with 0x3E/0xE3 signature).
fn read_compact(data: &[u8], header_offset: usize, big_endian: bool) -> Result<Option<Node>> {
    const HEADER_SIZE: usize = 28;

    if data.len() < header_offset + HEADER_SIZE {
        return Err(Error::UnexpectedEof);
    }

    let base = header_offset;

    // Parse BPackedHeader - first 4 bytes are single-byte fields (endian-independent)
    let sig = data[base];
    let expected_sig = if big_endian {
        PACKED_HEADER_SIG_BE
    } else {
        PACKED_HEADER_SIG_LE
    };
    if sig != expected_sig {
        return Err(Error::UnexpectedEof);
    }
    let num_user_sections = data[base + 3] as usize;

    // Remaining header fields are u32 (endian-dependent)
    let read_u32 = |off: usize| -> u32 {
        let b = &data[off..off + 4];
        if big_endian {
            u32::from_be_bytes([b[0], b[1], b[2], b[3]])
        } else {
            u32::from_le_bytes([b[0], b[1], b[2], b[3]])
        }
    };
    let read_u16 = |off: usize| -> u16 {
        let b = &data[off..off + 2];
        if big_endian {
            u16::from_be_bytes([b[0], b[1]])
        } else {
            u16::from_le_bytes([b[0], b[1]])
        }
    };

    let node_section_size = read_u32(base + 12) as usize;
    let nv_section_size = read_u32(base + 16) as usize;
    let name_data_size = read_u32(base + 20) as usize;
    let value_data_size = read_u32(base + 24) as usize;

    // Calculate section offsets
    let user_sections_offset = base + HEADER_SIZE;
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

    // Read packed nodes
    let mut packed_nodes = Vec::with_capacity(node_count);
    for i in 0..node_count {
        let off = node_offset + i * 8;
        packed_nodes.push(CompactNode {
            parent_index: read_u16(off),
            child_node_index: read_u16(off + 2),
            name_value_ofs: read_u16(off + 4),
            num_name_values: data[off + 6],
            num_children: data[off + 7],
        });
    }

    // Read packed name-values
    let mut packed_nvs = Vec::with_capacity(nv_count);
    for i in 0..nv_count {
        let off = nv_offset + i * 8;
        packed_nvs.push(CompactNameValue {
            value: read_u32(off),
            name_ofs: read_u16(off + 4),
            flags: read_u16(off + 6),
        });
    }

    // Section slices
    let name_data = &data[name_data_offset..name_data_offset + name_data_size];
    let value_data = &data[value_data_offset..value_data_offset + value_data_size];

    // Build tree
    build_compact_tree(
        &packed_nodes,
        &packed_nvs,
        name_data,
        value_data,
        big_endian,
    )
}

/// Build tree from compact packed data.
fn build_compact_tree(
    compact_nodes: &[CompactNode],
    nvs: &[CompactNameValue],
    name_data: &[u8],
    value_data: &[u8],
    big_endian: bool,
) -> Result<Option<Node>> {
    if compact_nodes.is_empty() {
        return Ok(None);
    }

    let mut tree_nodes: Vec<Node> = Vec::with_capacity(compact_nodes.len());
    let mut child_indices: Vec<Vec<usize>> = Vec::with_capacity(compact_nodes.len());

    for (i, pn) in compact_nodes.iter().enumerate() {
        let nv_start = pn.name_value_ofs as usize;
        let mut num_nv = pn.num_name_values as usize;

        // Handle extended count (0xFF means count by scanning for cLastNameValueMask)
        if pn.num_name_values == 0xFF {
            num_nv = 0;
            let scan_start = nv_start + 255;
            if scan_start < nvs.len() {
                let mut idx = scan_start;
                while idx < nvs.len() {
                    num_nv += 1;
                    if nvs[idx].flags & nv_flags::LAST_NAME_VALUE != 0 {
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
            let name =
                read_null_terminated_string(name_data, nv.name_ofs as usize).unwrap_or_default();
            let text = decode_compact_value(nv, value_data, big_endian);
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
                let attr_name = read_null_terminated_string(name_data, nv.name_ofs as usize)
                    .unwrap_or_default();
                let attr_value = decode_compact_value(nv, value_data, big_endian);
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
            let first_child = pn.child_node_index as usize;
            let scan_start = first_child + 255;
            if scan_start < compact_nodes.len() {
                let mut idx = scan_start;
                while idx < compact_nodes.len() && compact_nodes[idx].parent_index as usize == i {
                    num_children += 1;
                    idx += 1;
                }
                num_children += 255;
            } else {
                num_children = 255;
            }
        }
        let first_child = pn.child_node_index as usize;
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
        .find(|(_, pn)| pn.parent_index == 0xFFFF)
        .map(|(i, _)| i)
        .unwrap_or(0);

    Ok(assemble_tree(tree_nodes, &child_indices, root))
}

/// Decode a compact BPackedNameValue to a Variant.
fn decode_compact_value(nv: &CompactNameValue, value_data: &[u8], big_endian: bool) -> Variant {
    let flags = nv.flags;
    let type_class = TypeClass::from_flags(flags);
    let is_direct = (flags & nv_flags::DIRECT_ENCODING) != 0;
    let is_unsigned = (flags & nv_flags::TYPE_IS_UNSIGNED) != 0;
    let type_size_log2 =
        ((flags & nv_flags::TYPE_SIZE_LOG2_MASK) >> nv_flags::TYPE_SIZE_LOG2_SHIFT) as usize;
    let data_size = ((flags & nv_flags::SIZE_MASK) >> nv_flags::SIZE_SHIFT) as usize;

    // Get pointer to value bytes
    let value_bytes: &[u8] = if is_direct {
        // Value is stored directly in the mValue field (up to 4 bytes)
        // We need to create a temporary slice from nv.value
        // Note: this is a bit awkward since we need a reference to the bytes
        &[]
    } else {
        // Value is at offset nv.value in value_data
        let offset = nv.value as usize;
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
                Variant::Bool(nv.value != 0)
            } else if !value_bytes.is_empty() {
                Variant::Bool(value_bytes[0] != 0)
            } else {
                Variant::Bool(false)
            }
        }
        TypeClass::Int => {
            if is_direct {
                // Direct int: stored in nv.value (up to 4 bytes)
                if is_unsigned {
                    Variant::UInt(nv.value)
                } else {
                    // Sign extend based on type size
                    let type_size = 1usize << type_size_log2;
                    let v = match type_size {
                        1 => (nv.value as u8) as i8 as i32,
                        2 => (nv.value as u16) as i16 as i32,
                        _ => nv.value as i32,
                    };
                    Variant::Int(v)
                }
            } else if value_bytes.len() >= (1 << type_size_log2) {
                let type_size = 1usize << type_size_log2;
                if is_unsigned {
                    let v = match type_size {
                        1 => value_bytes[0] as u32,
                        2 => {
                            if big_endian {
                                u16::from_be_bytes([value_bytes[0], value_bytes[1]]) as u32
                            } else {
                                u16::from_le_bytes([value_bytes[0], value_bytes[1]]) as u32
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
                        _ => nv.value,
                    };
                    Variant::UInt(v)
                } else {
                    let v = match type_size {
                        1 => value_bytes[0] as i8 as i32,
                        2 => {
                            if big_endian {
                                i16::from_be_bytes([value_bytes[0], value_bytes[1]]) as i32
                            } else {
                                i16::from_le_bytes([value_bytes[0], value_bytes[1]]) as i32
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
                        _ => nv.value as i32,
                    };
                    Variant::Int(v)
                }
            } else {
                Variant::Int(0)
            }
        }
        TypeClass::Float => {
            if is_direct {
                Variant::Float(f32::from_bits(nv.value))
            } else if type_size_log2 == 3 && value_bytes.len() >= 8 {
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
        TypeClass::String => {
            if is_direct {
                // Direct string: up to 4 bytes in nv.value
                let bytes = nv.value.to_le_bytes();
                let end = bytes.iter().position(|&b| b == 0).unwrap_or(4);
                Variant::String(String::from_utf8_lossy(&bytes[..end]).into_owned())
            } else {
                // String in value data at offset nv.value
                let offset = nv.value as usize;
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
    }
}
