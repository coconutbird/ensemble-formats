//! XMB binary format reader and writer.

use byteorder::{BigEndian, LittleEndian, ReadBytesExt, WriteBytesExt};
use ecf::{EcfReader, EcfWriter};
use std::io::{Cursor, Read, Seek, Write};

use crate::error::{Error, Result};
use crate::types::{Attribute, Node, XmbData, XmbFormat};
use crate::variant::{
    pack_float24, pack_int24, pack_uint24, unpack_float24, unpack_fract24, unpack_int24,
    Variant, VariantType, OFFSET_FLAG, UNSIGNED_FLAG,
};

/// XMB signature (0x71439800).
pub const XMB_SIGNATURE: u32 = 0x71439800;

/// XMB ECF file ID.
pub const XMB_ECF_FILE_ID: u32 = 0xE43ABC00;

/// XMX packed data chunk ID.
pub const XMX_PACKED_DATA_CHUNK_ID: u64 = 0xA9C96500;

/// XMX file info chunk ID.
pub const XMX_FILE_INFO_CHUNK_ID: u64 = 0xA9C96501;

/// XMB file reader.
pub struct XmbReader;

impl XmbReader {
    /// Read an XMB file from a reader.
    pub fn read<R: Read + Seek>(reader: R) -> Result<XmbData> {
        let mut ecf = EcfReader::new(reader)?;
        
        // Verify ECF file ID
        if ecf.header().id != XMB_ECF_FILE_ID {
            return Err(Error::InvalidXmbFileId {
                expected: XMB_ECF_FILE_ID,
                actual: ecf.header().id,
            });
        }

        // Find the packed data chunk
        let chunk_idx = ecf
            .chunks()
            .iter()
            .position(|c| c.id == XMX_PACKED_DATA_CHUNK_ID)
            .ok_or(Error::ChunkNotFound(XMX_PACKED_DATA_CHUNK_ID))?;

        // Read and decompress chunk data
        let packed_data = ecf.read_chunk_data(chunk_idx)?;

        // Parse the packed data
        Self::parse_packed_data(&packed_data)
    }

    /// Parse packed XMB data.
    fn parse_packed_data(data: &[u8]) -> Result<XmbData> {
        if data.len() < 4 {
            return Err(Error::UnexpectedEof);
        }

        let mut cursor = Cursor::new(data);

        // Read signature - detect endianness
        let sig_bytes = cursor.read_u32::<LittleEndian>()?;
        let is_big_endian = sig_bytes == XMB_SIGNATURE.swap_bytes();

        let signature = if is_big_endian {
            sig_bytes.swap_bytes()
        } else {
            sig_bytes
        };

        if signature != XMB_SIGNATURE {
            return Err(Error::InvalidXmbSignature {
                expected: XMB_SIGNATURE,
                actual: signature,
            });
        }

        // Detect format by checking header structure
        // Xbox 360: big-endian signature, header is 20 bytes (sig + 4 u32 fields)
        // PC: little-endian signature, header is 40 bytes (sig + padding + two BPackedArrays)
        if is_big_endian {
            // Xbox 360 format
            Self::parse_xbox360_format(data)
        } else {
            // PC format
            Self::parse_pc_format(data)
        }
    }

    /// Parse PC format (little-endian, 48-byte nodes).
    fn parse_pc_format(data: &[u8]) -> Result<XmbData> {
        let mut cursor = Cursor::new(data);
        cursor.set_position(4); // Skip signature

        // PC header: sig(4) + pad(4) + nodes_bpa(16) + variant_bpa(16) = 40 bytes
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
            return Ok(XmbData {
                root: None,
                format: XmbFormat::PC,
                source_file: None,
            });
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
        let root = Self::build_tree_le(&packed_nodes, variant_data)?;

        Ok(XmbData {
            root,
            format: XmbFormat::PC,
            source_file: None,
        })
    }

