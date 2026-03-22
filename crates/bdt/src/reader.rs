//! BPackedReader — reads BBinaryDataTree packed documents.
//!
//! This is the Ensemble Studios `BPackedReader` implementation. It parses the
//! XMX variant format (used inside XMB files and other ECF containers) from
//! raw byte slices, supporting both little-endian (PC/DE) and big-endian
//! (Xbox 360) layouts.
//!
//! ## Usage
//!
//! ```ignore
//! use bdt::Reader;
//!
//! let node = Reader::read_le(&data)?;
//! ```

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use zerocopy::Ref;

use crate::compact;
use crate::error::{Error, Result};
use crate::node::{Attribute, Node};
use crate::raw::{AttrPairRaw, XmxHeaderBe, XmxHeaderLe, XmxNodeBe, XmxNodeLe};
use crate::util::{
    assemble_tree, decode_direct_string, read_null_terminated_string, read_null_terminated_wstring,
};
use crate::variant::{
    OFFSET_FLAG, UNSIGNED_FLAG, Variant, unpack_float24, unpack_fract24, unpack_int24,
};

/// Packed document reader for BBinaryDataTree format (`BPackedReader`).
///
/// Parses the XMX variant format from raw byte slices. Auto-detects compact
/// vs. XMX encoding based on the signature byte.
pub struct Reader;

impl Reader {
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
    pub fn read_le_at(data: &[u8], header_offset: usize) -> Result<Option<Node>> {
        if compact::is_compact_signature(data, header_offset) {
            return compact::read_compact(data, header_offset, false);
        }
        read_xmx_le(data, header_offset)
    }

    /// Parse big-endian packed data (Xbox 360 format).
    pub fn read_be(data: &[u8]) -> Result<Option<Node>> {
        Self::read_be_at(data, 0)
    }

    /// Parse big-endian packed data with a header offset.
    pub fn read_be_at(data: &[u8], header_offset: usize) -> Result<Option<Node>> {
        if compact::is_compact_signature(data, header_offset) {
            return compact::read_compact(data, header_offset, true);
        }
        read_xmx_be(data, header_offset)
    }
}

/// Packed node structure for reading.
struct PackedNodeRead {
    #[allow(dead_code)]
    parent_node: u32,
    name_variant: u32,
    text_variant: u32,
    attributes: Vec<(u32, u32)>,
    children: Vec<u32>,
}

fn read_xmx_le(data: &[u8], header_offset: usize) -> Result<Option<Node>> {
    let header_data = data.get(header_offset..).ok_or(Error::UnexpectedEof)?;
    let (header, _): (Ref<_, XmxHeaderLe>, _) =
        Ref::from_prefix(header_data).map_err(|_| Error::UnexpectedEof)?;

    let nodes_size = header.nodes_size();
    let nodes_ptr = header.nodes_ptr();
    let variant_data_size = header.variant_size();
    let variant_data_ptr = header.variant_ptr();

    if nodes_size == 0 || nodes_size == 0xFFFFFFFF {
        return Ok(None);
    }

    let variant_data = if variant_data_size > 0 && variant_data_ptr < data.len() {
        let end = (variant_data_ptr + variant_data_size as usize).min(data.len());
        &data[variant_data_ptr..end]
    } else {
        &[]
    };

    let node_bytes = data.get(nodes_ptr..).ok_or(Error::UnexpectedEof)?;
    let (nodes_slice, _): (Ref<_, [XmxNodeLe]>, _) =
        Ref::from_prefix_with_elems(node_bytes, nodes_size as usize)
            .map_err(|_| Error::UnexpectedEof)?;

    let mut packed_nodes = Vec::with_capacity(nodes_size as usize);

    for node in &*nodes_slice {
        let attrs_size = node.attrs_size();
        let attrs_ptr = node.attrs_ptr();
        let children_size = node.children_size();
        let children_ptr = node.children_ptr();

        let mut attributes = Vec::new();
        if attrs_size != 0xFFFFFFFF && attrs_size > 0 && attrs_ptr < data.len() {
            let attr_bytes = data.get(attrs_ptr..).unwrap_or(&[]);
            if let Ok((attr_slice, _)) =
                Ref::<_, [AttrPairRaw]>::from_prefix_with_elems(attr_bytes, attrs_size as usize)
            {
                for attr in &*attr_slice {
                    let name_var = u32::from_le_bytes(attr.name_var);
                    let value_var = u32::from_le_bytes(attr.value_var);
                    attributes.push((name_var, value_var));
                }
            }
        }

        let mut children = Vec::new();
        if children_size != 0xFFFFFFFF && children_size > 0 && children_ptr < data.len() {
            let child_bytes = data.get(children_ptr..).unwrap_or(&[]);
            if let Ok((child_slice, _)) =
                Ref::<_, [u32]>::from_prefix_with_elems(child_bytes, children_size as usize)
            {
                for &child_idx in &*child_slice {
                    children.push(u32::from_le(child_idx));
                }
            }
        }

        packed_nodes.push(PackedNodeRead {
            parent_node: node.parent_node(),
            name_variant: node.name_variant(),
            text_variant: node.text_variant(),
            attributes,
            children,
        });
    }

    build_tree(&packed_nodes, variant_data, false)
}

