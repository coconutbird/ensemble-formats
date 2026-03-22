//! XMB reader — parse an ECF-wrapped XMB file from a byte slice.
//!
//! The reader unwraps the ECF container, locates the packed data chunk,
//! detects endianness from the 4-byte XMB signature, and delegates to
//! [`bdt::PackedReader`] for the tree structure.
//!
//! # Example
//!
//! ```no_run
//! use xmb::Reader;
//!
//! let data = std::fs::read("example.xmb").unwrap();
//! let doc = Reader::read(&data).unwrap();
//! println!("root: {}", doc.root().unwrap().name);
//! ```

use crate::document::{Document, Format};
use crate::error::{Error, Result};
use crate::{ECF_FILE_ID, PACKED_DATA_CHUNK_ID, SIGNATURE};
use bdt::PackedReader;

/// XMB file reader.
pub struct Reader;

impl Reader {
    /// Read an XMB file from a byte slice.
    ///
    /// The slice must contain a complete ECF container with an XMB packed data
    /// chunk. Compressed chunks are transparently decompressed.
    pub fn read(data: &[u8]) -> Result<Document> {
        let ecf = ecf::Reader::new(data)?;

        if ecf.header().id != ECF_FILE_ID {
            return Err(Error::InvalidFileId {
                expected: ECF_FILE_ID,
                actual: ecf.header().id,
            });
        }

        let chunk_idx = ecf
            .chunks()
            .iter()
            .position(|c| c.id == PACKED_DATA_CHUNK_ID)
            .ok_or(Error::ChunkNotFound(PACKED_DATA_CHUNK_ID))?;

        let packed_data = ecf.chunk_data(chunk_idx)?;

        Self::parse_packed_data(&packed_data)
    }

    /// Parse packed XMB data (4-byte signature + BDT packed document).
    ///
    /// This is useful when you already have the raw packed bytes (e.g. from
    /// a custom ECF reader or an ERA archive).
    pub fn parse_packed_data(data: &[u8]) -> Result<Document> {
        if data.len() < 4 {
            return Err(Error::UnexpectedEof);
        }

        let sig_bytes = u32::from_le_bytes(data[..4].try_into().unwrap());
        let is_big_endian = sig_bytes == SIGNATURE.swap_bytes();

        let signature = if is_big_endian {
            sig_bytes.swap_bytes()
        } else {
            sig_bytes
        };

        if signature != SIGNATURE {
            return Err(Error::InvalidSignature {
                expected: SIGNATURE,
                actual: signature,
            });
        }

        // The packed document header starts after the 4-byte signature.
        // All internal pointers are absolute from data[0] (including the sig).
        if is_big_endian {
            let root = PackedReader::read_be_at(data, 4)?;
            Ok(Document {
                root,
                format: Format::Xbox360,
                source_file: None,
            })
        } else {
            let root = PackedReader::read_le_at(data, 4)?;
            Ok(Document {
                root,
                format: Format::PC,
                source_file: None,
            })
        }
    }
}
