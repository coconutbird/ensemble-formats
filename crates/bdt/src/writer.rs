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
        let mut string_table = StringTableLe::new();
        let mut data_table = DataTableLe::new();

        let mut packed_nodes: Vec<PackedNodeLe> = Vec::new();
        let mut all_attributes: Vec<(u32, u32)> = Vec::new();

        collect_nodes_le(
            root,
            u32::MAX,
            &mut packed_nodes,
            &mut all_attributes,
            &mut string_table,
            &mut data_table,
        )?;

        const HEADER_SIZE: usize = 36;
        const NODE_SIZE: usize = 48;

        // Pointer values are offset by pointer_base to account for any prefix
        let nodes_offset = HEADER_SIZE + pointer_base;
        let nodes_size = packed_nodes.len() * NODE_SIZE;

        let attrs_offset = nodes_offset + nodes_size;
        let mut children_offset = attrs_offset;

        for pn in &packed_nodes {
            children_offset += pn.num_attrs * 8;
        }

        let mut variant_data_offset = children_offset;
        for pn in &packed_nodes {
            variant_data_offset += pn.num_children * 4;
        }

        let variant_data_size = string_table.data.len() + data_table.data.len();

        let mut data = Vec::new();

        // Header: pad(4) + nodes BPackedArray(16) + variant BPackedArray(16) = 36 bytes
        data.write_u32::<LittleEndian>(0)?; // padding

        data.write_u32::<LittleEndian>(packed_nodes.len() as u32)?;
        data.write_u32::<LittleEndian>(0)?;
        data.write_u64::<LittleEndian>(nodes_offset as u64)?;

        data.write_u32::<LittleEndian>(variant_data_size as u32)?;
        data.write_u32::<LittleEndian>(0)?;
        data.write_u64::<LittleEndian>(variant_data_offset as u64)?;

        let mut current_attrs_offset = attrs_offset;
        let mut current_children_offset = children_offset;

        for (i, pn) in packed_nodes.iter().enumerate() {
            data.write_u32::<LittleEndian>(pn.parent_index)?;
            data.write_u32::<LittleEndian>(pn.name_variant)?;
            data.write_u32::<LittleEndian>(pn.text_variant)?;
            data.write_u32::<LittleEndian>(0)?; // padding

            if pn.num_attrs > 0 {
                data.write_u32::<LittleEndian>(pn.num_attrs as u32)?;
                data.write_u32::<LittleEndian>(0)?;
                data.write_u64::<LittleEndian>(current_attrs_offset as u64)?;
                current_attrs_offset += pn.num_attrs * 8;
            } else {
                data.write_u32::<LittleEndian>(0xFFFFFFFF)?;
                data.write_u32::<LittleEndian>(0)?;
                data.write_u64::<LittleEndian>(0)?;
            }

            if pn.num_children > 0 {
                data.write_u32::<LittleEndian>(pn.num_children as u32)?;
                data.write_u32::<LittleEndian>(0)?;
                data.write_u64::<LittleEndian>(current_children_offset as u64)?;
                current_children_offset += pn.num_children * 4;
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

        let mut attr_idx = 0;
        for pn in &packed_nodes {
            for _ in 0..pn.num_attrs {
                let (name_var, value_var) = all_attributes[attr_idx];
                data.write_u32::<LittleEndian>(name_var)?;
                data.write_u32::<LittleEndian>(value_var)?;
                attr_idx += 1;
            }
        }

        for pn in &packed_nodes {
            for &child_idx in &pn.children {
                data.write_u32::<LittleEndian>(child_idx)?;
            }
        }

        data.extend_from_slice(&string_table.data);
        data.extend_from_slice(&data_table.data);

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
        let mut collected_nodes: Vec<Xbox360NodeData> = Vec::new();
        let mut variant_data = VariantDataBuilder::new();

        collect_nodes_xbox360(root, 0xFFFFFFFF, &mut collected_nodes, &mut variant_data)?;

        if collected_nodes.is_empty() {
            let mut data = Vec::new();
            data.write_u32::<BigEndian>(0)?; // nodes_size
            data.write_u32::<BigEndian>(0)?; // nodes_ptr
            data.write_u32::<BigEndian>(0)?; // variant_data_size
            data.write_u32::<BigEndian>(0)?; // variant_data_ptr
            return Ok(data);
        }

        let header_size = 16u32;
        let nodes_ptr = header_size + pointer_base as u32;
        let nodes_size = collected_nodes.len() as u32;
        let nodes_array_size = nodes_size * 28;

        let mut current_offset = nodes_ptr + nodes_array_size;
        let mut node_attrs_ptrs: Vec<u32> = Vec::with_capacity(collected_nodes.len());
        let mut node_children_ptrs: Vec<u32> = Vec::with_capacity(collected_nodes.len());

        for node in &collected_nodes {
            if node.attributes.is_empty() {
                node_attrs_ptrs.push(0xFFFFFFFF);
            } else {
                node_attrs_ptrs.push(current_offset);
                current_offset += (node.attributes.len() as u32) * 8;
            }

            if node.children_indices.is_empty() {
                node_children_ptrs.push(0xFFFFFFFF);
            } else {
                node_children_ptrs.push(current_offset);
                current_offset += (node.children_indices.len() as u32) * 4;
            }
        }

        let variant_data_ptr = current_offset;

        for node in &mut collected_nodes {
            node.text_variant = variant_data.fixup_variant(node.text_variant);
            for (name_var, value_var) in &mut node.attributes {
                *name_var = variant_data.fixup_variant(*name_var);
                *value_var = variant_data.fixup_variant(*value_var);
            }
        }

        let variant_data_bytes = variant_data.finish();
        let variant_data_size = variant_data_bytes.len() as u32;

        let mut data = Vec::new();

        // Header
        data.write_u32::<BigEndian>(nodes_size)?;
        data.write_u32::<BigEndian>(nodes_ptr)?;
        data.write_u32::<BigEndian>(variant_data_size)?;
        data.write_u32::<BigEndian>(variant_data_ptr)?;

        // Nodes
        for (i, node) in collected_nodes.iter().enumerate() {
            data.write_u32::<BigEndian>(node.parent_index)?;
            data.write_u32::<BigEndian>(node.name_variant)?;
            data.write_u32::<BigEndian>(node.text_variant)?;
            data.write_u32::<BigEndian>(node.attributes.len() as u32)?;
            data.write_u32::<BigEndian>(node_attrs_ptrs[i])?;
            data.write_u32::<BigEndian>(node.children_indices.len() as u32)?;
            data.write_u32::<BigEndian>(node_children_ptrs[i])?;
        }

        // Attributes and children
        for node in &collected_nodes {
            for (name_var, value_var) in &node.attributes {
                data.write_u32::<BigEndian>(*name_var)?;
                data.write_u32::<BigEndian>(*value_var)?;
            }
            for child_idx in &node.children_indices {
                data.write_u32::<BigEndian>(*child_idx)?;
            }
        }

        data.extend_from_slice(&variant_data_bytes);

        Ok(data)
    }
}

