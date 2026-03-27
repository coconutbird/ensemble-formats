//! Compact BPackedHeader format writer.
//!
//! Serialises a [`Node`] tree into the compact BDT format (signature `0x3E`),
//! which is the native `BBinaryDataTree` on-disk representation used by
//! material chunks and other non-XMB packed data.
//!
//! Layout produced:
//! ```text
//! [BPackedHeader (28 bytes)]
//! [Node section (8 bytes per node)]
//! [NameValue section (8 bytes per entry)]
//! [NameData section (null-terminated strings)]
//! [Padding to 16-byte boundary]
//! [ValueData section]
//! ```

use alloc::string::String;
use alloc::vec::Vec;
use hashbrown::HashMap;

use crate::error::Result;
use crate::node::Node;
use crate::variant::Variant;

/// Compact format writer producing the `0x3E` BPackedHeader binary layout.
pub struct CompactWriter;

impl CompactWriter {
    /// Write a node tree in compact format (little-endian).
    pub fn write(root: &Node) -> Result<Vec<u8>> {
        Self::write_endian(root, false)
    }

    /// Write a node tree in compact format with explicit endianness.
    pub fn write_endian(root: &Node, big_endian: bool) -> Result<Vec<u8>> {
        let mut ctx = CompactCtx::new(big_endian);
        ctx.collect(root);
        ctx.build()
    }
}

/// Intermediate node during collection.
struct CNode {
    parent_index: u16,
    first_child: u16,
    num_children: u8,
    nv_offset: u16,
    num_nvs: u8,
}

/// Name-value entry to be written.
struct CNv {
    name_offset: u16,
    flags: u16,
    value: u32,
}

struct CompactCtx {
    big_endian: bool,
    nodes: Vec<CNode>,
    nvs: Vec<CNv>,
    name_data: Vec<u8>,
    name_offsets: HashMap<String, u16>,
    value_data: Vec<u8>,
}

impl CompactCtx {
    fn new(big_endian: bool) -> Self {
        Self {
            big_endian,
            nodes: Vec::new(),
            nvs: Vec::new(),
            name_data: Vec::new(),
            name_offsets: HashMap::new(),
            value_data: Vec::new(),
        }
    }

    /// Add a name string, deduplicating.
    fn add_name(&mut self, name: &str) -> u16 {
        if let Some(&ofs) = self.name_offsets.get(name) {
            return ofs;
        }
        let ofs = self.name_data.len() as u16;
        self.name_data.extend_from_slice(name.as_bytes());
        self.name_data.push(0);
        self.name_offsets.insert(name.into(), ofs);
        ofs
    }

    /// Encode a Variant into compact NV flags + value.
    fn encode_variant(&mut self, variant: &Variant) -> (u16, u32) {
        // nv_flags bit layout:
        // bit 0: TYPE_IS_UNSIGNED
        // bit 1: DIRECT_ENCODING
        // bits 2-4: type class (0=Null,1=Bool,2=Int,3=Float,4=String)
        // bits 5-7: type_size_log2
        // bit 8: LAST_NAME_VALUE (set later)
        // bits 9-15: size (for strings)
        match variant {
            Variant::Null => (0x0002, 0), // Null, direct
            Variant::Bool(v) => {
                let flags: u16 = (1 << 2) | 0x0002; // Bool + direct
                let value = if *v { 1u32 } else { 0u32 };
                (flags, value)
            }
            Variant::Int(v) => {
                // Direct encoding: value fits in u32
                let flags: u16 = (2 << 2) | 0x0002 | (2 << 5); // Int + direct + size_log2=2
                (flags, *v as u32)
            }
            Variant::UInt(v) => {
                let flags: u16 = (2 << 2) | 0x0002 | 0x0001 | (2 << 5); // Int + direct + unsigned + size_log2=2
                (flags, *v)
            }
            Variant::Float(v) => {
                // Use direct encoding for all f32 values — the bits fit in u32
                let flags: u16 = (3 << 2) | 0x0002 | (2 << 5); // Float + direct + size_log2=2
                (flags, v.to_bits())
            }
            Variant::Double(v) => {
                // Indirect: store in value_data
                let offset = self.value_data.len() as u32;
                if self.big_endian {
                    self.value_data.extend_from_slice(&v.to_be_bytes());
                } else {
                    self.value_data.extend_from_slice(&v.to_le_bytes());
                }
                let flags: u16 = (3 << 2) | (3 << 5); // Float type + size_log2=3 (8 bytes), NOT direct
                (flags, offset)
            }
            Variant::String(s) => self.encode_string_value(s),
            _ => (0x0002, 0), // Fallback to null for unsupported types
        }
    }

