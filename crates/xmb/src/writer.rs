//! XMB writer — serialize a [`Document`] into an ECF-wrapped byte stream.
//!
//! The writer prepends the 4-byte XMB signature to the BDT packed data,
//! wraps it in an ECF chunk (optionally compressed), and returns the final
//! byte vector.
//!
//! # Example
//!
//! ```
//! use xmb::{Writer, Document, Format, Node};
//!
//! let doc = Document::with_root(Node::with_text("greeting", "hello"));
//! let bytes = Writer::write(&doc, Format::PC).unwrap();
//! assert!(!bytes.is_empty());
//! ```

use alloc::vec::Vec;

use crate::document::{Document, Format};
use crate::error::Result;
use crate::{ECF_FILE_ID, PACKED_DATA_CHUNK_ID, SIGNATURE};

/// XMB file writer.
pub struct Writer;

impl Writer {
    /// Write an XMB document to bytes (compressed by default).
    pub fn write(doc: &Document, format: Format) -> Result<Vec<u8>> {
        Self::write_with_options(doc, format, true)
    }

    /// Write an XMB document without compression.
    pub fn write_uncompressed(doc: &Document, format: Format) -> Result<Vec<u8>> {
        Self::write_with_options(doc, format, false)
    }

    /// Write an XMB document with explicit compression option.
    pub fn write_with_options(doc: &Document, format: Format, compress: bool) -> Result<Vec<u8>> {
        let packed_data = Self::build_packed_data(doc, format)?;

        let mut ecf = ecf::Writer::new(ECF_FILE_ID);
        if compress {
            match format {
                Format::PC => ecf.add_chunk_compressed(PACKED_DATA_CHUNK_ID, packed_data)?,
                Format::Xbox360 => {
                    ecf.add_chunk_compressed_be(PACKED_DATA_CHUNK_ID, packed_data)?
                }
            }
        } else {
            ecf.add_chunk(PACKED_DATA_CHUNK_ID, packed_data);
        }

        Ok(ecf.finalize()?)
    }

    /// Write an XMB document in its native format (compressed).
    pub fn write_native(doc: &Document) -> Result<Vec<u8>> {
        Self::write(doc, doc.format())
    }

    /// Build packed data for the given format.
    ///
    /// Layout: `[signature (4)] [bdt packed data with base offset 4]`
    fn build_packed_data(doc: &Document, format: Format) -> Result<Vec<u8>> {
        let endian = match format {
            Format::PC => bdt::Endian::Little,
            Format::Xbox360 => bdt::Endian::Big,
        };

        if let Some(root) = &doc.root {
            let bdt_data = bdt::Writer::write_with_base(root, 4, endian)?;

            let mut data = Vec::with_capacity(4 + bdt_data.len());
            let sig_bytes = match format {
                Format::PC => SIGNATURE.to_le_bytes(),
                Format::Xbox360 => SIGNATURE.to_be_bytes(),
            };
            data.extend_from_slice(&sig_bytes);
            data.extend_from_slice(&bdt_data);
            Ok(data)
        } else {
            let mut data = Vec::new();
            match format {
                Format::PC => {
                    data.extend_from_slice(&SIGNATURE.to_le_bytes());
                    data.extend_from_slice(&0u32.to_le_bytes()); // padding
                    // Nodes BPackedArray (empty)
                    data.extend_from_slice(&0xFFFFFFFFu32.to_le_bytes());
                    data.extend_from_slice(&0u32.to_le_bytes());
                    data.extend_from_slice(&0u64.to_le_bytes());
                    // Variant BPackedArray (empty)
                    data.extend_from_slice(&0u32.to_le_bytes());
                    data.extend_from_slice(&0u32.to_le_bytes());
                    data.extend_from_slice(&0u64.to_le_bytes());
                }
                Format::Xbox360 => {
                    data.extend_from_slice(&SIGNATURE.to_be_bytes());
                    data.extend_from_slice(&0u32.to_be_bytes()); // nodes_size
                    data.extend_from_slice(&0u32.to_be_bytes()); // nodes_ptr
                    data.extend_from_slice(&0u32.to_be_bytes()); // variant_data_size
                    data.extend_from_slice(&0u32.to_be_bytes()); // variant_data_ptr
                }
            }
            Ok(data)
        }
    }
}
