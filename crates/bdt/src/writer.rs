//! `BPackedWriter` — writes `BBinaryDataTree` packed documents.
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
use crate::error::{Error, Result};
use crate::node::Node;
use crate::variant::{
    OFFSET_FLAG, UNSIGNED_FLAG, Variant, VariantType, pack_float24, pack_fract24, pack_int24,
    pack_uint24, unpack_float24, unpack_fract24,
};

const HEADER_SIZE: usize = 36;
const NODE_SIZE: usize = 48;

fn checked_u32(value: usize, field: &'static str) -> Result<u32> {
    u32::try_from(value).map_err(|_| Error::SizeOverflow(field))
}

fn checked_add(lhs: u32, rhs: u32, field: &'static str) -> Result<u32> {
    lhs.checked_add(rhs).ok_or(Error::SizeOverflow(field))
}

fn checked_mul(lhs: u32, rhs: u32, field: &'static str) -> Result<u32> {
    lhs.checked_mul(rhs).ok_or(Error::SizeOverflow(field))
}

/// Packed document writer for `BBinaryDataTree` format (`BPackedWriter`).
///
/// Serialises a [`Node`] tree into the XMX variant binary format.
pub struct Writer;

impl Writer {
    /// Build packed data with the given endianness.
    ///
    /// # Errors
    ///
    /// Returns an error when a count, offset, string, or value cannot be
    /// represented by the selected on-disk format.
    pub fn write(root: &Node, endian: Endian) -> Result<Vec<u8>> {
        Self::write_with_base(root, 0, endian)
    }

    /// Build packed data with a pointer base offset and the given endianness.
    ///
    /// # Errors
    ///
    /// Returns an error when pointer arithmetic overflows or a count, offset,
    /// string, or value cannot be represented by the selected on-disk format.
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
        let nodes_len = checked_u32(nodes.len(), "node count")?;
        let variant_data_size_u32 = checked_u32(variant_data_size, "variant data size")?;

        let mut data = Vec::new();

        // Header: pad(4) + nodes BPackedArray(16) + variant BPackedArray(16) = 36 bytes
        data.extend_from_slice(&0u32.to_le_bytes());

        data.extend_from_slice(&nodes_len.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&(nodes_offset as u64).to_le_bytes());

        data.extend_from_slice(&variant_data_size_u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&(variant_data_offset as u64).to_le_bytes());

        let mut current_attrs_offset = attrs_offset;
        let mut current_children_offset = children_offset;

