//! BBinaryDataTree packed document reader.
//!
//! Reads the packed binary tree format used by Ensemble Studios games.
//! Supports both little-endian (PC/DE) and big-endian (Xbox 360) formats.

use byteorder::{BigEndian, LittleEndian, ReadBytesExt};
use std::io::Cursor;

use crate::error::{Error, Result};
use crate::types::{Attribute, Node};
use crate::variant::{
    unpack_float24, unpack_fract24, unpack_int24, Variant,
    UNSIGNED_FLAG,
};

/// Packed document reader for BBinaryDataTree format.
pub struct PackedReader;

impl PackedReader {
    /// Parse little-endian packed data (PC/Definitive Edition format).
    ///
    /// The data should start at the header (after any container-specific signature).
    /// Expected layout: pad(4) + nodes_bpa(16) + variant_bpa(16) = 36 bytes header,
    /// followed by node data, attributes, children, and variant data.
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
        let mut packed_nodes = Vec::with_capacity(nodes_size as usize);

        for i in 0..nodes_size as usize {
            let node_offset = nodes_ptr + i * NODE_SIZE;
            if node_offset + NODE_SIZE > data.len() {
                return Err(Error::UnexpectedEof);
            }

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
        build_tree_le(&packed_nodes, variant_data)
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
        let mut packed_nodes = Vec::with_capacity(nodes_size as usize);

        for i in 0..nodes_size as usize {
            let node_offset = nodes_ptr + i * NODE_SIZE;
            if node_offset + NODE_SIZE > data.len() {
                return Err(Error::UnexpectedEof);
            }

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
        build_tree_be(&packed_nodes, variant_data)
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

/// Build tree from packed nodes (little-endian variant data).
fn build_tree_le(packed_nodes: &[PackedNodeRead], variant_data: &[u8]) -> Result<Option<Node>> {
    if packed_nodes.is_empty() {
        return Ok(None);
    }

    let mut nodes: Vec<Node> = Vec::with_capacity(packed_nodes.len());

    for pn in packed_nodes {
        let name = decode_variant_string(pn.name_variant, variant_data)?;
        let text = decode_variant_to_variant_le(pn.text_variant, variant_data)?;

        let mut attributes = Vec::with_capacity(pn.attributes.len());
        for (name_var, value_var) in &pn.attributes {
            let attr_name = decode_variant_string(*name_var, variant_data)?;
            let attr_value = decode_variant_to_variant_le(*value_var, variant_data)?;
            attributes.push(Attribute {
                name: attr_name,
                value: attr_value,
            });
        }

        nodes.push(Node {
            name,
            text,
            attributes,
            children: Vec::new(),
        });
    }

    // Build parent-child relationships in REVERSE order so that leaf nodes
    // are fully built before their parents clone them
    for (i, pn) in packed_nodes.iter().enumerate().rev() {
        let child_indices: Vec<usize> = pn.children.iter().map(|&c| c as usize).collect();
        for &child_idx in &child_indices {
            if child_idx < nodes.len() {
                let child = nodes[child_idx].clone();
                nodes[i].children.push(child);
            }
        }
    }

    // Find root (node with parent 0xFFFFFFFF or parent pointing to itself)
    for (i, pn) in packed_nodes.iter().enumerate() {
        if pn.parent_node == 0xFFFFFFFF || pn.parent_node as usize == i {
            return Ok(Some(nodes[i].clone()));
        }
    }

    // Default to first node
    Ok(Some(nodes[0].clone()))
}

/// Build tree from packed nodes (big-endian variant data).
fn build_tree_be(packed_nodes: &[PackedNodeRead], variant_data: &[u8]) -> Result<Option<Node>> {
    if packed_nodes.is_empty() {
        return Ok(None);
    }

    let mut nodes: Vec<Node> = Vec::with_capacity(packed_nodes.len());

    for pn in packed_nodes {
        let name = decode_variant_string(pn.name_variant, variant_data)?;
        let text = decode_variant_to_variant_be(pn.text_variant, variant_data)?;

        let mut attributes = Vec::with_capacity(pn.attributes.len());
        for (name_var, value_var) in &pn.attributes {
            let attr_name = decode_variant_string(*name_var, variant_data)?;
            let attr_value = decode_variant_to_variant_be(*value_var, variant_data)?;
            attributes.push(Attribute {
                name: attr_name,
                value: attr_value,
            });
        }

        nodes.push(Node {
            name,
            text,
            attributes,
            children: Vec::new(),
        });
    }

    for (i, pn) in packed_nodes.iter().enumerate().rev() {
        let child_indices: Vec<usize> = pn.children.iter().map(|&c| c as usize).collect();
        for &child_idx in &child_indices {
            if child_idx < nodes.len() {
                let child = nodes[child_idx].clone();
                nodes[i].children.push(child);
            }
        }
    }

    for (i, pn) in packed_nodes.iter().enumerate() {
        if pn.parent_node == 0xFFFFFFFF || pn.parent_node as usize == i {
            return Ok(Some(nodes[i].clone()));
        }
    }

    Ok(Some(nodes[0].clone()))
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

fn decode_variant_to_variant_le(variant_value: u32, variant_data: &[u8]) -> Result<Variant> {
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
                Ok(Variant::Float(f32::from_le_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3],
                ])))
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
                Ok(Variant::Int(i32::from_le_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3],
                ])))
            } else {
                Ok(Variant::Int(0))
            }
        }
        5 => Ok(Variant::Float(unpack_fract24(data_bits) as f32)),
        6 => {
            if is_offset && data_bits as usize + 8 <= variant_data.len() {
                let bytes = &variant_data[data_bits as usize..data_bits as usize + 8];
                Ok(Variant::Double(f64::from_le_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                ])))
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
                Ok(Variant::String(read_null_terminated_wstring_le(
                    variant_data,
                    data_bits as usize,
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
                    vec.push(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]));
                }
                Ok(Variant::FloatVec(vec))
            } else {
                Ok(Variant::FloatVec(vec![0.0; vec_size as usize]))
            }
        }
        _ => Ok(Variant::Null),
    }
}

