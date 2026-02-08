//! BBinaryDataTree packed document writer.
//!
//! Builds the packed binary tree format used by Ensemble Studios games.
//! Supports both little-endian (PC/DE) and big-endian (Xbox 360) formats.

use byteorder::{BigEndian, LittleEndian, WriteBytesExt};

use crate::error::Result;
use crate::types::Node;
use crate::variant::{
    pack_float24, pack_int24, pack_uint24, unpack_float24, Variant, VariantType, OFFSET_FLAG,
    UNSIGNED_FLAG,
};

/// Packed document writer for BBinaryDataTree format.
pub struct PackedWriter;

impl PackedWriter {
    /// Build little-endian packed data (PC/Definitive Edition format).
    ///
    /// Returns the packed binary data (without any container-specific signature).
    /// All internal pointers are relative to the start of the output.
    pub fn write_le(root: &Node) -> Result<Vec<u8>> {
        Self::write_le_with_base(root, 0)
    }

    /// Build little-endian packed data with a pointer base offset.
    ///
    /// All internal pointer values are shifted by `pointer_base`. This is useful
    /// when the output will be prefixed with additional data (e.g., XMB's 4-byte
    /// signature), so that pointers are absolute from the start of the combined output.
    ///
    /// The returned data does NOT include the prefix - the caller is responsible
    /// for prepending it.
    pub fn write_le_with_base(root: &Node, pointer_base: usize) -> Result<Vec<u8>> {
        let mut buf = VariantBuffer::new(false);
        let mut nodes = Vec::new();

        collect_nodes(root, u32::MAX, &mut nodes, &mut buf)?;

        const HEADER_SIZE: usize = 36;
        const NODE_SIZE: usize = 48;

        // Pointer values are offset by pointer_base to account for any prefix
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
        data.write_u32::<LittleEndian>(0)?; // padding

        data.write_u32::<LittleEndian>(nodes.len() as u32)?;
        data.write_u32::<LittleEndian>(0)?;
        data.write_u64::<LittleEndian>(nodes_offset as u64)?;

        data.write_u32::<LittleEndian>(variant_data_size as u32)?;
        data.write_u32::<LittleEndian>(0)?;
        data.write_u64::<LittleEndian>(variant_data_offset as u64)?;

        let mut current_attrs_offset = attrs_offset;
        let mut current_children_offset = children_offset;

        for (i, node) in nodes.iter().enumerate() {
            data.write_u32::<LittleEndian>(node.parent_index)?;
            data.write_u32::<LittleEndian>(node.name_variant)?;
            data.write_u32::<LittleEndian>(node.text_variant)?;
            data.write_u32::<LittleEndian>(0)?; // padding

            if !node.attributes.is_empty() {
                data.write_u32::<LittleEndian>(node.attributes.len() as u32)?;
                data.write_u32::<LittleEndian>(0)?;
                data.write_u64::<LittleEndian>(current_attrs_offset as u64)?;
                current_attrs_offset += node.attributes.len() * 8;
            } else {
                data.write_u32::<LittleEndian>(0xFFFFFFFF)?;
                data.write_u32::<LittleEndian>(0)?;
                data.write_u64::<LittleEndian>(0)?;
            }

            if !node.children.is_empty() {
                data.write_u32::<LittleEndian>(node.children.len() as u32)?;
                data.write_u32::<LittleEndian>(0)?;
                data.write_u64::<LittleEndian>(current_children_offset as u64)?;
                current_children_offset += node.children.len() * 4;
            } else {
                data.write_u32::<LittleEndian>(0xFFFFFFFF)?;
                data.write_u32::<LittleEndian>(0)?;
                data.write_u64::<LittleEndian>(0xFFFFFFFFFFFFFFFF)?;
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
                data.write_u32::<LittleEndian>(*name_var)?;
                data.write_u32::<LittleEndian>(*value_var)?;
            }
        }

        // Write children indices
        for node in &nodes {
            for &child_idx in &node.children {
                data.write_u32::<LittleEndian>(child_idx)?;
            }
        }

        // Write variant data
        data.extend_from_slice(&buf.data);

        Ok(data)
    }

