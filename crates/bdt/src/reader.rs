//! `BPackedReader` — reads `BBinaryDataTree` packed documents.
//!
//! This is the Ensemble Studios `BPackedReader` implementation. It parses the
//! XMX variant format (used inside XMB files and other ECF containers) from
//! raw byte slices, supporting both little-endian (PC/DE) and big-endian
//! (Xbox 360) layouts.
//!
//! ## Usage
//!
//! ```ignore
//! use bdt::{Endian, Reader};
//!
//! let node = Reader::read(&data, Endian::Little)?;
//! ```

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use nostdio::{Cursor, Endian as IoEndian, ReadEndian};
use zerocopy::Ref;

use crate::Endian;
use crate::compact;
use crate::error::{Error, Result};
use crate::node::{Attribute, Node};
use crate::raw::{AttrPairRaw, XmxHeaderBe, XmxHeaderLe, XmxNodeBe, XmxNodeLe};
use crate::util::{
    assemble_tree, decode_direct_string, read_null_terminated_string, read_null_terminated_wstring,
};
use crate::variant::{
    OFFSET_FLAG, TYPE_MASK, UNSIGNED_FLAG, Variant, VariantType, unpack_float24, unpack_fract24,
    unpack_int24,
};

/// Packed document reader for `BBinaryDataTree` format (`BPackedReader`).
///
/// Parses the XMX variant format from raw byte slices. Auto-detects compact
/// vs. XMX encoding based on the signature byte.
pub struct Reader;

impl Reader {
    /// Parse packed data with the given endianness.
    ///
    /// Auto-detects the format:
    /// - If data starts with `0x3E`/`0xE3`: compact `BPackedHeader` format
    /// - Otherwise: XMX variant format (pad + `BPackedArrays`)
    ///
    /// # Errors
    ///
    /// Returns an error when the packed document is truncated, malformed, or
    /// contains an invalid variant type.
    pub fn read(data: &[u8], endian: Endian) -> Result<Option<Node>> {
        Self::read_at(data, 0, endian)
    }

    /// Parse packed data with a header offset and the given endianness.
    ///
    /// The header starts at `header_offset` within `data`. All internal pointers
    /// (node pointers, attribute pointers, etc.) are absolute offsets from `data[0]`.
    ///
    /// # Errors
    ///
    /// Returns an error when `header_offset` or an internal pointer is outside
    /// `data`, or when the packed document is otherwise malformed.
    pub fn read_at(data: &[u8], header_offset: usize, endian: Endian) -> Result<Option<Node>> {
        let big_endian = endian == Endian::Big;
        if compact::is_compact_signature(data, header_offset) {
            return compact::read_compact(data, header_offset, big_endian);
        }
        match endian {
            Endian::Little => read_xmx_le(data, header_offset),
            Endian::Big => read_xmx_be(data, header_offset),
        }
    }
}

/// Packed node structure for reading.
struct PackedNodeRead {
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
    let nodes_ptr = header.nodes_ptr().ok_or(Error::UnexpectedEof)?;
    let variant_data_size = header.variant_size();
    let variant_data_ptr = header.variant_ptr().ok_or(Error::UnexpectedEof)?;