// ============================================================================
// Little-endian writer helpers
// ============================================================================

struct PackedNodeLe {
    parent_index: u32,
    name_variant: u32,
    text_variant: u32,
    #[allow(dead_code)]
    first_attr: usize,
    num_attrs: usize,
    #[allow(dead_code)]
    first_child: usize,
    num_children: usize,
    children: Vec<u32>,
}

fn collect_nodes_le(
    node: &Node,
    parent_index: u32,
    nodes: &mut Vec<PackedNodeLe>,
    all_attributes: &mut Vec<(u32, u32)>,
    string_table: &mut StringTableLe,
    data_table: &mut DataTableLe,
) -> Result<u32> {
    let node_index = nodes.len() as u32;
    let first_attr = all_attributes.len();
    let num_attrs = node.attributes.len();
    let first_child = nodes.len() + 1;
    let num_children = node.children.len();

    nodes.push(PackedNodeLe {
        parent_index,
        name_variant: string_table.add_string(&node.name),
        text_variant: pack_variant_le(&node.text, string_table, data_table),
        first_attr,
        num_attrs,
        first_child,
        num_children,
        children: Vec::new(),
    });

    for attr in &node.attributes {
        let name_var = string_table.add_string(&attr.name);
        let value_var = pack_variant_le(&attr.value, string_table, data_table);
        all_attributes.push((name_var, value_var));
    }

    let mut child_indices = Vec::with_capacity(num_children);
    for child in &node.children {
        let child_idx = collect_nodes_le(
            child,
            node_index,
            nodes,
            all_attributes,
            string_table,
            data_table,
        )?;
        child_indices.push(child_idx);
    }

    nodes[node_index as usize].children = child_indices;

    Ok(node_index)
}