fn decode_variant_to_variant_be(variant_value: u32, variant_data: &[u8]) -> Result<Variant> {
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
                Ok(Variant::Float(f32::from_be_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3],
                ])))
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
                Ok(Variant::Int(i32::from_be_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3],
                ])))
            } else {
                Ok(Variant::Int(0))
            }
        }
        5 => Ok(Variant::Float(unpack_fract24(data_bits) as f32)),
        6 => {
            if is_offset && data_bits as usize + 8 <= variant_data.len() {
                let bytes = &variant_data[data_bits as usize..data_bits as usize + 8];
                Ok(Variant::Double(f64::from_be_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                ])))
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
                Ok(Variant::String(read_null_terminated_wstring_be(
                    variant_data,
                    data_bits as usize,
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
                    vec.push(f32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]));
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

fn read_null_terminated_wstring_le(data: &[u8], offset: usize) -> Result<String> {
    if offset >= data.len() {
        return Ok(String::new());
    }
    let mut chars = Vec::new();
    let mut i = offset;
    while i + 1 < data.len() {
        let c = u16::from_le_bytes([data[i], data[i + 1]]);
        if c == 0 {
            break;
        }
        chars.push(c);
        i += 2;
    }
    Ok(String::from_utf16_lossy(&chars))
}

fn read_null_terminated_wstring_be(data: &[u8], offset: usize) -> Result<String> {
    if offset >= data.len() {
        return Ok(String::new());
    }
    let mut chars = Vec::new();
    let mut i = offset;
    while i + 1 < data.len() {
        let c = u16::from_be_bytes([data[i], data[i + 1]]);
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