    if nodes_size == 0 || nodes_size == 0xFFFF_FFFF {
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
        let attrs_ptr = node.attrs_ptr().ok_or(Error::UnexpectedEof)?;
        let children_size = node.children_size();
        let children_ptr = node.children_ptr().ok_or(Error::UnexpectedEof)?;

        let mut attributes = Vec::new();
        if attrs_size != 0xFFFF_FFFF && attrs_size > 0 && attrs_ptr < data.len() {
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
        if children_size != 0xFFFF_FFFF && children_size > 0 && children_ptr < data.len() {
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

    Ok(build_tree(&packed_nodes, variant_data, false))
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
        if attrs_ptr != 0xFFFF_FFFF && attrs_size > 0 && attrs_ptr < data.len() {
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
        if children_ptr != 0xFFFF_FFFF && children_size > 0 && children_ptr < data.len() {
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

    Ok(build_tree(&packed_nodes, variant_data, true))
}

/// Build tree from packed nodes (XMX variant format).
fn build_tree(
    packed_nodes: &[PackedNodeRead],
    variant_data: &[u8],
    big_endian: bool,
) -> Option<Node> {
    if packed_nodes.is_empty() {
        return None;
    }

    let mut nodes: Vec<Node> = Vec::with_capacity(packed_nodes.len());
    let mut child_indices: Vec<Vec<usize>> = Vec::with_capacity(packed_nodes.len());

    for pn in packed_nodes {
        let name = decode_variant_string(pn.name_variant, variant_data);
        let text = decode_variant_to_variant(pn.text_variant, variant_data, big_endian);

        let mut attributes = Vec::with_capacity(pn.attributes.len());
        for (name_var, value_var) in &pn.attributes {
            let attr_name = decode_variant_string(*name_var, variant_data);
            let attr_value = decode_variant_to_variant(*value_var, variant_data, big_endian);
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
        .find(|(i, pn)| pn.parent_node == 0xFFFF_FFFF || pn.parent_node as usize == *i)
        .map_or(0, |(i, _)| i);

    assemble_tree(nodes, &child_indices, root)
}

fn decode_variant_string(variant_value: u32, variant_data: &[u8]) -> String {
    let type_bits = (variant_value >> 24) as u8;
    let data_bits = variant_value & 0x00FF_FFFF;
    let is_offset = (type_bits & OFFSET_FLAG) != 0;

    match VariantType::from_byte(type_bits) {
        Ok(VariantType::Null) => String::new(),
        Ok(VariantType::String) => {
            if is_offset {
                let offset = data_bits as usize;
                read_null_terminated_string(variant_data, offset)
            } else {
                decode_direct_string(data_bits)
            }
        }
        Ok(vt) => format!("<type:{vt:?}>"),
        Err(_) => format!("<type:{}>", type_bits & TYPE_MASK),
    }
}

fn io_endian(big_endian: bool) -> IoEndian {
    if big_endian {
        IoEndian::Big
    } else {
        IoEndian::Little
    }
}

fn read_f32(data: &[u8], offset: usize, big_endian: bool) -> Option<f32> {
    let mut cursor = Cursor::new(data.get(offset..)?);
    cursor.read_f32(io_endian(big_endian)).ok()
}

fn read_i32(data: &[u8], offset: usize, big_endian: bool) -> Option<i32> {
    let mut cursor = Cursor::new(data.get(offset..)?);
    cursor.read_i32(io_endian(big_endian)).ok()
}

fn read_f64(data: &[u8], offset: usize, big_endian: bool) -> Option<f64> {
    let mut cursor = Cursor::new(data.get(offset..)?);
    cursor.read_f64(io_endian(big_endian)).ok()
}

fn decode_variant_to_variant(variant_value: u32, variant_data: &[u8], big_endian: bool) -> Variant {
    let type_bits = variant_value.to_be_bytes()[0];
    let data_bits = variant_value & 0x00FF_FFFF;
    let data_offset = usize::try_from(data_bits).unwrap_or(usize::MAX);
    let is_offset = (type_bits & OFFSET_FLAG) != 0;
    let is_unsigned = (type_bits & UNSIGNED_FLAG) != 0;

    let Ok(vt) = VariantType::from_byte(type_bits) else {
        return Variant::Null;
    };

    match vt {
        VariantType::Null => Variant::Null,
        VariantType::Float24 => Variant::Float(unpack_float24(data_bits)),
        VariantType::Float => Variant::Float(
            is_offset
                .then(|| read_f32(variant_data, data_offset, big_endian))
                .flatten()
                .unwrap_or(0.0),
        ),
        VariantType::Int24 => {
            if is_unsigned {
                Variant::UInt(data_bits)
            } else {
                Variant::Int(unpack_int24(data_bits))
            }
        }
        VariantType::Int32 => {
            if is_offset {
                Variant::Int(read_i32(variant_data, data_offset, big_endian).unwrap_or_default())
            } else {
                Variant::Int(0)
            }
        }
        VariantType::Fract24 => Variant::Fract24(unpack_fract24(data_bits)),
        VariantType::Double => {
            if is_offset {
                Variant::Double(read_f64(variant_data, data_offset, big_endian).unwrap_or_default())
            } else {
                Variant::Double(0.0)
            }
        }
        VariantType::Bool => Variant::Bool(data_bits != 0),
        VariantType::String => {
            if is_offset {
                Variant::String(read_null_terminated_string(variant_data, data_offset))
            } else {
                Variant::String(decode_direct_string(data_bits))
            }
        }
        VariantType::UString => {
            if is_offset {
                Variant::UString(read_null_terminated_wstring(
                    variant_data,
                    data_offset,
                    big_endian,
                ))
            } else {
                Variant::UString(String::new())
            }
        }
        VariantType::FloatVec => {
            let vec_size = usize::from(1 + ((type_bits >> 4) & 0x03));
            let values = is_offset
                .then(|| {
                    (0..vec_size)
                        .map(|index| {
                            let offset = data_offset.checked_add(index.checked_mul(4)?)?;
                            read_f32(variant_data, offset, big_endian)
                        })
                        .collect::<Option<Vec<_>>>()
                })
                .flatten()
                .unwrap_or_else(|| alloc::vec![0.0; vec_size]);
            Variant::FloatVec(values)
        }
    }
}
