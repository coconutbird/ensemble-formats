//! BDeflateStream compression format.
//!
//! BDeflateStream is EA/Ensemble's custom compression wrapper that adds
//! checksums and metadata around standard raw-deflate compression.
//!
//! ## On-Disk Header Layout (36 bytes)
//!
//! | Offset | Size | Field |
//! |--------|------|-------|
//! | 0 | 4 | signature |
//! | 4 | 4 | header_adler32 |
//! | 8 | 4 | header_type |
//! | 12 | 8 | src_bytes (uncompressed size) |
//! | 20 | 8 | dst_bytes (compressed size) |
//! | 28 | 4 | src_adler32 |
//! | 32 | 4 | dst_adler32 |
//!
//! ## In-Memory Struct Layout (after `BDeflateStream_ReadChunkHeader`)
//!
//! The game's reader (`BDeflateStream_ReadChunkHeader`) reshuffles fields
//! into a different in-memory struct layout. `BDeflateStream_ValidateHeader`
//! checksums `struct[8..36]` contiguously, so `header_adler32` must be
//! computed over the **in-memory** order, not the on-disk order:
//!
//! | Struct Offset | Size | Field |
//! |---------------|------|-------|
//! | 0 | 4 | signature |
//! | 4 | 4 | header_adler32 |
//! | 8 | 8 | src_bytes |
//! | 16 | 4 | src_adler32 |
//! | 20 | 8 | dst_bytes |
//! | 28 | 4 | dst_adler32 |
//! | 32 | 4 | header_type |
//!
//! Followed by `dst_bytes` of raw deflate data and a 4-byte end magic.

use alloc::{format, vec, vec::Vec};
use miniz_oxide::deflate::compress_to_vec;
use miniz_oxide::inflate::decompress_to_vec;

use crate::checksum::adler32;
use crate::{Error, Result};

/// BDeflateStream signature value (as a native u32).
///
/// Both HW1 and HW2 use a big-endian stream reader that byte-swaps values
/// from disk, so the on-disk bytes are always `CC 34 EE AD` (BE encoding).
/// After the reader swaps, the in-memory value equals `0xADEE34CC` on the
/// Xbox 360 or `0xCC34EEAD` on PC — but the disk representation is always BE.
pub const SIGNATURE: u32 = 0xCC34EEAD;

/// BDeflateStream signature as it appears on disk (big-endian byte order).
/// Reading these 4 bytes as a little-endian u32 gives `0xADEE34CC`.
pub const SIGNATURE_INVERTED: u32 = 0xADEE34CC;

/// BDeflateStream header size in bytes.
pub const HEADER_SIZE: usize = 36;

/// BDeflateStream end magic value.
pub const END_MAGIC: u32 = 0xA5D91776;

/// Read a u32 from `data` at `offset` with the given endianness.
fn read_u32(data: &[u8], offset: usize, big_endian: bool) -> u32 {
    let b: [u8; 4] = data[offset..offset + 4].try_into().unwrap();
    if big_endian {
        u32::from_be_bytes(b)
    } else {
        u32::from_le_bytes(b)
    }
}

/// Read a u64 from `data` at `offset` with the given endianness.
fn read_u64(data: &[u8], offset: usize, big_endian: bool) -> u64 {
    let b: [u8; 8] = data[offset..offset + 8].try_into().unwrap();
    if big_endian {
        u64::from_be_bytes(b)
    } else {
        u64::from_le_bytes(b)
    }
}

/// Decompress BDeflateStream format data.
///
/// Automatically detects endianness from the signature.
pub fn decompress(data: &[u8]) -> Result<Vec<u8>> {
    if data.len() < HEADER_SIZE {
        return Err(Error::DecompressionError(
            "BDeflateStream data too short".into(),
        ));
    }

    // Signature is always stored in its native byte order — read as LE first
    let sig = u32::from_le_bytes(data[0..4].try_into().unwrap());
    let big_endian = sig == SIGNATURE_INVERTED;

    if sig != SIGNATURE && sig != SIGNATURE_INVERTED {
        return Err(Error::InvalidDeflateStreamSignature(sig));
    }

    // Header fields (disk offsets from start of data):
    //  4: header_adler32, 8: header_type,
    // 12: src_bytes (u64), 20: dst_bytes (u64),
    // 28: src_adler32, 32: dst_adler32
    let header_adler32 = read_u32(data, 4, big_endian);
    let header_type = read_u32(data, 8, big_endian);
    let src_bytes = read_u64(data, 12, big_endian);
    let dst_bytes = read_u64(data, 20, big_endian);
    let src_adler32 = read_u32(data, 28, big_endian);
    let dst_adler32 = read_u32(data, 32, big_endian);

    // Validate header checksum using the in-memory struct order
    // (same as BDeflateStream_ValidateHeader checksums struct[8..36]):
    //   src_bytes(8) + src_adler32(4) + dst_bytes(8) + dst_adler32(4) + header_type(4)
    let mut checksum_buf = [0u8; 28];
    if big_endian {
        checksum_buf[0..8].copy_from_slice(&src_bytes.to_be_bytes());
        checksum_buf[8..12].copy_from_slice(&src_adler32.to_be_bytes());
        checksum_buf[12..20].copy_from_slice(&dst_bytes.to_be_bytes());
        checksum_buf[20..24].copy_from_slice(&dst_adler32.to_be_bytes());
        checksum_buf[24..28].copy_from_slice(&header_type.to_be_bytes());
    } else {
        checksum_buf[0..8].copy_from_slice(&src_bytes.to_le_bytes());
        checksum_buf[8..12].copy_from_slice(&src_adler32.to_le_bytes());
        checksum_buf[12..20].copy_from_slice(&dst_bytes.to_le_bytes());
        checksum_buf[20..24].copy_from_slice(&dst_adler32.to_le_bytes());
        checksum_buf[24..28].copy_from_slice(&header_type.to_le_bytes());
    }
    let computed_adler32 = adler32(&checksum_buf);
    if computed_adler32 != header_adler32 {
        return Err(Error::DecompressionError(format!(
            "BDeflateStream header checksum mismatch: expected 0x{:08X}, computed 0x{:08X}",
            header_adler32, computed_adler32
        )));
    }

    let src_bytes = src_bytes as usize;
    let dst_bytes = dst_bytes as usize;

    if data.len() < HEADER_SIZE + dst_bytes {
        return Err(Error::DecompressionError(format!(
            "BDeflateStream data too short: have {}, need {}",
            data.len(),
            HEADER_SIZE + dst_bytes
        )));
    }

    let deflate_data = &data[HEADER_SIZE..HEADER_SIZE + dst_bytes];

    let decompressed = decompress_to_vec(deflate_data)
        .map_err(|e| Error::DecompressionError(format!("deflate decompression failed: {:?}", e)))?;

    // Sanity-check decompressed size
    if decompressed.len() != src_bytes {
        return Err(Error::DecompressionError(format!(
            "decompressed size mismatch: expected {}, got {}",
            src_bytes,
            decompressed.len()
        )));
    }

    Ok(decompressed)
}