    fn encode_string_value(&mut self, s: &str) -> (u16, u32) {
        let bytes = s.as_bytes();
        let len = bytes.len();

        if len <= 4 {
            // Direct encoding: pack bytes into value u32
            let mut value = 0u32;
            for (i, &b) in bytes.iter().enumerate() {
                value |= (b as u32) << (i * 8);
            }
            let size_field = (len as u16) << 9;
            let flags: u16 = (4 << 2) | 0x0002 | size_field; // String + direct + size
            (flags, value)
        } else {
            // Indirect: store in value_data
            let offset = self.value_data.len() as u32;
            self.value_data.extend_from_slice(bytes);
            self.value_data.push(0);
            let size = if len < 127 { len as u16 } else { 127u16 };
            let size_field = size << 9;
            let flags: u16 = (4 << 2) | size_field; // String, NOT direct
            (flags, offset)
        }
    }

    /// Collect nodes in BFS order so each node's children are contiguous.
    ///
    /// The compact format requires that for any node with `first_child=F`
    /// and `num_children=N`, the child nodes occupy indices `F..F+N`
    /// consecutively in the node array.
    fn collect(&mut self, root: &Node) {
        use alloc::collections::VecDeque;

        // BFS queue: (source Node ref, parent index in self.nodes)
        let mut queue: VecDeque<(&Node, u16)> = VecDeque::new();
        queue.push_back((root, 0xFFFF));

        while let Some((node, parent)) = queue.pop_front() {
            let node_idx = self.nodes.len() as u16;

            // Build name-values for this node
            let nv_start = self.nvs.len() as u16;
            let name_ofs = self.add_name(&node.name);
            let (text_flags, text_value) = self.encode_variant(&node.text);
            self.nvs.push(CNv {
                name_offset: name_ofs,
                flags: text_flags,
                value: text_value,
            });

            for attr in &node.attributes {
                let attr_name_ofs = self.add_name(&attr.name);
                let (attr_flags, attr_value) = self.encode_variant(&attr.value);
                self.nvs.push(CNv {
                    name_offset: attr_name_ofs,
                    flags: attr_flags,
                    value: attr_value,
                });
            }

            let num_nvs = (self.nvs.len() - nv_start as usize) as u8;
            if num_nvs > 0 {
                let last_idx = self.nvs.len() - 1;
                self.nvs[last_idx].flags |= 0x0100; // cLastNameValueMask
            }

            // We don't know first_child yet — it will be filled when we
            // process this node's children below. Record a placeholder.
            self.nodes.push(CNode {
                parent_index: parent,
                first_child: 0,
                num_children: node.children.len() as u8,
                nv_offset: nv_start,
                num_nvs,
            });

            // Enqueue children; they'll be placed contiguously when dequeued.
            // But we need to know their indices *now* to set first_child.
            // We can't — they haven't been pushed yet. Instead, we do a
            // two-pass: first push all direct children, then recurse.
            // Actually with BFS, we enqueue children and they'll be popped
            // in order after all siblings at this level.
            for child in &node.children {
                queue.push_back((child, node_idx));
            }
        }

        // Fix up first_child: in BFS order, children of node i are the
        // contiguous nodes that have parent_index == i.
        // We scan and find the first occurrence for each parent.
        for i in 0..self.nodes.len() {
            let num_children = self.nodes[i].num_children;
            if num_children == 0 {
                continue;
            }
            // Find first node with parent_index == i
            let first = self.nodes.iter().position(|n| n.parent_index == i as u16);
            if let Some(f) = first {
                self.nodes[i].first_child = f as u16;
            }
        }
    }

