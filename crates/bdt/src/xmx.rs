//! XMX variant format reader and writer.
//!
//! The XMX format is used by XMB files after the 4-byte XMB signature. It uses
//! 48-byte nodes (LE) or 28-byte nodes (BE) with BPackedArray pointers and the
//! XMX variant type encoding.
//!
//! ## Reading
//!
//! - `PackedReader::read_le` / `read_le_at` — little-endian (PC/DE)
//! - `PackedReader::read_be` / `read_be_at` — big-endian (Xbox 360)
//!
//! ## Writing
//!
//! - `PackedWriter::write_le` / `write_le_with_base` — little-endian
//! - `PackedWriter::write_be` / `write_be_with_base` — big-endian

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use hashbrown::HashMap;
use zerocopy::Ref;

use crate::compact;
use crate::error::{Error, Result};
use crate::node::{Attribute, Node};
use crate::raw::{AttrPairRaw, XmxHeaderBe, XmxHeaderLe, XmxNodeBe, XmxNodeLe};
use crate::util::{
    assemble_tree, decode_direct_string, read_null_terminated_string, read_null_terminated_wstring,
};
use crate::variant::{
    OFFSET_FLAG, UNSIGNED_FLAG, Variant, VariantType, pack_float24, pack_int24, pack_uint24,
    unpack_float24, unpack_fract24, unpack_int24,
};

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

/// Packed document writer for BBinaryDataTree format.
pub struct PackedWriter;

impl PackedWriter {
    /// Build little-endian packed data (PC/Definitive Edition format).
    pub fn write_le(root: &Node) -> Result<Vec<u8>> {
        Self::write_le_with_base(root, 0)
    }

    /// Build little-endian packed data with a pointer base offset.
    pub fn write_le_with_base(root: &Node, pointer_base: usize) -> Result<Vec<u8>> {
        let mut buf = VariantBuffer::new(false);
        let mut nodes = Vec::new();

        collect_nodes(root, u32::MAX, &mut nodes, &mut buf)?;

        const HEADER_SIZE: usize = 36;
        const NODE_SIZE: usize = 48;

        let nodes_offset = HEADER_SIZE + pointer_base;
        let nodes_byte_size = nodes.len() * NODE_SIZE;

        let attrs_offset = nodes_offset + nodes_byte_size;
        let mut children_offset = attrs_offset;
        for node in &nodes {
            children_offset += node.attributes.len() * 8;
        }

        let mut variant_data_offset = children_offset;
        for node in &nodes {
            variant_data_offset += node.children.len() * 4;
        }

        let variant_data_size = buf.data.len();

        let mut data = Vec::new();

        // Header: pad(4) + nodes BPackedArray(16) + variant BPackedArray(16) = 36 bytes
        data.extend_from_slice(&0u32.to_le_bytes());

        data.extend_from_slice(&(nodes.len() as u32).to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&(nodes_offset as u64).to_le_bytes());

        data.extend_from_slice(&(variant_data_size as u32).to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&(variant_data_offset as u64).to_le_bytes());

        let mut current_attrs_offset = attrs_offset;
        let mut current_children_offset = children_offset;

        for (i, node) in nodes.iter().enumerate() {
            data.extend_from_slice(&node.parent_index.to_le_bytes());
            data.extend_from_slice(&node.name_variant.to_le_bytes());
            data.extend_from_slice(&node.text_variant.to_le_bytes());
            data.extend_from_slice(&0u32.to_le_bytes());

            if !node.attributes.is_empty() {
                data.extend_from_slice(&(node.attributes.len() as u32).to_le_bytes());
                data.extend_from_slice(&0u32.to_le_bytes());
                data.extend_from_slice(&(current_attrs_offset as u64).to_le_bytes());
                current_attrs_offset += node.attributes.len() * 8;
            } else {
                data.extend_from_slice(&0xFFFFFFFFu32.to_le_bytes());
                data.extend_from_slice(&0u32.to_le_bytes());
                data.extend_from_slice(&0u64.to_le_bytes());
            }

            if !node.children.is_empty() {
                data.extend_from_slice(&(node.children.len() as u32).to_le_bytes());
                data.extend_from_slice(&0u32.to_le_bytes());
                data.extend_from_slice(&(current_children_offset as u64).to_le_bytes());
                current_children_offset += node.children.len() * 4;
            } else {
                data.extend_from_slice(&0xFFFFFFFFu32.to_le_bytes());
                data.extend_from_slice(&0u32.to_le_bytes());
                data.extend_from_slice(&0xFFFFFFFFFFFFFFFFu64.to_le_bytes());
            }

            debug_assert_eq!(
                data.len(),
                HEADER_SIZE + (i + 1) * NODE_SIZE,
                "Node size mismatch at index {}",
                i
            );
        }

        // Write attributes
        for node in &nodes {
            for (name_var, value_var) in &node.attributes {
                data.extend_from_slice(&name_var.to_le_bytes());
                data.extend_from_slice(&value_var.to_le_bytes());
            }
        }

        // Write children indices
        for node in &nodes {
            for &child_idx in &node.children {
                data.extend_from_slice(&child_idx.to_le_bytes());
            }
        }

        // Write variant data
        data.extend_from_slice(&buf.data);

        Ok(data)
    }

