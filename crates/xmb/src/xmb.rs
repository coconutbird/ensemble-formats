//! XMB binary format reader and writer.
//!
//! XMB wraps the BBinaryDataTree packed document format with an ECF container
//! and a 4-byte signature prefix (0x71439800).

use byteorder::{BigEndian, LittleEndian, ReadBytesExt, WriteBytesExt};
use ecf::{EcfReader, EcfWriter};
use std::io::Cursor;

use bdt::{PackedReader, PackedWriter};

use crate::error::{Error, Result};
use crate::types::{XmbData, XmbFormat};

/// XMB signature (0x71439800).
pub const XMB_SIGNATURE: u32 = 0x71439800;

/// XMB ECF file ID.
pub const XMB_ECF_FILE_ID: u32 = 0xE43ABC00;

/// XMX packed data chunk ID.
pub const XMX_PACKED_DATA_CHUNK_ID: u64 = 0xA9C96500;

/// XMX file info chunk ID.
#[allow(dead_code)]
pub const XMX_FILE_INFO_CHUNK_ID: u64 = 0xA9C96501;

/// XMB file reader.
pub struct XmbReader;

impl XmbReader {
    /// Read an XMB file from a byte slice.
    pub fn read(data: &[u8]) -> Result<XmbData> {
        let ecf = EcfReader::new(data)?;

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
        let packed_data = ecf.chunk_data(chunk_idx)?;

        // Parse the packed data
        Self::parse_packed_data(&packed_data)
    }

    /// Parse packed XMB data (signature + BBinaryDataTree packed document).
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

        // The packed document header starts after the 4-byte signature.
        // All internal pointers are absolute from data[0] (including the sig).
        if is_big_endian {
            let root = PackedReader::read_be_at(data, 4)?;
            Ok(XmbData {
                root,
                format: XmbFormat::Xbox360,
                source_file: None,
            })
        } else {
            let root = PackedReader::read_le_at(data, 4)?;
            Ok(XmbData {
                root,
                format: XmbFormat::PC,
                source_file: None,
            })
        }
    }
}

/// XMB file writer.
pub struct XmbWriter;

impl XmbWriter {
    /// Write an XMB document to bytes with the specified format.
    pub fn write(xmb: &XmbData, format: XmbFormat) -> Result<Vec<u8>> {
        Self::write_with_options(xmb, format, true)
    }

    /// Write an XMB document without compression.
    pub fn write_uncompressed(xmb: &XmbData, format: XmbFormat) -> Result<Vec<u8>> {
        Self::write_with_options(xmb, format, false)
    }

    /// Write an XMB document with explicit compression option.
    pub fn write_with_options(xmb: &XmbData, format: XmbFormat, compress: bool) -> Result<Vec<u8>> {
        let packed_data = match format {
            XmbFormat::PC => Self::build_packed_data_pc(xmb)?,
            XmbFormat::Xbox360 => Self::build_packed_data_xbox360(xmb)?,
        };

        let mut ecf = EcfWriter::new(XMB_ECF_FILE_ID);
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

        Ok(ecf.finalize()?)
    }

    /// Write an XMB document in its native format.
    pub fn write_native(xmb: &XmbData) -> Result<Vec<u8>> {
        Self::write(xmb, xmb.format())
    }

    /// Build the packed XMB data in PC format.
    ///
    /// Output: [signature(4)] + [bdt packed data with pointers offset by 4]
    fn build_packed_data_pc(xmb: &XmbData) -> Result<Vec<u8>> {
        if let Some(root) = &xmb.root {
            // Build packed data with pointer base of 4 (for the signature prefix)
            let bdt_data = PackedWriter::write_le_with_base(root, 4)?;

            // Prepend signature
            let mut data = Vec::with_capacity(4 + bdt_data.len());
            data.write_u32::<LittleEndian>(XMB_SIGNATURE)?;
            data.extend_from_slice(&bdt_data);
            Ok(data)
        } else {
            // Empty document
            let mut data = Vec::new();
            data.write_u32::<LittleEndian>(XMB_SIGNATURE)?;
            data.write_u32::<LittleEndian>(0)?; // padding
            // Nodes BPackedArray (empty)
            data.write_u32::<LittleEndian>(0xFFFFFFFF)?;
            data.write_u32::<LittleEndian>(0)?;
            data.write_u64::<LittleEndian>(0)?;
            // Variant BPackedArray (empty)
            data.write_u32::<LittleEndian>(0)?;
            data.write_u32::<LittleEndian>(0)?;
            data.write_u64::<LittleEndian>(0)?;
            Ok(data)
        }
    }

    /// Build the packed XMB data in Xbox 360 format.
    ///
    /// Output: [signature(4)] + [bdt packed data with pointers offset by 4]
    fn build_packed_data_xbox360(xmb: &XmbData) -> Result<Vec<u8>> {
        if let Some(root) = &xmb.root {
            // Build packed data with pointer base of 4 (for the signature prefix)
            let bdt_data = PackedWriter::write_be_with_base(root, 4)?;

            // Prepend signature
            let mut data = Vec::with_capacity(4 + bdt_data.len());
            data.write_u32::<BigEndian>(XMB_SIGNATURE)?;
            data.extend_from_slice(&bdt_data);
            Ok(data)
        } else {
            let mut data = Vec::new();
            data.write_u32::<BigEndian>(XMB_SIGNATURE)?;
            data.write_u32::<BigEndian>(0)?; // nodes_size
            data.write_u32::<BigEndian>(0)?; // nodes_ptr
            data.write_u32::<BigEndian>(0)?; // variant_data_size
            data.write_u32::<BigEndian>(0)?; // variant_data_ptr
            Ok(data)
        }
    }
}