    /// Serialize collected data into the compact binary format.
    fn build(&self) -> Result<Vec<u8>> {
        const HEADER_SIZE: usize = 28;

        let node_section_size = self.nodes.len() * 8;
        let nv_section_size = self.nvs.len() * 8;
        let name_data_size = self.name_data.len();
        let value_data_size = self.value_data.len();

        // The reader computes value_data_offset as 16-byte aligned from
        // (header_offset + 28 + nodes + nvs + names). We need matching padding.
        let value_data_offset_unaligned =
            HEADER_SIZE + node_section_size + nv_section_size + name_data_size;
        let padding = if value_data_size > 0 {
            (16 - (value_data_offset_unaligned % 16)) % 16
        } else {
            0
        };
        let data_size =
            node_section_size + nv_section_size + name_data_size + padding + value_data_size;

        let total_size = 28 + data_size; // header + data
        let mut buf = Vec::with_capacity(total_size);

        // BPackedHeader (28 bytes)
        // Byte 0: signature, Byte 1: version (0x07), Byte 2: header checksum (filled below),
        // Byte 3: num_user_sections (0)
        let sig = if self.big_endian { 0xE3u8 } else { 0x3Eu8 };
        buf.push(sig);
        buf.push(0x07); // version — must be 0x07 for engine compatibility
        buf.push(0); // header checksum placeholder (computed after header is complete)
        buf.push(0); // num_user_sections

        // CRC-32 placeholder (filled after data section is written)
        self.write_u32(&mut buf, 0);
        // data_size
        self.write_u32(&mut buf, data_size as u32);
        // section sizes
        self.write_u32(&mut buf, node_section_size as u32);
        self.write_u32(&mut buf, nv_section_size as u32);
        self.write_u32(&mut buf, name_data_size as u32);
        self.write_u32(&mut buf, value_data_size as u32);

        // Node section
        for node in &self.nodes {
            self.write_u16(&mut buf, node.parent_index);
            self.write_u16(&mut buf, node.first_child);
            self.write_u16(&mut buf, node.nv_offset);
            buf.push(node.num_nvs);
            buf.push(node.num_children);
        }

        // NameValue section
        for nv in &self.nvs {
            self.write_u32(&mut buf, nv.value);
            self.write_u16(&mut buf, nv.name_offset);
            self.write_u16(&mut buf, nv.flags);
        }

        // NameData section
        buf.extend_from_slice(&self.name_data);

        // Padding to 16-byte boundary
        buf.extend(core::iter::repeat_n(0u8, padding));

        // ValueData section
        buf.extend_from_slice(&self.value_data);

        // Compute and fill CRC-32 over data section (bytes 28..)
        let data_crc = crate::checksum::crc32(&buf[HEADER_SIZE..]);
        let crc_bytes = if self.big_endian {
            data_crc.to_be_bytes()
        } else {
            data_crc.to_le_bytes()
        };
        buf[4..8].copy_from_slice(&crc_bytes);

        // Compute and fill header checksum (CRC-16/CCITT over 28-byte header with byte 2 zeroed)
        let saved_byte2 = buf[2];
        buf[2] = 0;
        let hdr_crc = crate::checksum::crc16_ccitt(&buf[..HEADER_SIZE]);
        buf[2] = saved_byte2;
        // Store low byte of CRC-16 result as the checksum byte
        buf[2] = hdr_crc as u8;

        Ok(buf)
    }

    fn write_u32(&self, buf: &mut Vec<u8>, v: u32) {
        if self.big_endian {
            buf.extend_from_slice(&v.to_be_bytes());
        } else {
            buf.extend_from_slice(&v.to_le_bytes());
        }
    }