    /// Build big-endian packed data (Xbox 360 format).
    pub fn write_be(root: &Node) -> Result<Vec<u8>> {
        Self::write_be_with_base(root, 0)
    }

    /// Build big-endian packed data with a pointer base offset.
    pub fn write_be_with_base(root: &Node, pointer_base: usize) -> Result<Vec<u8>> {
        let mut buf = VariantBuffer::new(true);
        let mut nodes = Vec::new();

        collect_nodes(root, 0xFFFFFFFF, &mut nodes, &mut buf)?;

        if nodes.is_empty() {
            let mut data = Vec::new();
            data.extend_from_slice(&0u32.to_be_bytes());
            data.extend_from_slice(&0u32.to_be_bytes());
            data.extend_from_slice(&0u32.to_be_bytes());
            data.extend_from_slice(&0u32.to_be_bytes());
            return Ok(data);
        }

        let header_size = 16u32;
        let nodes_ptr = header_size + pointer_base as u32;
        let nodes_size = nodes.len() as u32;
        let nodes_array_size = nodes_size * 28;

        let mut current_offset = nodes_ptr + nodes_array_size;
        let mut node_attrs_ptrs: Vec<u32> = Vec::with_capacity(nodes.len());
        let mut node_children_ptrs: Vec<u32> = Vec::with_capacity(nodes.len());

        for node in &nodes {
            if node.attributes.is_empty() {
                node_attrs_ptrs.push(0xFFFFFFFF);
            } else {
                node_attrs_ptrs.push(current_offset);
                current_offset += (node.attributes.len() as u32) * 8;
            }

            if node.children.is_empty() {
                node_children_ptrs.push(0xFFFFFFFF);
            } else {
                node_children_ptrs.push(current_offset);
                current_offset += (node.children.len() as u32) * 4;
            }
        }

        let variant_data_ptr = current_offset;
        let variant_data_size = buf.data.len() as u32;

        let mut data = Vec::new();

        // Header
        data.extend_from_slice(&nodes_size.to_be_bytes());
        data.extend_from_slice(&nodes_ptr.to_be_bytes());
        data.extend_from_slice(&variant_data_size.to_be_bytes());
        data.extend_from_slice(&variant_data_ptr.to_be_bytes());

        // Nodes
        for (i, node) in nodes.iter().enumerate() {
            data.extend_from_slice(&node.parent_index.to_be_bytes());
            data.extend_from_slice(&node.name_variant.to_be_bytes());
            data.extend_from_slice(&node.text_variant.to_be_bytes());
            data.extend_from_slice(&(node.attributes.len() as u32).to_be_bytes());
            data.extend_from_slice(&node_attrs_ptrs[i].to_be_bytes());
            data.extend_from_slice(&(node.children.len() as u32).to_be_bytes());
            data.extend_from_slice(&node_children_ptrs[i].to_be_bytes());
        }

        // Attributes and children
        for node in &nodes {
            for (name_var, value_var) in &node.attributes {
                data.extend_from_slice(&name_var.to_be_bytes());
                data.extend_from_slice(&value_var.to_be_bytes());
            }
            for child_idx in &node.children {
                data.extend_from_slice(&child_idx.to_be_bytes());
            }
        }

        // Variant data
        data.extend_from_slice(&buf.data);

        Ok(data)
    }
}

/// Collected node data during tree traversal.
struct CollectedNode {
    parent_index: u32,
    name_variant: u32,
    text_variant: u32,
    attributes: Vec<(u32, u32)>,
    children: Vec<u32>,
}

/// Recursively collect nodes from the tree, packing variants into the buffer.
fn collect_nodes(
    node: &Node,
    parent_index: u32,
    nodes: &mut Vec<CollectedNode>,
    buf: &mut VariantBuffer,
) -> Result<u32> {
    let node_index = nodes.len() as u32;

    let name_variant = buf.add_string(&node.name);
    let text_variant = pack_variant(&node.text, buf);

    let mut attributes = Vec::with_capacity(node.attributes.len());
    for attr in &node.attributes {
        let name_var = buf.add_string(&attr.name);
        let value_var = pack_variant(&attr.value, buf);
        attributes.push((name_var, value_var));
    }

    nodes.push(CollectedNode {
        parent_index,
        name_variant,
        text_variant,
        attributes,
        children: Vec::new(),
    });

    let mut child_indices = Vec::with_capacity(node.children.len());
    for child in &node.children {
        let child_idx = collect_nodes(child, node_index, nodes, buf)?;
        child_indices.push(child_idx);
    }

    nodes[node_index as usize].children = child_indices;

    Ok(node_index)
}