/// Compress data to BDeflateStream format.
///
/// # Arguments
/// * `data` — Uncompressed data to compress
/// * `big_endian` — If true, use big-endian format (Xbox 360); if false, little-endian (PC)
pub fn compress(data: &[u8], big_endian: bool) -> Result<Vec<u8>> {
    let src_bytes = data.len() as u64;
    let src_adler32 = adler32(data);

    // Compress using raw deflate (level 6 ≈ default)
    let compressed_data = compress_to_vec(data, 6);
    let dst_bytes = compressed_data.len() as u64;
    let dst_adler32 = adler32(&compressed_data);

    // Compute header_adler32 over the **in-memory struct order** (not disk order).
    //
    // The game's BDeflateStream_ReadChunkHeader reads fields from disk into a
    // reshuffled struct, then BDeflateStream_ValidateHeader checksums struct[8..36]:
    //   src_bytes(8) + src_adler32(4) + dst_bytes(8) + dst_adler32(4) + header_type(4)
    //
    // This differs from the on-disk order (header_type, src_bytes, dst_bytes,
    // src_adler32, dst_adler32).
    let mut checksum_buf = vec![0u8; 28];
    if big_endian {
        checksum_buf[0..8].copy_from_slice(&src_bytes.to_be_bytes());
        checksum_buf[8..12].copy_from_slice(&src_adler32.to_be_bytes());
        checksum_buf[12..20].copy_from_slice(&dst_bytes.to_be_bytes());
        checksum_buf[20..24].copy_from_slice(&dst_adler32.to_be_bytes());
        checksum_buf[24..28].copy_from_slice(&0u32.to_be_bytes()); // header_type
    } else {
        checksum_buf[0..8].copy_from_slice(&src_bytes.to_le_bytes());
        checksum_buf[8..12].copy_from_slice(&src_adler32.to_le_bytes());
        checksum_buf[12..20].copy_from_slice(&dst_bytes.to_le_bytes());
        checksum_buf[20..24].copy_from_slice(&dst_adler32.to_le_bytes());
        checksum_buf[24..28].copy_from_slice(&0u32.to_le_bytes()); // header_type
    }
    let header_adler32 = adler32(&checksum_buf);

    // Assemble on-disk layout:
    // signature(4) + header_adler32(4) + header_type(4) + src_bytes(8)
    // + dst_bytes(8) + src_adler32(4) + dst_adler32(4) + compressed + end_magic(4)
    let total_size = HEADER_SIZE + compressed_data.len() + 4;
    let mut out = Vec::with_capacity(total_size);

    if big_endian {
        out.extend_from_slice(&SIGNATURE.to_be_bytes());
        out.extend_from_slice(&header_adler32.to_be_bytes());
        out.extend_from_slice(&0u32.to_be_bytes()); // header_type
        out.extend_from_slice(&src_bytes.to_be_bytes());
        out.extend_from_slice(&dst_bytes.to_be_bytes());
        out.extend_from_slice(&src_adler32.to_be_bytes());
        out.extend_from_slice(&dst_adler32.to_be_bytes());
    } else {
        out.extend_from_slice(&SIGNATURE.to_le_bytes());
        out.extend_from_slice(&header_adler32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // header_type
        out.extend_from_slice(&src_bytes.to_le_bytes());
        out.extend_from_slice(&dst_bytes.to_le_bytes());
        out.extend_from_slice(&src_adler32.to_le_bytes());
        out.extend_from_slice(&dst_adler32.to_le_bytes());
    }
    out.extend_from_slice(&compressed_data);
    if big_endian {
        out.extend_from_slice(&END_MAGIC.to_be_bytes());
    } else {
        out.extend_from_slice(&END_MAGIC.to_le_bytes());
    }

    Ok(out)
}