fn pack_variant_le(
    variant: &Variant,
    string_table: &mut StringTableLe,
    data_table: &mut DataTableLe,
) -> u32 {
    match variant {
        Variant::Null => 0,
        Variant::Bool(v) => {
            let value = if *v { 1u32 } else { 0u32 };
            (VariantType::Bool as u32) << 24 | value
        }
        Variant::Int(v) => {
            if *v >= -8388607 && *v <= 8388607 {
                let packed = pack_int24(*v);
                (VariantType::Int24 as u32) << 24 | packed
            } else {
                data_table.add_int32(*v)
            }
        }
        Variant::UInt(v) => {
            if *v <= 0xFFFFFF {
                ((VariantType::Int24 as u32 | UNSIGNED_FLAG as u32) << 24) | *v
            } else {
                data_table.add_int32(*v as i32)
            }
        }
        Variant::Float(v) => {
            let packed = pack_float24(*v);
            let unpacked = unpack_float24(packed);
            if (*v - unpacked).abs() < 0.001 || *v == 0.0 {
                (VariantType::Float24 as u32) << 24 | packed
            } else {
                data_table.add_float(*v)
            }
        }
        Variant::Double(v) => data_table.add_double(*v),
        Variant::String(s) => string_table.add_string(s),
        Variant::UString(s) => string_table.add_ustring(s),
        Variant::FloatVec(v) => data_table.add_float_vec(v),
    }
}

struct StringTableLe {
    data: Vec<u8>,
    strings: std::collections::HashMap<String, u32>,
}

impl StringTableLe {
    fn new() -> Self {
        Self {
            data: Vec::new(),
            strings: std::collections::HashMap::new(),
        }
    }

    fn add_string(&mut self, s: &str) -> u32 {
        if let Some(&offset) = self.strings.get(s) {
            return ((VariantType::String as u32 | OFFSET_FLAG as u32) << 24) | offset;
        }
        let offset = self.data.len() as u32;
        self.data.extend_from_slice(s.as_bytes());
        self.data.push(0);
        self.strings.insert(s.to_string(), offset);
        ((VariantType::String as u32 | OFFSET_FLAG as u32) << 24) | offset
    }

    fn add_ustring(&mut self, s: &str) -> u32 {
        let offset = self.data.len() as u32;
        for c in s.encode_utf16() {
            self.data.push((c & 0xFF) as u8);
            self.data.push((c >> 8) as u8);
        }
        self.data.push(0);
        self.data.push(0);
        ((VariantType::UString as u32 | OFFSET_FLAG as u32) << 24) | offset
    }
}

struct DataTableLe {
    data: Vec<u8>,
}

impl DataTableLe {
    fn new() -> Self {
        Self { data: Vec::new() }
    }

    fn add_double(&mut self, v: f64) -> u32 {
        let offset = self.data.len() as u32;
        self.data.extend_from_slice(&v.to_le_bytes());
        ((VariantType::Double as u32 | OFFSET_FLAG as u32) << 24) | offset
    }

    fn add_float(&mut self, v: f32) -> u32 {
        let offset = self.data.len() as u32;
        self.data.extend_from_slice(&v.to_le_bytes());
        ((VariantType::Float as u32 | OFFSET_FLAG as u32) << 24) | offset
    }

    fn add_int32(&mut self, v: i32) -> u32 {
        let offset = self.data.len() as u32;
        self.data.extend_from_slice(&v.to_le_bytes());
        ((VariantType::Int32 as u32 | OFFSET_FLAG as u32) << 24) | offset
    }

