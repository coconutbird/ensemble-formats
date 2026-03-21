//! BDeflateStream compression format.
//!
//! BDeflateStream is EA's custom compression wrapper format that adds
//! checksums and metadata around standard deflate compression.
//!
//! ## Header Layout (36 bytes)
//!
//! **IMPORTANT**: The serialization order differs from the struct memory layout!
//!
//! Serialization order (file format):
//! 1. signature (4 bytes)
//! 2. header_adler32 (4 bytes)
//! 3. header_type (4 bytes)
//! 4. src_bytes (8 bytes) - uncompressed size
//! 5. dst_bytes (8 bytes) - compressed size
//! 6. src_adler32 (4 bytes)
//! 7. dst_adler32 (4 bytes)
//!
//! Followed by:
//! - dst_bytes of raw deflate compressed data
//! - end magic (4 bytes)

use byteorder::{BigEndian, LittleEndian, ReadBytesExt};
use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use std::io::{Cursor, Read, Write};

use crate::checksum::adler32;
use crate::{Error, Result};

/// BDeflateStream signature for little-endian format (PC).
pub const SIGNATURE: u32 = 0xCC34EEAD;

/// BDeflateStream inverted signature for big-endian format (Xbox 360).
pub const SIGNATURE_INVERTED: u32 = 0xADEE34CC;

/// BDeflateStream header size in bytes.
pub const HEADER_SIZE: usize = 36;

/// BDeflateStream end magic value.
pub const END_MAGIC: u32 = 0xA5D91776;

/// Decompress BDeflateStream format data.
///
/// Automatically detects endianness from the signature.
pub fn decompress_bdeflate_stream(data: &[u8]) -> Result<Vec<u8>> {
    if data.len() < HEADER_SIZE {
        return Err(Error::DecompressionError(
            "BDeflateStream data too short".to_string(),
        ));
    }

    let mut cursor = Cursor::new(data);

    // Read signature to determine endianness
    let sig = cursor.read_u32::<LittleEndian>()?;
    let is_big_endian = sig == SIGNATURE_INVERTED;

    if sig != SIGNATURE && sig != SIGNATURE_INVERTED {
        return Err(Error::InvalidDeflateStreamSignature(sig));
    }

    // Read header fields in SERIALIZATION order (not struct layout!)
    // Order: sig, adler32, header_type, src_bytes, dst_bytes, src_adler32, dst_adler32
    let (src_bytes, dst_bytes) = if is_big_endian {
        let _header_adler32 = cursor.read_u32::<BigEndian>()?;
        let _header_type = cursor.read_u32::<BigEndian>()?;
        let src_bytes = cursor.read_u64::<BigEndian>()? as usize;
        let dst_bytes = cursor.read_u64::<BigEndian>()? as usize;
        let _src_adler32 = cursor.read_u32::<BigEndian>()?;
        let _dst_adler32 = cursor.read_u32::<BigEndian>()?;
        (src_bytes, dst_bytes)
    } else {
        let _header_adler32 = cursor.read_u32::<LittleEndian>()?;
        let _header_type = cursor.read_u32::<LittleEndian>()?;
        let src_bytes = cursor.read_u64::<LittleEndian>()? as usize;
        let dst_bytes = cursor.read_u64::<LittleEndian>()? as usize;
        let _src_adler32 = cursor.read_u32::<LittleEndian>()?;
        let _dst_adler32 = cursor.read_u32::<LittleEndian>()?;
        (src_bytes, dst_bytes)
    };

    // Verify we have enough data
    if data.len() < HEADER_SIZE + dst_bytes {
        return Err(Error::DecompressionError(format!(
            "BDeflateStream data too short: have {}, need {}",
            data.len(),
            HEADER_SIZE + dst_bytes
        )));
    }

    let deflate_data = &data[HEADER_SIZE..HEADER_SIZE + dst_bytes];

    let mut decoder = DeflateDecoder::new(deflate_data);
    let mut decompressed = Vec::with_capacity(src_bytes);
    decoder
        .read_to_end(&mut decompressed)
        .map_err(|e| Error::DecompressionError(format!("deflate decompression failed: {}", e)))?;

    Ok(decompressed)
}

/// Compress data to BDeflateStream format.
///
/// # Arguments
/// * `data` - Uncompressed data to compress
/// * `big_endian` - If true, use big-endian format (Xbox 360); if false, use little-endian (PC)
pub fn compress_bdeflate_stream(data: &[u8], big_endian: bool) -> Result<Vec<u8>> {
    let src_bytes = data.len() as u64;
    let src_adler32 = adler32(data);

    // Compress the data using deflate
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data)?;
    let compressed_data = encoder.finish()?;
    let dst_bytes = compressed_data.len() as u64;
    let dst_adler32 = adler32(&compressed_data);

    // Build header bytes 8-36 first to compute header_adler32
    // SERIALIZATION order: header_type, src_bytes, dst_bytes, src_adler32, dst_adler32
    let mut header_data = Vec::with_capacity(28);
    if big_endian {
        header_data.extend_from_slice(&0u32.to_be_bytes()); // header_type
        header_data.extend_from_slice(&src_bytes.to_be_bytes());
        header_data.extend_from_slice(&dst_bytes.to_be_bytes());
        header_data.extend_from_slice(&src_adler32.to_be_bytes());
        header_data.extend_from_slice(&dst_adler32.to_be_bytes());
    } else {
        header_data.extend_from_slice(&0u32.to_le_bytes()); // header_type
        header_data.extend_from_slice(&src_bytes.to_le_bytes());
        header_data.extend_from_slice(&dst_bytes.to_le_bytes());
        header_data.extend_from_slice(&src_adler32.to_le_bytes());
        header_data.extend_from_slice(&dst_adler32.to_le_bytes());
    }
    let header_adler32 = adler32(&header_data);

    let total_size = HEADER_SIZE + compressed_data.len() + 4;
    let mut wrapped_data = Vec::with_capacity(total_size);

    if big_endian {
        wrapped_data.extend_from_slice(&SIGNATURE.to_be_bytes());
        wrapped_data.extend_from_slice(&header_adler32.to_be_bytes());
    } else {
        wrapped_data.extend_from_slice(&SIGNATURE.to_le_bytes());
        wrapped_data.extend_from_slice(&header_adler32.to_le_bytes());
    }
    wrapped_data.extend_from_slice(&header_data);
    wrapped_data.extend_from_slice(&compressed_data);
    if big_endian {
        wrapped_data.extend_from_slice(&END_MAGIC.to_be_bytes());
    } else {
        wrapped_data.extend_from_slice(&END_MAGIC.to_le_bytes());
    }

    Ok(wrapped_data)
}