    /// Build big-endian packed data (Xbox 360 format).
    ///
    /// Returns the packed binary data (without any container-specific signature).
    /// All internal pointers are relative to the start of the output.
    pub fn write_be(root: &Node) -> Result<Vec<u8>> {
        Self::write_be_with_base(root, 0)
    }

    /// Build big-endian packed data with a pointer base offset.
    ///
    /// All internal pointer values are shifted by `pointer_base`.
    pub fn write_be_with_base(root: &Node, pointer_base: usize) -> Result<Vec<u8>> {
        let mut buf = VariantBuffer::new(true);
        let mut nodes = Vec::new();

        collect_nodes(root, 0xFFFFFFFF, &mut nodes, &mut buf)?;

        if nodes.is_empty() {
            let mut data = Vec::new();
            data.write_u32::<BigEndian>(0)?; // nodes_size
            data.write_u32::<BigEndian>(0)?; // nodes_ptr
            data.write_u32::<BigEndian>(0)?; // variant_data_size
            data.write_u32::<BigEndian>(0)?; // variant_data_ptr
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
        data.write_u32::<BigEndian>(nodes_size)?;
        data.write_u32::<BigEndian>(nodes_ptr)?;
        data.write_u32::<BigEndian>(variant_data_size)?;
        data.write_u32::<BigEndian>(variant_data_ptr)?;

        // Nodes
        for (i, node) in nodes.iter().enumerate() {
            data.write_u32::<BigEndian>(node.parent_index)?;
            data.write_u32::<BigEndian>(node.name_variant)?;
            data.write_u32::<BigEndian>(node.text_variant)?;
            data.write_u32::<BigEndian>(node.attributes.len() as u32)?;
            data.write_u32::<BigEndian>(node_attrs_ptrs[i])?;
            data.write_u32::<BigEndian>(node.children.len() as u32)?;
            data.write_u32::<BigEndian>(node_children_ptrs[i])?;
        }

        // Attributes and children
        for node in &nodes {
            for (name_var, value_var) in &node.attributes {
                data.write_u32::<BigEndian>(*name_var)?;
                data.write_u32::<BigEndian>(*value_var)?;
            }
            for child_idx in &node.children {
                data.write_u32::<BigEndian>(*child_idx)?;
            }
        }

        // Variant data
        data.extend_from_slice(&buf.data);

        Ok(data)
    }
}

// ============================================================================
// Shared types and functions
// ============================================================================

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

// ============================================================================
// Variant data buffer
// ============================================================================

/// Combined buffer for all variant data (strings + data values).
///
/// Uses a single buffer so all offsets are correct without fixup.
struct VariantBuffer {
    data: Vec<u8>,
    string_offsets: std::collections::HashMap<String, u32>,
    big_endian: bool,
}

impl VariantBuffer {
    fn new(big_endian: bool) -> Self {
        Self {
            data: Vec::new(),
            string_offsets: std::collections::HashMap::new(),
            big_endian,
        }
    }

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

    fn add_float(&mut self, v: f32) -> u32 {
        let offset = self.data.len() as u32;
        if self.big_endian {
            self.data.extend_from_slice(&v.to_be_bytes());
        } else {
            self.data.extend_from_slice(&v.to_le_bytes());
        }
        ((VariantType::Float as u32 | OFFSET_FLAG as u32) << 24) | offset
    }

    fn add_double(&mut self, v: f64) -> u32 {
        let offset = self.data.len() as u32;
        if self.big_endian {
            self.data.extend_from_slice(&v.to_be_bytes());
        } else {
            self.data.extend_from_slice(&v.to_le_bytes());
        }
        ((VariantType::Double as u32 | OFFSET_FLAG as u32) << 24) | offset
    }

    fn add_int32(&mut self, v: i32) -> u32 {
        let offset = self.data.len() as u32;
        if self.big_endian {
            self.data.extend_from_slice(&v.to_be_bytes());
        } else {
            self.data.extend_from_slice(&v.to_le_bytes());
        }
        ((VariantType::Int32 as u32 | OFFSET_FLAG as u32) << 24) | offset
    }

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