        for (i, node) in nodes.iter().enumerate() {
            data.extend_from_slice(&node.parent_index.to_le_bytes());
            data.extend_from_slice(&node.name_variant.to_le_bytes());
            data.extend_from_slice(&node.text_variant.to_le_bytes());
            data.extend_from_slice(&0u32.to_le_bytes());

            if node.attributes.is_empty() {
                // Empty BPackedArray sentinel: count=0, ptr=0xFFFFFFFF (32-bit).
                // The game's XMX reader checks the u64 pointer against 0x00000000FFFFFFFF
                // to identify empty arrays, then verifies count==0.
                data.extend_from_slice(&0u32.to_le_bytes());
                data.extend_from_slice(&0u32.to_le_bytes());
                data.extend_from_slice(&0x0000_0000_FFFF_FFFF_u64.to_le_bytes());
            } else {
                let attribute_count = checked_u32(node.attributes.len(), "attribute count")?;
                data.extend_from_slice(&attribute_count.to_le_bytes());
                data.extend_from_slice(&0u32.to_le_bytes());
                data.extend_from_slice(&(current_attrs_offset as u64).to_le_bytes());
                current_attrs_offset += node.attributes.len() * 8;
            }

            if node.children.is_empty() {
                // Empty BPackedArray sentinel: count=0, ptr=0xFFFFFFFF (32-bit).
                data.extend_from_slice(&0u32.to_le_bytes());
                data.extend_from_slice(&0u32.to_le_bytes());
                data.extend_from_slice(&0x0000_0000_FFFF_FFFF_u64.to_le_bytes());
            } else {
                let child_count = checked_u32(node.children.len(), "child count")?;
                data.extend_from_slice(&child_count.to_le_bytes());
                data.extend_from_slice(&0u32.to_le_bytes());
                data.extend_from_slice(&(current_children_offset as u64).to_le_bytes());
                current_children_offset += node.children.len() * 4;
            }

            debug_assert_eq!(
                data.len(),
                HEADER_SIZE + (i + 1) * NODE_SIZE,
                "Node size mismatch at index {i}"
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

        collect_nodes(root, 0xFFFF_FFFF, &mut nodes, &mut buf)?;

        if nodes.is_empty() {
            let mut data = Vec::new();
            data.extend_from_slice(&0u32.to_be_bytes());
            data.extend_from_slice(&0u32.to_be_bytes());
            data.extend_from_slice(&0u32.to_be_bytes());
            data.extend_from_slice(&0u32.to_be_bytes());
            return Ok(data);
        }

        let header_size = 16u32;
        let nodes_ptr = checked_add(
            header_size,
            checked_u32(pointer_base, "pointer base")?,
            "node pointer",
        )?;
        let nodes_size = checked_u32(nodes.len(), "node count")?;
        let nodes_array_size = checked_mul(nodes_size, 28, "node array size")?;

        let mut current_offset = checked_add(nodes_ptr, nodes_array_size, "attribute offset")?;
        let mut node_attrs_ptrs: Vec<u32> = Vec::with_capacity(nodes.len());
        let mut node_children_ptrs: Vec<u32> = Vec::with_capacity(nodes.len());

        for node in &nodes {
            if node.attributes.is_empty() {
                node_attrs_ptrs.push(0xFFFF_FFFF);
            } else {
                node_attrs_ptrs.push(current_offset);
                let bytes = checked_mul(
                    checked_u32(node.attributes.len(), "attribute count")?,
                    8,
                    "attribute data size",
                )?;
                current_offset = checked_add(current_offset, bytes, "attribute offset")?;
            }

            if node.children.is_empty() {
                node_children_ptrs.push(0xFFFF_FFFF);
            } else {
                node_children_ptrs.push(current_offset);
                let bytes = checked_mul(
                    checked_u32(node.children.len(), "child count")?,
                    4,
                    "child index data size",
                )?;
                current_offset = checked_add(current_offset, bytes, "child index offset")?;
            }
        }

        let variant_data_ptr = current_offset;
        let variant_data_size = checked_u32(buf.data.len(), "variant data size")?;

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
            let attribute_count = checked_u32(node.attributes.len(), "attribute count")?;
            data.extend_from_slice(&attribute_count.to_be_bytes());
            data.extend_from_slice(&node_attrs_ptrs[i].to_be_bytes());
            let child_count = checked_u32(node.children.len(), "child count")?;
            data.extend_from_slice(&child_count.to_be_bytes());
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
    let node_index = checked_u32(nodes.len(), "node index")?;

    let name_variant = buf.add_string(&node.name)?;
    let text_variant = pack_variant(&node.text, buf)?;

    let mut attributes = Vec::with_capacity(node.attributes.len());
    for attr in &node.attributes {
        let name_var = buf.add_string(&attr.name)?;
        let value_var = pack_variant(&attr.value, buf)?;
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

    let node_slot = usize::try_from(node_index).map_err(|_| Error::SizeOverflow("node index"))?;
    nodes[node_slot].children = child_indices;

    Ok(node_index)
}

/// Pack a variant value, storing offset-based data in the buffer.
fn pack_variant(variant: &Variant, buf: &mut VariantBuffer) -> Result<u32> {
    match variant {
        Variant::Null => Ok(0),
        Variant::Bool(v) => Ok(((VariantType::Bool as u32) << 24) | u32::from(*v)),
        Variant::Int(v) => {
            if *v >= -8_388_608 && *v <= 8_388_607 {
                Ok(((VariantType::Int24 as u32) << 24) | pack_int24(*v))
            } else {
                buf.add_int32(*v)
            }
        }
        Variant::UInt(v) => {
            if *v <= 0x00FF_FFFF {
                Ok(
                    ((VariantType::Int24 as u32 | u32::from(UNSIGNED_FLAG)) << 24)
                        | pack_uint24(*v),
                )
            } else {
                buf.add_int32((*v).cast_signed())
            }
        }
        Variant::Float(v) | Variant::Fract24(v) => {
            // Try Fract24 first: value × 10000, rounded, must fit in 23 bits
            // (magnitude ≤ 0x7FFFFF = 8388607, i.e. |value| ≤ 838.8607).
            // Use tolerance for IEEE 754 imprecision (e.g. 1.05f32 = 1.04999995...).
            let scaled = (*v * 10000.0).round();
            let diff = (scaled - *v * 10000.0).abs();
            if diff < 0.5 && scaled.abs() <= 8_388_607.0 {
                let packed = pack_fract24(*v);
                // Verify roundtrip: decode must match the original
                let rt_val = unpack_fract24(packed);
                if (rt_val - *v).abs() < 1e-4 {
                    return Ok(((VariantType::Fract24 as u32) << 24) | packed);
                }
            }

            // Then try Float24
            let packed = pack_float24(*v);
            let unpacked = unpack_float24(packed);
            if (*v - unpacked).abs() < 0.001 || v.to_bits() << 1 == 0 {
                Ok(((VariantType::Float24 as u32) << 24) | packed)
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
    fn offset(&self) -> Result<u32> {
        let offset = checked_u32(self.data.len(), "variant data offset")?;
        if offset > 0x00FF_FFFF {
            return Err(Error::SizeOverflow("24-bit variant data offset"));
        }
        Ok(offset)
    }

    /// Append a UTF-8 string (deduplicated). Returns the packed variant u32.
    fn add_string(&mut self, s: &str) -> Result<u32> {
        if let Some(&offset) = self.string_offsets.get(s) {
            return Ok(((VariantType::String as u32 | u32::from(OFFSET_FLAG)) << 24) | offset);
        }
        let offset = self.offset()?;
        self.data.extend_from_slice(s.as_bytes());
        self.data.push(0);
        self.string_offsets.insert(s.to_string(), offset);
        Ok(((VariantType::String as u32 | u32::from(OFFSET_FLAG)) << 24) | offset)
    }

    /// Append a UTF-16 string (not deduplicated). Returns the packed variant u32.
    fn add_ustring(&mut self, s: &str) -> Result<u32> {
        let offset = self.offset()?;
        for c in s.encode_utf16() {
            let bytes = if self.big_endian {
                c.to_be_bytes()
            } else {
                c.to_le_bytes()
            };
            self.data.extend_from_slice(&bytes);
        }
        self.data.push(0);
        self.data.push(0);
        Ok(((VariantType::UString as u32 | u32::from(OFFSET_FLAG)) << 24) | offset)
    }

    /// Append a 32-bit float. Returns the packed variant u32.
    fn add_float(&mut self, v: f32) -> Result<u32> {
        let offset = self.offset()?;
        if self.big_endian {
            self.data.extend_from_slice(&v.to_be_bytes());
        } else {
            self.data.extend_from_slice(&v.to_le_bytes());
        }
        Ok(((VariantType::Float as u32 | u32::from(OFFSET_FLAG)) << 24) | offset)
    }

    /// Append a 64-bit double. Returns the packed variant u32.
    fn add_double(&mut self, v: f64) -> Result<u32> {
        let offset = self.offset()?;
        if self.big_endian {
            self.data.extend_from_slice(&v.to_be_bytes());
        } else {
            self.data.extend_from_slice(&v.to_le_bytes());
        }
        Ok(((VariantType::Double as u32 | u32::from(OFFSET_FLAG)) << 24) | offset)
    }

    /// Append a 32-bit integer. Returns the packed variant u32.
    fn add_int32(&mut self, v: i32) -> Result<u32> {
        let offset = self.offset()?;
        if self.big_endian {
            self.data.extend_from_slice(&v.to_be_bytes());
        } else {
            self.data.extend_from_slice(&v.to_le_bytes());
        }
        Ok(((VariantType::Int32 as u32 | u32::from(OFFSET_FLAG)) << 24) | offset)
    }

    /// Append a float vector (2–4 elements). Returns the packed variant u32.
    fn add_float_vec(&mut self, v: &[f32]) -> Result<u32> {
        let offset = self.offset()?;
        for f in v {
            if self.big_endian {
                self.data.extend_from_slice(&f.to_be_bytes());
            } else {
                self.data.extend_from_slice(&f.to_le_bytes());
            }
        }
        let vec_size_bits = match v.len() {
            3 => 1u32,
            4 => 2u32,
            _ => 0u32,
        };
        Ok(
            ((VariantType::FloatVec as u32 | u32::from(OFFSET_FLAG) | (vec_size_bits << 5)) << 24)
                | offset,
        )
    }
}