    fn add_float_vec(&mut self, v: &[f32]) -> u32 {
        let offset = self.data.len() as u32;
        for f in v {
            self.data.extend_from_slice(&f.to_le_bytes());
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

// ============================================================================
// Big-endian writer helpers
// ============================================================================

struct Xbox360NodeData {
    parent_index: u32,
    name_variant: u32,
    text_variant: u32,
    attributes: Vec<(u32, u32)>,
    children_indices: Vec<u32>,
}

fn collect_nodes_xbox360(
    node: &Node,
    parent_index: u32,
    collected: &mut Vec<Xbox360NodeData>,
    variant_data: &mut VariantDataBuilder,
) -> Result<u32> {
    let my_index = collected.len() as u32;

    let name_variant = variant_data.add_string(&node.name);
    let text_variant = pack_variant_xbox360(&node.text, variant_data);

    let mut attributes = Vec::with_capacity(node.attributes.len());
    for attr in &node.attributes {
        let attr_name = variant_data.add_string(&attr.name);
        let attr_value = pack_variant_xbox360(&attr.value, variant_data);
        attributes.push((attr_name, attr_value));
    }

    collected.push(Xbox360NodeData {
        parent_index,
        name_variant,
        text_variant,
        attributes,
        children_indices: Vec::new(),
    });

    let mut children_indices = Vec::with_capacity(node.children.len());
    for child in &node.children {
        let child_idx = collect_nodes_xbox360(child, my_index, collected, variant_data)?;
        children_indices.push(child_idx);
    }

    collected[my_index as usize].children_indices = children_indices;

    Ok(my_index)
}

fn pack_variant_xbox360(variant: &Variant, variant_data: &mut VariantDataBuilder) -> u32 {
    match variant {
        Variant::Null => 0,
        Variant::Bool(v) => ((VariantType::Bool as u32) << 24) | (if *v { 1 } else { 0 }),
        Variant::Int(v) => {
            if *v >= -8_388_608 && *v <= 8_388_607 {
                ((VariantType::Int24 as u32) << 24) | pack_int24(*v)
            } else {
                variant_data.add_int32(*v)
            }
        }
        Variant::UInt(v) => {
            if *v <= 0xFFFFFF {
                ((VariantType::Int24 as u32 | (UNSIGNED_FLAG as u32)) << 24) | pack_uint24(*v)
            } else {
                variant_data.add_int32(*v as i32)
            }
        }
        Variant::Float(v) => {
            let packed24 = pack_float24(*v);
            let unpacked = unpack_float24(packed24);
            if (unpacked - *v).abs() < 0.001 || *v == 0.0 {
                ((VariantType::Float24 as u32) << 24) | packed24
            } else {
                variant_data.add_float(*v)
            }
        }
        Variant::Double(v) => variant_data.add_double(*v),
        Variant::FloatVec(vec) => variant_data.add_float_vec(vec),
        Variant::String(s) => variant_data.add_string(s),
        Variant::UString(s) => variant_data.add_ustring(s),
    }
}

struct VariantDataBuilder {
    string_data: Vec<u8>,
    data_table: Vec<u8>,
    string_offsets: std::collections::HashMap<String, u32>,
    data_table_fixups: Vec<u32>,
}

impl VariantDataBuilder {
    fn new() -> Self {
        Self {
            string_data: Vec::new(),
            data_table: Vec::new(),
            string_offsets: std::collections::HashMap::new(),
            data_table_fixups: Vec::new(),
        }
    }

    fn add_string(&mut self, s: &str) -> u32 {
        if let Some(&offset) = self.string_offsets.get(s) {
            return ((VariantType::String as u32 | OFFSET_FLAG as u32) << 24) | offset;
        }
        let offset = self.string_data.len() as u32;
        self.string_data.extend_from_slice(s.as_bytes());
        self.string_data.push(0);
        self.string_offsets.insert(s.to_string(), offset);
        ((VariantType::String as u32 | OFFSET_FLAG as u32) << 24) | offset
    }

    fn add_ustring(&mut self, s: &str) -> u32 {
        let offset = self.string_data.len() as u32;
        for c in s.encode_utf16() {
            self.string_data.push((c >> 8) as u8);
            self.string_data.push((c & 0xFF) as u8);
        }
        self.string_data.push(0);
        self.string_data.push(0);
        ((VariantType::UString as u32 | OFFSET_FLAG as u32) << 24) | offset
    }

    fn add_float(&mut self, v: f32) -> u32 {
        let data_table_offset = self.data_table.len() as u32;
        self.data_table.extend_from_slice(&v.to_be_bytes());
        let variant = ((VariantType::Float as u32 | OFFSET_FLAG as u32) << 24) | data_table_offset;
        self.data_table_fixups.push(variant);
        variant
    }

    fn add_double(&mut self, v: f64) -> u32 {
        let data_table_offset = self.data_table.len() as u32;
        self.data_table.extend_from_slice(&v.to_be_bytes());
        let variant = ((VariantType::Double as u32 | OFFSET_FLAG as u32) << 24) | data_table_offset;
        self.data_table_fixups.push(variant);
        variant
    }

    fn add_int32(&mut self, v: i32) -> u32 {
        let data_table_offset = self.data_table.len() as u32;
        self.data_table.extend_from_slice(&v.to_be_bytes());
        let variant = ((VariantType::Int32 as u32 | OFFSET_FLAG as u32) << 24) | data_table_offset;
        self.data_table_fixups.push(variant);
        variant
    }

    fn add_float_vec(&mut self, v: &[f32]) -> u32 {
        let data_table_offset = self.data_table.len() as u32;
        for f in v {
            self.data_table.extend_from_slice(&f.to_be_bytes());
        }
        let vec_size_bits = match v.len() {
            2 => 0u32,
            3 => 1u32,
            4 => 2u32,
            _ => 0u32,
        };
        let variant = ((VariantType::FloatVec as u32 | OFFSET_FLAG as u32 | (vec_size_bits << 5))
            << 24)
            | data_table_offset;
        self.data_table_fixups.push(variant);
        variant
    }

    fn fixup_variant(&self, variant: u32) -> u32 {
        if self.data_table_fixups.contains(&variant) {
            let type_byte = variant & 0xFF000000;
            let data_offset = variant & 0x00FFFFFF;
            let fixed_offset = data_offset + (self.string_data.len() as u32);
            type_byte | fixed_offset
        } else {
            variant
        }
    }

    fn finish(self) -> Vec<u8> {
        let mut result = self.string_data;
        result.extend_from_slice(&self.data_table);
        result
    }
}
