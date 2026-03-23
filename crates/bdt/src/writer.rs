//! BPackedWriter — writes BBinaryDataTree packed documents.
//!
//! This is the Ensemble Studios `BPackedWriter` implementation. It serialises
//! a [`Node`] tree into the XMX variant format, supporting both little-endian
//! (PC/DE) and big-endian (Xbox 360) layouts.
//!
//! ## Usage
//!
//! ```ignore
//! use bdt::{Endian, Writer};
//!
//! let data = Writer::write(&root, Endian::Little)?;
//! ```

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use hashbrown::HashMap;

use crate::Endian;
use crate::error::Result;
use crate::node::Node;
use crate::variant::{
    OFFSET_FLAG, UNSIGNED_FLAG, Variant, VariantType, pack_float24, pack_fract24, pack_fract24_str,
    pack_int24, pack_uint24, unpack_float24, unpack_fract24,
};

/// Packed document writer for BBinaryDataTree format (`BPackedWriter`).
///
/// Serialises a [`Node`] tree into the XMX variant binary format.
pub struct Writer;

impl Writer {
    /// Build packed data with the given endianness.
    pub fn write(root: &Node, endian: Endian) -> Result<Vec<u8>> {
        Self::write_with_base(root, 0, endian)
    }

    /// Build packed data with a pointer base offset and the given endianness.
    pub fn write_with_base(root: &Node, pointer_base: usize, endian: Endian) -> Result<Vec<u8>> {
        match endian {
            Endian::Little => Self::write_le_impl(root, pointer_base),
            Endian::Big => Self::write_be_impl(root, pointer_base),
        }
    }

    /// Build little-endian packed data with a pointer base offset.
    fn write_le_impl(root: &Node, pointer_base: usize) -> Result<Vec<u8>> {
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

    /// Build big-endian packed data with a pointer base offset.
    fn write_be_impl(root: &Node, pointer_base: usize) -> Result<Vec<u8>> {
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
            // Try Fract24 first: value × 10000, rounded, must fit in 23 bits
            // (magnitude ≤ 0x7FFFFF = 8388607, i.e. |value| ≤ 838.8607).
            // Use tolerance for IEEE 754 imprecision (e.g. 1.05f32 = 1.04999995...).
            let scaled = (*v * 10000.0).round();
            let diff = (scaled - *v * 10000.0).abs();
            if diff < 0.5 && scaled.abs() <= 8_388_607.0 {
                let packed = pack_fract24(*v);
                // Verify roundtrip: the string → f32 must match the original
                let rt = unpack_fract24(packed);
                let rt_val: f32 = rt.parse().unwrap_or(f32::NAN);
                if (rt_val - *v).abs() < 1e-4 {
                    return ((VariantType::Fract24 as u32) << 24) | packed;
                }
            }

            // Then try Float24
            let packed = pack_float24(*v);
            let unpacked = unpack_float24(packed);
            if (*v - unpacked).abs() < 0.001 || *v == 0.0 {
                ((VariantType::Float24 as u32) << 24) | packed
            } else {
                buf.add_float(*v)
            }
        }
        Variant::Fract24(s) => ((VariantType::Fract24 as u32) << 24) | pack_fract24_str(s),
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
