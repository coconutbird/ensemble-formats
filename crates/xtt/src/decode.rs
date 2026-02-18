//! XTT texture decoding module.
//!
//! Decodes compressed albedo atlas from XTT files to RGBA pixels.
//! The albedo data is DXT1 (BC1) compressed with optional Xbox 360 tiling.

use crate::{Error, Result, XttFile};
use byteorder::{BigEndian, ReadBytesExt};
use std::io::Cursor;

/// Decoded albedo atlas information.
#[derive(Debug, Clone)]
pub struct AlbedoAtlas {
    /// Width of the atlas in pixels.
    pub width: u32,
    /// Height of the atlas in pixels.
    pub height: u32,
    /// Number of mip levels.
    pub num_mips: u32,
    /// Decoded RGBA pixel data (width * height * 4 bytes).
    pub pixels: Vec<u8>,
}

/// Header for the albedo atlas chunk.
#[derive(Debug, Clone)]
pub struct AlbedoHeader {
    /// Total memory size of compressed data.
    pub out_mem_size: i32,
    /// Atlas width in pixels.
    pub width: i32,
    /// Atlas height in pixels.
    pub height: i32,
    /// Number of mip levels (includes mip0).
    pub num_mips: i32,
}

impl AlbedoHeader {
    /// Size of the header in bytes.
    pub const SIZE: usize = 16;

    /// Parse albedo header from bytes (BigEndian).
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < Self::SIZE {
            return Err(Error::InvalidChunkData("Albedo header too short".into()));
        }
        let mut cursor = Cursor::new(data);
        Ok(Self {
            out_mem_size: cursor.read_i32::<BigEndian>()?,
            width: cursor.read_i32::<BigEndian>()?,
            height: cursor.read_i32::<BigEndian>()?,
            num_mips: cursor.read_i32::<BigEndian>()?,
        })
    }
}

impl XttFile {
    /// Decode the albedo atlas to RGBA pixels.
    ///
    /// The albedo data is stored as:
    /// - 16-byte header (BigEndian): outMemSize, width, height, numMips
    /// - DXT1 (BC1) compressed data, potentially endian-swapped and tile-swapped
    ///
    /// For DE/PC version, the data appears to be stored without Xbox 360 tiling,
    /// but may still need endian-swapping of the DXT1 blocks.
    pub fn decode_albedo(&self) -> Result<AlbedoAtlas> {
        if self.albedo_data.len() < AlbedoHeader::SIZE {
            return Err(Error::InvalidChunkData("Albedo data too short".into()));
        }

        let header = AlbedoHeader::from_bytes(&self.albedo_data)?;

        if header.width <= 0 || header.height <= 0 {
            return Err(Error::InvalidChunkData(format!(
                "Invalid albedo dimensions: {}x{}",
                header.width, header.height
            )));
        }

        let width = header.width as usize;
        let height = header.height as usize;

        // DXT1/BC1: 4x4 blocks = 8 bytes per block
        // Expected size = (width * height) / 2
        let expected_dxt1_size = (width * height) / 2;
        let data_start = AlbedoHeader::SIZE;
        let available_data = self.albedo_data.len() - data_start;

        if available_data < expected_dxt1_size {
            return Err(Error::InvalidChunkData(format!(
                "Not enough albedo data: expected {} bytes, have {}",
                expected_dxt1_size, available_data
            )));
        }

        // Extract mip0 DXT1 data
        let dxt1_data = &self.albedo_data[data_start..data_start + expected_dxt1_size];

        // For DE/PC, try decoding directly first (no endian swap)
        // If that fails or produces garbage, we'll try with endian swap
        let pixels = decode_dxt1(dxt1_data, width, height)?;

        Ok(AlbedoAtlas {
            width: header.width as u32,
            height: header.height as u32,
            num_mips: header.num_mips as u32,
            pixels,
        })
    }
}

/// Decode DXT1 (BC1) compressed data to RGBA pixels.
fn decode_dxt1(data: &[u8], width: usize, height: usize) -> Result<Vec<u8>> {
    let mut pixels = vec![0u32; width * height];

    texture2ddecoder::decode_bc1(data, width, height, &mut pixels)
        .map_err(|e| Error::InvalidChunkData(format!("DXT1 decode error: {}", e)))?;

    // Convert u32 pixels to RGBA bytes
    let mut rgba = Vec::with_capacity(width * height * 4);
    for pixel in pixels {
        // texture2ddecoder returns BGRA format
        let b = (pixel & 0xFF) as u8;
        let g = ((pixel >> 8) & 0xFF) as u8;
        let r = ((pixel >> 16) & 0xFF) as u8;
        let a = ((pixel >> 24) & 0xFF) as u8;
        rgba.push(r);
        rgba.push(g);
        rgba.push(b);
        rgba.push(a);
    }

    Ok(rgba)
}

