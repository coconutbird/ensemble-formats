//! XMB reader — parse binary XMB or XML text from a byte slice.
//!
//! [`Reader::read`] auto-detects the format: if the input starts with `<`
//! (or a UTF-8 BOM followed by `<`) it is parsed as XML text; otherwise it
//! is treated as a binary ECF-wrapped XMB file.

use crate::document::{Document, Format};
use crate::error::{Error, Result};
use crate::{ECF_FILE_ID, PACKED_DATA_CHUNK_ID, SIGNATURE};
use nostdio::{Cursor, ReadLe};

/// UTF-8 BOM prefix.
const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

/// XMB file reader.
pub struct Reader;

impl Reader {
    /// Read a [`Document`] from a byte slice, auto-detecting the format.
    ///
    /// - If the data starts with `<` or a UTF-8 BOM, it is parsed as XML text.
    /// - Otherwise it is parsed as a binary ECF-wrapped XMB file.
    ///
    /// # Errors
    ///
    /// Returns an error if text input is invalid UTF-8 or XML, or if binary
    /// input has an invalid ECF/XMB header, checksum, or packed BDT document.
    pub fn read(data: &[u8]) -> Result<Document> {
        if Self::looks_like_xml(data) {
            let s = core::str::from_utf8(data)?;
            Document::from_xml(s)
        } else {
            Self::read_ecf(data)
        }
    }

    /// Read a binary ECF-wrapped XMB file from a byte slice.
    ///
    /// Use this when you know the input is a binary XMB (skips XML detection).
    ///
    /// # Errors
    ///
    /// Returns an error if the ECF container, file ID, required chunk, XMB
    /// signature, or packed BDT document is invalid.
    pub fn read_ecf(data: &[u8]) -> Result<Document> {
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
    ///
    /// # Errors
    ///
    /// Returns an error if the signature is missing or invalid or the packed
    /// BDT document is malformed.
    pub fn parse_packed_data(data: &[u8]) -> Result<Document> {
        if data.len() < 4 {
            return Err(Error::UnexpectedEof);
        }

        let sig_bytes = Cursor::new(data)
            .read_u32_le()
            .map_err(|_| Error::UnexpectedEof)?;
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
        let endian = if is_big_endian {
            bdt::Endian::Big
        } else {
            bdt::Endian::Little
        };
        let format = if is_big_endian {
            Format::Xbox360
        } else {
            Format::PC
        };

        let root = bdt::Reader::read_at(data, 4, endian)?;
        Ok(Document {
            root,
            format,
            source_file: None,
        })
    }

    /// Returns `true` if `data` looks like XML text rather than binary XMB.
    fn looks_like_xml(data: &[u8]) -> bool {
        let data = data.strip_prefix(UTF8_BOM).unwrap_or(data);
        data.iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'<')
    }
}