/// Pack a variant value, storing offset-based data in the buffer.
fn pack_variant(variant: &Variant, buf: &mut VariantBuffer) -> u32 {
    match variant {
        Variant::Null => 0,
        Variant::Bool(v) => ((VariantType::Bool as u32) << 24) | (if *v { 1 } else { 0 }),
        Variant::Int(v) => {
            if *v >= -8_388_608 && *v <= 8_388_607 {
                ((VariantType::Int24 as u32) << 24) | pack_int24(*v)
            } else {
                buf.add_int32(*v)
            }
        }
        Variant::UInt(v) => {
            if *v <= 0xFFFFFF {
                ((VariantType::Int24 as u32 | UNSIGNED_FLAG as u32) << 24) | pack_uint24(*v)
            } else {
                buf.add_int32(*v as i32)
            }
        }
        Variant::Float(v) => {
            let packed = pack_float24(*v);
            let unpacked = unpack_float24(packed);
            if (*v - unpacked).abs() < 0.001 || *v == 0.0 {
                ((VariantType::Float24 as u32) << 24) | packed
            } else {
                buf.add_float(*v)
            }
        }
        Variant::Double(v) => buf.add_double(*v),
        Variant::String(s) => buf.add_string(s),
        Variant::UString(s) => buf.add_ustring(s),
        Variant::FloatVec(v) => buf.add_float_vec(v),
    }
}

/// Accumulates variant data (strings, floats, doubles, etc.) during writing.
///
/// Values that don't fit in the 24-bit direct field are appended here and
/// referenced by offset. Strings are deduplicated via `string_offsets`.
struct VariantBuffer {
    /// Raw byte buffer for all indirect variant data.
    data: Vec<u8>,
    /// Deduplication map: string content → byte offset in `data`.
    string_offsets: HashMap<String, u32>,
    /// Whether to write multi-byte values in big-endian order.
    big_endian: bool,
}

impl VariantBuffer {
    /// Create an empty buffer for the given endianness.
    fn new(big_endian: bool) -> Self {
        Self {
            data: Vec::new(),
            string_offsets: HashMap::new(),
            big_endian,
        }
    }

    /// Append a UTF-8 string (deduplicated). Returns the packed variant u32.
    fn add_string(&mut self, s: &str) -> u32 {
        if let Some(&offset) = self.string_offsets.get(s) {
            return ((VariantType::String as u32 | OFFSET_FLAG as u32) << 24) | offset;
        }
        let offset = self.data.len() as u32;
        self.data.extend_from_slice(s.as_bytes());
        self.data.push(0);
        self.string_offsets.insert(s.to_string(), offset);
        ((VariantType::String as u32 | OFFSET_FLAG as u32) << 24) | offset
    }

    /// Append a UTF-16 string (not deduplicated). Returns the packed variant u32.
    fn add_ustring(&mut self, s: &str) -> u32 {
        let offset = self.data.len() as u32;
        for c in s.encode_utf16() {
            if self.big_endian {
                self.data.push((c >> 8) as u8);
                self.data.push((c & 0xFF) as u8);
            } else {
                self.data.push((c & 0xFF) as u8);
                self.data.push((c >> 8) as u8);
            }
        }
        self.data.push(0);
        self.data.push(0);
        ((VariantType::UString as u32 | OFFSET_FLAG as u32) << 24) | offset
    }

    /// Append a 32-bit float. Returns the packed variant u32.
    fn add_float(&mut self, v: f32) -> u32 {
        let offset = self.data.len() as u32;
        if self.big_endian {
            self.data.extend_from_slice(&v.to_be_bytes());
        } else {
            self.data.extend_from_slice(&v.to_le_bytes());
        }
        ((VariantType::Float as u32 | OFFSET_FLAG as u32) << 24) | offset
    }

    /// Append a 64-bit double. Returns the packed variant u32.
    fn add_double(&mut self, v: f64) -> u32 {
        let offset = self.data.len() as u32;
        if self.big_endian {
            self.data.extend_from_slice(&v.to_be_bytes());
        } else {
            self.data.extend_from_slice(&v.to_le_bytes());
        }
        ((VariantType::Double as u32 | OFFSET_FLAG as u32) << 24) | offset
    }

    /// Append a 32-bit integer. Returns the packed variant u32.
    fn add_int32(&mut self, v: i32) -> u32 {
        let offset = self.data.len() as u32;
        if self.big_endian {
            self.data.extend_from_slice(&v.to_be_bytes());
        } else {
            self.data.extend_from_slice(&v.to_le_bytes());
        }
        ((VariantType::Int32 as u32 | OFFSET_FLAG as u32) << 24) | offset
    }

    /// Append a float vector (2–4 elements). Returns the packed variant u32.
    fn add_float_vec(&mut self, v: &[f32]) -> u32 {
        let offset = self.data.len() as u32;
        for f in v {
            if self.big_endian {
                self.data.extend_from_slice(&f.to_be_bytes());
            } else {
                self.data.extend_from_slice(&f.to_le_bytes());
            }
        }
        let vec_size_bits = match v.len() {
            2 => 0u32,
            3 => 1u32,
            4 => 2u32,
            _ => 0u32,
        };
        ((VariantType::FloatVec as u32 | OFFSET_FLAG as u32 | (vec_size_bits << 5)) << 24) | offset
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
                Ok(Variant::FloatVec(vec![0.0; vec_size as usize]))
            }
        }
        _ => Ok(Variant::Null),
    }
}