    fn write_u16(&self, buf: &mut Vec<u8>, v: u16) {
        if self.big_endian {
            buf.extend_from_slice(&v.to_be_bytes());
        } else {
            buf.extend_from_slice(&v.to_le_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use std::println;

    use super::*;
    use crate::{Attribute, Endian, Reader, Variant};

    #[test]
    fn compact_roundtrip_simple() {
        let mut root = Node::new("Root");
        let mut child = Node::new("Child");
        child
            .attributes
            .push(Attribute::with_string("Name", "hello"));
        child.text = Variant::Float(1.0);
        root.children.push(child);

        let data = CompactWriter::write(&root).unwrap();
        println!("Written {} bytes", data.len());
        println!("Hex: {:02X?}", &data);

        let parsed = Reader::read(&data, Endian::Little).unwrap();
        let node = parsed.expect("should parse");
        println!("Root: '{}', children: {}", node.name, node.children.len());
        assert_eq!(node.name, "Root");
        assert_eq!(node.children.len(), 1);

        let c = &node.children[0];
        println!(
            "Child: '{}', text: {:?}, attrs: {}",
            c.name,
            c.text,
            c.attributes.len()
        );
        assert_eq!(c.name, "Child");
        assert_eq!(c.attributes.len(), 1);
        assert_eq!(c.attributes[0].name, "Name");

        if let Variant::String(s) = &c.attributes[0].value {
            assert_eq!(s, "hello");
        } else {
            panic!("Expected string, got {:?}", c.attributes[0].value);
        }
    }

    #[test]
    fn compact_roundtrip_deep_tree() {
        // Mimic the material tree structure
        let mut root = Node::new("Materials");

        let mut mat = Node::new("Material");
        mat.attributes
            .push(Attribute::with_string("Name", "terrain_grass"));
        mat.attributes.push(Attribute::new("Ver", Variant::Int(4)));

        let mut nv = Node::new("NameValues");
        let mut sp = Node::new("SpecPower");
        sp.text = Variant::Float(25.0);
        nv.children.push(sp);
        mat.children.push(nv);

        let mut maps = Node::new("Maps");
        let mut diff = Node::new("diffuse");
        diff.attributes
            .push(Attribute::new("UVWVel", Variant::Float(0.0)));
        let mut map_node = Node::new("Map");
        map_node.attributes.push(Attribute::with_string(
            "Name",
            "art/textures/grass_diff.ddx",
        ));
        map_node
            .attributes
            .push(Attribute::new("Channel", Variant::Int(0)));
        map_node
            .attributes
            .push(Attribute::new("Flags", Variant::UInt(7)));
        diff.children.push(map_node);
        maps.children.push(diff);
        mat.children.push(maps);

        root.children.push(mat);

        let data = CompactWriter::write(&root).unwrap();
        let parsed = Reader::read(&data, Endian::Little).unwrap();
        let node = parsed.expect("should parse");

        assert_eq!(node.name, "Materials");
        assert_eq!(node.children.len(), 1);

        let m = &node.children[0];
        assert_eq!(m.name, "Material");
        println!("Material attrs: {:?}", m.attributes);
        assert_eq!(m.attributes.len(), 2);
        assert_eq!(m.attributes[0].name, "Name");

        println!("Material children:");

        for c in &m.children {
            println!(
                "  '{}' attrs:{} children:{}",
                c.name,
                c.attributes.len(),
                c.children.len()
            );
            for cc in &c.children {
                println!(
                    "    '{}' attrs:{} children:{}",
                    cc.name,
                    cc.attributes.len(),
                    cc.children.len()
                );
                for ccc in &cc.children {
                    println!(
                        "      '{}' attrs:{} children:{}",
                        ccc.name,
                        ccc.attributes.len(),
                        ccc.children.len()
                    );
                }
            }
        }

        let maps_node = m.children.iter().find(|c| c.name == "Maps").unwrap();
        let diff_node = maps_node
            .children
            .iter()
            .find(|c| c.name == "diffuse")
            .unwrap();
        assert_eq!(diff_node.children.len(), 1);

        let map_entry = &diff_node.children[0];
        assert_eq!(map_entry.name, "Map");
        let map_name = map_entry.get_attribute("Name").unwrap();
        if let Variant::String(s) = &map_name.value {
            assert_eq!(s, "art/textures/grass_diff.ddx");
        } else {
            panic!("Expected string for Map Name");
        }
    }
}
