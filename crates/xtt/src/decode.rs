//! XTT texture decoding module.
//!
//! Decodes compressed albedo atlas from XTT files to RGBA pixels.
//! The albedo data is DXT1 (BC1) compressed with optional Xbox 360 tiling.
//! Also provides alpha texture unpacking for splat blending.

use crate::{Error, Result, XttFile, XttLinker};
use byteorder::{BigEndian, LittleEndian, ReadBytesExt};
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

// ============================================================================
// Alpha texture unpacking for splat blending
// ============================================================================

/// Alpha texture dimensions (fixed for terrain splatting).
pub const ALPHA_TEXTURE_SIZE: usize = 64;

/// Decoded splat alpha data for a terrain chunk.
///
/// Contains individual alpha maps for each splat layer (0-255 per pixel).
/// The first layer (base) has no alpha map - it fills where others don't.
#[derive(Debug, Clone)]
pub struct SplatAlphaData {
    /// Number of splat layers in this chunk.
    pub num_layers: usize,
    /// Alpha maps for layers 1..n (layer 0 has no explicit alpha).
    /// Each map is 64x64 = 4096 bytes.
    pub alpha_maps: Vec<Vec<u8>>,
}

impl XttLinker {
    /// Decode the packed A4R4G4B4 alpha data into individual layer alpha maps.
    ///
    /// The alpha data format is:
    /// - 64x64 pixels, 16bpp (A4R4G4B4)
    /// - 4 alpha channels per pixel (A, R, G, B each contain 4-bit alpha)
    /// - Multiple "slices" for chunks with more than 4 layers
    ///
    /// Returns alpha maps for layers 1..n (layer 0 is the base with no alpha).
    pub fn decode_splat_alpha(&self) -> Result<SplatAlphaData> {
        if self.num_splat_layers <= 1 {
            // Single layer = no blending needed
            return Ok(SplatAlphaData {
                num_layers: self.num_splat_layers as usize,
                alpha_maps: Vec::new(),
            });
        }

        let num_layers = self.num_splat_layers as usize;
        let num_slices = ((num_layers - 1) >> 2) + 1; // Each slice holds 4 layers
        let expected_size = num_slices * ALPHA_TEXTURE_SIZE * ALPHA_TEXTURE_SIZE * 2;

        if self.splat_alpha_data.len() < expected_size {
            return Err(Error::InvalidChunkData(format!(
                "Splat alpha data too small: expected {} bytes, have {}",
                expected_size,
                self.splat_alpha_data.len()
            )));
        }

        // Decode each layer's alpha (starting from layer 1, layer 0 has no alpha)
        let mut alpha_maps = Vec::with_capacity(num_layers - 1);

        for layer_idx in 1..num_layers {
            let alpha_map = decode_layer_alpha(&self.splat_alpha_data, layer_idx, num_slices)?;
            alpha_maps.push(alpha_map);
        }

        Ok(SplatAlphaData {
            num_layers,
            alpha_maps,
        })
    }
}

/// Decode a single layer's alpha map from the packed alpha data.
///
/// From ExportXTT.cs writePacked4bitLayers():
/// The original Xbox 360 export packs layers with shiftOffset = {8, 4, 0, 12}:
/// - Layer 0 → shift 8 → bits 8-11
/// - Layer 1 → shift 4 → bits 4-7
/// - Layer 2 → shift 0 → bits 0-3
/// - Layer 3 → shift 12 → bits 12-15
///
/// The intArrayToByteArrayToFile() function writes bytes in big-endian order
/// for Xbox 360. When we read the file as BigEndian, we get the original
/// Int16 bit layout back.
///
/// Slice layout:
/// - Slice 0: layers 0-3
/// - Slice 1: layers 4-7
/// - etc.
fn decode_layer_alpha(data: &[u8], layer_idx: usize, _num_slices: usize) -> Result<Vec<u8>> {
    // layer_idx is 0-based
    let slice_idx = layer_idx / 4;
    let channel_idx = layer_idx % 4;

    let slice_size = ALPHA_TEXTURE_SIZE * ALPHA_TEXTURE_SIZE * 2; // 8192 bytes per slice
    let slice_offset = slice_idx * slice_size;

    let mut alpha_map = vec![0u8; ALPHA_TEXTURE_SIZE * ALPHA_TEXTURE_SIZE];
    let mut cursor = Cursor::new(&data[slice_offset..]);

    for y in 0..ALPHA_TEXTURE_SIZE {
        for x in 0..ALPHA_TEXTURE_SIZE {
            // Read 16-bit pixel as BigEndian (matches Xbox 360 export format)
            // This gives us the original packed Int16 bit layout
            let pixel = cursor.read_u16::<BigEndian>().map_err(|e| {
                Error::InvalidChunkData(format!("Failed to read alpha pixel: {}", e))
            })?;

            // Extract 4-bit channel using original shiftOffset = {8, 4, 0, 12}
            // Layer 0 → bits 8-11, Layer 1 → bits 4-7, Layer 2 → bits 0-3, Layer 3 → bits 12-15
            let alpha_4bit = match channel_idx {
                0 => ((pixel >> 8) & 0x0F) as u8,    // Layer 0: bits 8-11
                1 => ((pixel >> 4) & 0x0F) as u8,    // Layer 1: bits 4-7
                2 => (pixel & 0x000F) as u8,         // Layer 2: bits 0-3
                3 => ((pixel >> 12) & 0x0F) as u8,   // Layer 3: bits 12-15
                _ => unreachable!(),
            };

            // Expand 4-bit (0-15) to 8-bit (0-255)
            let alpha_8bit = (alpha_4bit << 4) | alpha_4bit;
            alpha_map[y * ALPHA_TEXTURE_SIZE + x] = alpha_8bit;
        }
    }

    Ok(alpha_map)
}

