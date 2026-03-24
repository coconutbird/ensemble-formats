//! Shared little-endian byte-reading helpers.
//!
//! Cursor-based readers that advance a `&mut usize` position through a `&[u8]`
//! slice. All functions are `no_std` compatible — no file I/O, no allocations.

use alloc::string::String;

use crate::error::{Error, Result};

/// Read a little-endian `u16` from `data` at `*pos`, advancing `*pos` by 2.
#[inline]
pub(crate) fn read_u16_le(data: &[u8], pos: &mut usize) -> Result<u16> {
    let end = *pos + 2;
    if end > data.len() {
        return Err(Error::UnexpectedEof {
            context: String::from("u16"),
        });
    }
    let v = u16::from_le_bytes([data[*pos], data[*pos + 1]]);
    *pos = end;
    Ok(v)
}

/// Read a little-endian `i16` from `data` at `*pos`, advancing `*pos` by 2.
#[inline]
pub(crate) fn read_i16_le(data: &[u8], pos: &mut usize) -> Result<i16> {
    Ok(read_u16_le(data, pos)? as i16)
}

/// Read a little-endian `u32` from `data` at `*pos`, advancing `*pos` by 4.
#[inline]
pub(crate) fn read_u32_le(data: &[u8], pos: &mut usize) -> Result<u32> {
    let end = *pos + 4;
    if end > data.len() {
        return Err(Error::UnexpectedEof {
            context: String::from("u32"),
        });
    }
    let v = u32::from_le_bytes([data[*pos], data[*pos + 1], data[*pos + 2], data[*pos + 3]]);
    *pos = end;
    Ok(v)
}

/// Read a little-endian `i32` from `data` at `*pos`, advancing `*pos` by 4.
#[inline]
pub(crate) fn read_i32_le(data: &[u8], pos: &mut usize) -> Result<i32> {
    Ok(read_u32_le(data, pos)? as i32)
}

/// Read a little-endian `u64` from `data` at `*pos`, advancing `*pos` by 8.
#[inline]
pub(crate) fn read_u64_le(data: &[u8], pos: &mut usize) -> Result<u64> {
    let end = *pos + 8;
    if end > data.len() {
        return Err(Error::UnexpectedEof {
            context: String::from("u64"),
        });
    }
    let v = u64::from_le_bytes([
        data[*pos],
        data[*pos + 1],
        data[*pos + 2],
        data[*pos + 3],
        data[*pos + 4],
        data[*pos + 5],
        data[*pos + 6],
        data[*pos + 7],
    ]);
    *pos = end;
    Ok(v)
}

/// Read a little-endian `f32` from `data` at `*pos`, advancing `*pos` by 4.
#[inline]
pub(crate) fn read_f32_le(data: &[u8], pos: &mut usize) -> Result<f32> {
    Ok(f32::from_bits(read_u32_le(data, pos)?))
}