fn read_xmx_be(data: &[u8], header_offset: usize) -> Result<Option<Node>> {
    let header_data = data.get(header_offset..).ok_or(Error::UnexpectedEof)?;
    let (header, _): (Ref<_, XmxHeaderBe>, _) =
        Ref::from_prefix(header_data).map_err(|_| Error::UnexpectedEof)?;

    let nodes_size = header.nodes_size();
    let nodes_ptr = header.nodes_ptr();
    let variant_data_size = header.variant_size();
    let variant_data_ptr = header.variant_ptr();

    if nodes_size == 0 {
        return Ok(None);
    }

    let variant_data = if variant_data_size > 0 && variant_data_ptr < data.len() {
        let end = (variant_data_ptr + variant_data_size as usize).min(data.len());
        &data[variant_data_ptr..end]
    } else {
        &[]
    };

    let node_bytes = data.get(nodes_ptr..).ok_or(Error::UnexpectedEof)?;
    let (nodes_slice, _): (Ref<_, [XmxNodeBe]>, _) =
        Ref::from_prefix_with_elems(node_bytes, nodes_size as usize)
            .map_err(|_| Error::UnexpectedEof)?;

    let mut packed_nodes = Vec::with_capacity(nodes_size as usize);

    for node in &*nodes_slice {
        let attrs_size = node.attrs_size();
        let attrs_ptr = node.attrs_ptr();
        let children_size = node.children_size();
        let children_ptr = node.children_ptr();

        let mut attributes = Vec::new();
        if attrs_ptr != 0xFFFFFFFF && attrs_size > 0 && attrs_ptr < data.len() {
            let attr_bytes = data.get(attrs_ptr..).unwrap_or(&[]);
            if let Ok((attr_slice, _)) =
                Ref::<_, [AttrPairRaw]>::from_prefix_with_elems(attr_bytes, attrs_size as usize)
            {
                for attr in &*attr_slice {
                    let name_var = u32::from_be_bytes(attr.name_var);
                    let value_var = u32::from_be_bytes(attr.value_var);
                    attributes.push((name_var, value_var));
                }
            }
        }

        let mut children = Vec::new();
        if children_ptr != 0xFFFFFFFF && children_size > 0 && children_ptr < data.len() {
            let child_bytes = data.get(children_ptr..).unwrap_or(&[]);
            if let Ok((child_slice, _)) =
                Ref::<_, [u32]>::from_prefix_with_elems(child_bytes, children_size as usize)
            {
                for &child_idx in &*child_slice {
                    children.push(u32::from_be(child_idx));
                }
            }
        }

        packed_nodes.push(PackedNodeRead {
            parent_node: node.parent_node(),
            name_variant: node.name_variant(),
            text_variant: node.text_variant(),
            attributes,
            children,
        });
    }

    build_tree(&packed_nodes, variant_data, true)
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
        5 => Ok(Variant::Float(unpack_fract24(data_bits))),
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
                Ok(Variant::FloatVec(alloc::vec![0.0; vec_size as usize]))
            }
        }
        _ => Ok(Variant::Null),
    }
}