    /// Parse Xbox 360 format (big-endian, 28-byte nodes).
    fn parse_xbox360_format(data: &[u8]) -> Result<XmbData> {
        let mut cursor = Cursor::new(data);
        cursor.set_position(4); // Skip signature

        // Xbox 360 header: sig(4) + nodes_size(4) + nodes_ptr(4) + variant_size(4) + variant_ptr(4) = 20 bytes
        let nodes_size = cursor.read_u32::<BigEndian>()?;
        let nodes_ptr = cursor.read_u32::<BigEndian>()? as usize;
        let variant_data_size = cursor.read_u32::<BigEndian>()?;
        let variant_data_ptr = cursor.read_u32::<BigEndian>()? as usize;

        if nodes_size == 0 {
            return Ok(XmbData {
                root: None,
                format: XmbFormat::Xbox360,
                source_file: None,
            });
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
        let root = Self::build_tree_be(&packed_nodes, variant_data)?;

        Ok(XmbData {
            root,
            format: XmbFormat::Xbox360,
            source_file: None,
        })
    }

    /// Build tree from packed nodes (little-endian variant data).
    fn build_tree_le(packed_nodes: &[PackedNodeRead], variant_data: &[u8]) -> Result<Option<Node>> {
        if packed_nodes.is_empty() {
            return Ok(None);
        }

        // Build nodes
        let mut nodes: Vec<Node> = Vec::with_capacity(packed_nodes.len());
        
        for pn in packed_nodes {
            let name = Self::decode_variant_string(pn.name_variant, variant_data)?;
            let text = Self::decode_variant_to_variant(pn.text_variant, variant_data)?;
            
            let mut attributes = Vec::with_capacity(pn.attributes.len());
            for (name_var, value_var) in &pn.attributes {
                let attr_name = Self::decode_variant_string(*name_var, variant_data)?;
                let attr_value = Self::decode_variant_to_variant(*value_var, variant_data)?;
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

        // Build parent-child relationships
        for (i, pn) in packed_nodes.iter().enumerate() {
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
            let name = Self::decode_variant_string(pn.name_variant, variant_data)?;
            let text = Self::decode_variant_to_variant_be(pn.text_variant, variant_data)?;
            
            let mut attributes = Vec::with_capacity(pn.attributes.len());
            for (name_var, value_var) in &pn.attributes {
                let attr_name = Self::decode_variant_string(*name_var, variant_data)?;
                let attr_value = Self::decode_variant_to_variant_be(*value_var, variant_data)?;
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

        for (i, pn) in packed_nodes.iter().enumerate() {
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
                    Self::read_null_terminated_string(variant_data, offset)
                } else {
                    Self::decode_direct_string(data_bits)
                }
            }
            _ => Ok(format!("<type:{}>", variant_type)),
        }
    }

    fn decode_variant_to_variant(variant_value: u32, variant_data: &[u8]) -> Result<Variant> {
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
                    Ok(Variant::Float(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])))
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
                    Ok(Variant::Int(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])))
                } else {
                    Ok(Variant::Int(0))
                }
            }
            5 => Ok(Variant::Float(unpack_fract24(data_bits) as f32)),
            6 => {
                if is_offset && data_bits as usize + 8 <= variant_data.len() {
                    let bytes = &variant_data[data_bits as usize..data_bits as usize + 8];
                    Ok(Variant::Double(f64::from_le_bytes([
                        bytes[0], bytes[1], bytes[2], bytes[3],
                        bytes[4], bytes[5], bytes[6], bytes[7],
                    ])))
                } else {
                    Ok(Variant::Double(0.0))
                }
            }
            7 => Ok(Variant::Bool(data_bits != 0)),
            8 => {
                if is_offset {
                    Ok(Variant::String(Self::read_null_terminated_string(variant_data, data_bits as usize)?))
                } else {
                    Ok(Variant::String(Self::decode_direct_string(data_bits)?))
                }
            }
            9 => {
                if is_offset {
                    Ok(Variant::String(Self::read_null_terminated_wstring(variant_data, data_bits as usize)?))
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
                    Ok(Variant::Float(f32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])))
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
                    Ok(Variant::Int(i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])))
                } else {
                    Ok(Variant::Int(0))
                }
            }
            5 => Ok(Variant::Float(unpack_fract24(data_bits) as f32)),
            6 => {
                if is_offset && data_bits as usize + 8 <= variant_data.len() {
                    let bytes = &variant_data[data_bits as usize..data_bits as usize + 8];
                    Ok(Variant::Double(f64::from_be_bytes([
                        bytes[0], bytes[1], bytes[2], bytes[3],
                        bytes[4], bytes[5], bytes[6], bytes[7],
                    ])))
                } else {
                    Ok(Variant::Double(0.0))
                }
            }
            7 => Ok(Variant::Bool(data_bits != 0)),
            8 => {
                if is_offset {
                    Ok(Variant::String(Self::read_null_terminated_string(variant_data, data_bits as usize)?))
                } else {
                    Ok(Variant::String(Self::decode_direct_string(data_bits)?))
                }
            }
            9 => {
                if is_offset {
                    Ok(Variant::String(Self::read_null_terminated_wstring_be(variant_data, data_bits as usize)?))
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

    fn read_null_terminated_wstring(data: &[u8], offset: usize) -> Result<String> {
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

/// XMB file writer.
pub struct XmbWriter;

impl XmbWriter {
    /// Write an XMB document to a writer with the specified format.
    pub fn write<W: Write + Seek>(xmb: &XmbData, writer: W, format: XmbFormat) -> Result<()> {
        Self::write_with_options(xmb, writer, format, true)
    }

    /// Write an XMB document without compression.
    pub fn write_uncompressed<W: Write + Seek>(
        xmb: &XmbData,
        writer: W,
        format: XmbFormat,
    ) -> Result<()> {
        Self::write_with_options(xmb, writer, format, false)
    }

    /// Write an XMB document with explicit compression option.
    pub fn write_with_options<W: Write + Seek>(
        xmb: &XmbData,
        writer: W,
        format: XmbFormat,
        compress: bool,
    ) -> Result<()> {
        let packed_data = match format {
            XmbFormat::PC => Self::build_packed_data_pc(xmb)?,
            XmbFormat::Xbox360 => Self::build_packed_data_xbox360(xmb)?,
        };

        let mut ecf = EcfWriter::new(writer, XMB_ECF_FILE_ID);
        if compress {
            match format {
                XmbFormat::PC => ecf.add_chunk_compressed(XMX_PACKED_DATA_CHUNK_ID, packed_data)?,
                XmbFormat::Xbox360 => {
                    ecf.add_chunk_compressed_be(XMX_PACKED_DATA_CHUNK_ID, packed_data)?
                }
            }
        } else {
            ecf.add_chunk(XMX_PACKED_DATA_CHUNK_ID, packed_data);
        }

        ecf.finalize()?;
        Ok(())
    }

    /// Write an XMB document in its native format.
    pub fn write_native<W: Write + Seek>(xmb: &XmbData, writer: W) -> Result<()> {
        Self::write(xmb, writer, xmb.format())
    }

    /// Build the packed XMB data in Xbox 360 format.
    fn build_packed_data_xbox360(xmb: &XmbData) -> Result<Vec<u8>> {
        let mut collected_nodes: Vec<Xbox360NodeData> = Vec::new();
        let mut variant_data = VariantDataBuilder::new();

        if let Some(root) = &xmb.root {
            Self::collect_nodes_xbox360(root, 0xFFFFFFFF, &mut collected_nodes, &mut variant_data)?;
        }

        if collected_nodes.is_empty() {
            let mut data = Vec::new();
            data.write_u32::<BigEndian>(XMB_SIGNATURE)?;
            data.write_u32::<BigEndian>(0)?;
            data.write_u32::<BigEndian>(0)?;
            data.write_u32::<BigEndian>(0)?;
            data.write_u32::<BigEndian>(0)?;
            return Ok(data);
        }

        let header_size = 20u32;
        let nodes_ptr = header_size;
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

        data.write_u32::<BigEndian>(XMB_SIGNATURE)?;
        data.write_u32::<BigEndian>(nodes_size)?;
        data.write_u32::<BigEndian>(nodes_ptr)?;
        data.write_u32::<BigEndian>(variant_data_size)?;
        data.write_u32::<BigEndian>(variant_data_ptr)?;

        for (i, node) in collected_nodes.iter().enumerate() {
            data.write_u32::<BigEndian>(node.parent_index)?;
            data.write_u32::<BigEndian>(node.name_variant)?;
            data.write_u32::<BigEndian>(node.text_variant)?;
            data.write_u32::<BigEndian>(node.attributes.len() as u32)?;
            data.write_u32::<BigEndian>(node_attrs_ptrs[i])?;
            data.write_u32::<BigEndian>(node.children_indices.len() as u32)?;
            data.write_u32::<BigEndian>(node_children_ptrs[i])?;
        }

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

    fn collect_nodes_xbox360(
        node: &Node,
        parent_index: u32,
        collected: &mut Vec<Xbox360NodeData>,
        variant_data: &mut VariantDataBuilder,
    ) -> Result<u32> {
        let my_index = collected.len() as u32;

        let name_variant = variant_data.add_string(&node.name);
        let text_variant = Self::pack_variant_xbox360(&node.text, variant_data);

        let mut attributes = Vec::with_capacity(node.attributes.len());
        for attr in &node.attributes {
            let attr_name = variant_data.add_string(&attr.name);
            let attr_value = Self::pack_variant_xbox360(&attr.value, variant_data);
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
            let child_idx = Self::collect_nodes_xbox360(child, my_index, collected, variant_data)?;
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
                    ((VariantType::Int24 as u32 | (UNSIGNED_FLAG as u32)) << 24)
                        | pack_uint24(*v)
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

    /// Build the packed XMB data in PC format.
    fn build_packed_data_pc(xmb: &XmbData) -> Result<Vec<u8>> {
        let mut string_table = StringTableLe::new();
        let mut data_table = DataTableLe::new();

        let mut packed_nodes: Vec<PackedNodeLe> = Vec::new();
        let mut all_attributes: Vec<(u32, u32)> = Vec::new();

        if let Some(root) = &xmb.root {
            Self::collect_nodes_le(
                root,
                u32::MAX,
                &mut packed_nodes,
                &mut all_attributes,
                &mut string_table,
                &mut data_table,
            )?;
        }

        const HEADER_SIZE: usize = 40;
        const NODE_SIZE: usize = 48;

        let nodes_offset = HEADER_SIZE;
        let nodes_size = packed_nodes.len() * NODE_SIZE;

        let attrs_offset = nodes_offset + nodes_size;
        let mut children_offset = attrs_offset;

        for pn in &packed_nodes {
            children_offset += pn.num_attrs as usize * 8;
        }

        let mut variant_data_offset = children_offset;
        for pn in &packed_nodes {
            variant_data_offset += pn.num_children as usize * 4;
        }

        let variant_data_size = string_table.data.len() + data_table.data.len();

        let mut data = Vec::new();

        data.write_u32::<LittleEndian>(XMB_SIGNATURE)?;
        data.write_u32::<LittleEndian>(0)?;

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
            data.write_u32::<LittleEndian>(0)?;

            if pn.num_attrs > 0 {
                data.write_u32::<LittleEndian>(pn.num_attrs as u32)?;
                data.write_u32::<LittleEndian>(0)?;
                data.write_u64::<LittleEndian>(current_attrs_offset as u64)?;
                current_attrs_offset += pn.num_attrs as usize * 8;
            } else {
                data.write_u32::<LittleEndian>(0xFFFFFFFF)?;
                data.write_u32::<LittleEndian>(0)?;
                data.write_u64::<LittleEndian>(0)?;
            }

            if pn.num_children > 0 {
                data.write_u32::<LittleEndian>(pn.num_children as u32)?;
                data.write_u32::<LittleEndian>(0)?;
                data.write_u64::<LittleEndian>(current_children_offset as u64)?;
                current_children_offset += pn.num_children as usize * 4;
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
            text_variant: Self::pack_variant_le(&node.text, string_table, data_table),
            first_attr,
            num_attrs,
            first_child,
            num_children,
            children: Vec::new(),
        });

        for attr in &node.attributes {
            let name_var = string_table.add_string(&attr.name);
            let value_var = Self::pack_variant_le(&attr.value, string_table, data_table);
            all_attributes.push((name_var, value_var));
        }

        let mut child_indices = Vec::with_capacity(num_children);
        for child in &node.children {
            let child_idx = Self::collect_nodes_le(
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
}

// ============================================================================
// Helper Structures
// ============================================================================

struct Xbox360NodeData {
    parent_index: u32,
    name_variant: u32,
    text_variant: u32,
    attributes: Vec<(u32, u32)>,
    children_indices: Vec<u32>,
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
