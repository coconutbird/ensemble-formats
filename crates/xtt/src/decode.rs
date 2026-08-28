//! XTT texture decoding module.
//!
//! Decodes compressed albedo atlas from XTT files to RGBA pixels.
//! The DE albedo data is a linear BC1 mip chain.
//! Also provides alpha texture unpacking for splat blending.

use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

use nostdio::{Cursor, ReadBe};
use zerocopy::Ref;

use crate::types::RoadData;
use crate::{AlbedoHeaderRaw, Error, Result, XttFile, XttLinker};

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

    /// Parse albedo header from bytes (`BigEndian`, zero-copy).
    ///
    /// # Errors
    ///
    /// Returns an error if `data` is shorter than the 16-byte albedo header.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let (raw, _): (Ref<_, AlbedoHeaderRaw>, _) = Ref::from_prefix(data)
            .map_err(|_| Error::InvalidChunkData("Albedo header too short".into()))?;
        Ok(Self {
            out_mem_size: i32::from_be_bytes(raw.out_mem_size),
            width: i32::from_be_bytes(raw.width),
            height: i32::from_be_bytes(raw.height),
            num_mips: i32::from_be_bytes(raw.num_mips),
        })
    }
}

impl XttFile {
    /// Decode the albedo atlas to RGBA pixels.
    ///
    /// The albedo data is stored as:
    /// - 16-byte header (BigEndian): outMemSize, width, height, numMips
    /// - A linear BC1 mip chain in PC byte order
    ///
    /// # Errors
    ///
    /// Returns an error if the header or dimensions are invalid, the compressed
    /// payload is truncated, or the BC1 decoder rejects the payload.
    pub fn decode_albedo(&self) -> Result<AlbedoAtlas> {
        let header = validate_albedo_data(&self.albedo_data)?;

        let width = nonnegative_usize(header.width, "albedo width")?;
        let height = nonnegative_usize(header.height, "albedo height")?;
        let atlas_width = u32::try_from(header.width)
            .map_err(|_| Error::InvalidChunkData("Invalid albedo width".into()))?;
        let atlas_height = u32::try_from(header.height)
            .map_err(|_| Error::InvalidChunkData("Invalid albedo height".into()))?;
        let num_mips = u32::try_from(header.num_mips)
            .map_err(|_| Error::InvalidChunkData("Invalid albedo mip count".into()))?;

        // DXT1/BC1 stores each 4x4 block in 8 bytes.
        let expected_dxt1_size = width
            .div_ceil(4)
            .checked_mul(height.div_ceil(4))
            .and_then(|blocks| blocks.checked_mul(8))
            .ok_or(Error::SizeOverflow("albedo texture"))?;
        let data_start = AlbedoHeader::SIZE;
        // Extract mip0 DXT1 data
        let data_end = data_start
            .checked_add(expected_dxt1_size)
            .ok_or(Error::SizeOverflow("albedo payload"))?;
        let dxt1_data = &self.albedo_data[data_start..data_end];

        let pixels = decode_dxt1(dxt1_data, width, height)?;

        Ok(AlbedoAtlas {
            width: atlas_width,
            height: atlas_height,
            num_mips,
            pixels,
        })
    }

    /// Decode road data from the raw road chunk (0x8888).
    ///
    /// Convenience method that delegates to [`decode_road_data`].
    /// Returns `None` if no road data is present.
    ///
    /// # Errors
    ///
    /// Returns an error if the road chunk is malformed or truncated.
    pub fn decode_road(&self) -> Result<Option<RoadData>> {
        if self.road_data.is_empty() {
            return Ok(None);
        }
        decode_road_data(&self.road_data).map(Some)
    }
}

pub(crate) fn validate_albedo_data(data: &[u8]) -> Result<AlbedoHeader> {
    let header = AlbedoHeader::from_bytes(data)?;
    let width = positive_usize(header.width, "albedo width")?;
    let height = positive_usize(header.height, "albedo height")?;
    let mip_count = positive_usize(header.num_mips, "albedo mip count")?;
    let encoded_size = positive_usize(header.out_mem_size, "albedo output size")?;
    let expected_size = bc1_mip_chain_size(width, height, mip_count)?;
    if encoded_size != expected_size {
        return Err(Error::InvalidChunkData(format!(
            "Albedo output size is {encoded_size}, but {mip_count} BC1 mips require {expected_size}"
        )));
    }
    let expected_chunk_size = AlbedoHeader::SIZE
        .checked_add(encoded_size)
        .ok_or(Error::SizeOverflow("albedo chunk"))?;
    if data.len() != expected_chunk_size {
        return Err(Error::InvalidChunkData(format!(
            "Albedo chunk size is {}, expected {expected_chunk_size}",
            data.len()
        )));
    }
    Ok(header)
}

fn positive_usize(value: i32, field: &'static str) -> Result<usize> {
    let converted = usize::try_from(value)
        .map_err(|_| Error::InvalidChunkData(format!("Invalid {field}: {value}")))?;
    if converted == 0 {
        Err(Error::InvalidChunkData(format!("{field} must be positive")))
    } else {
        Ok(converted)
    }
}

fn bc1_mip_chain_size(mut width: usize, mut height: usize, mip_count: usize) -> Result<usize> {
    let mut total = 0usize;
    for _ in 0..mip_count {
        let level_size = width
            .div_ceil(4)
            .checked_mul(height.div_ceil(4))
            .and_then(|blocks| blocks.checked_mul(8))
            .ok_or(Error::SizeOverflow("albedo mip"))?;
        total = total
            .checked_add(level_size)
            .ok_or(Error::SizeOverflow("albedo mip chain"))?;
        width = (width / 2).max(1);
        height = (height / 2).max(1);
    }
    Ok(total)
}

/// Decode DXT1 (BC1) compressed data to RGBA pixels.
fn decode_dxt1(data: &[u8], width: usize, height: usize) -> Result<Vec<u8>> {
    let pixel_count = width
        .checked_mul(height)
        .ok_or(Error::SizeOverflow("decoded albedo pixels"))?;
    let mut pixels = vec![0u32; pixel_count];

    texture2ddecoder::decode_bc1(data, width, height, &mut pixels)
        .map_err(|e| Error::InvalidChunkData(format!("DXT1 decode error: {e}")))?;

    // Convert u32 pixels to RGBA bytes
    let rgba_size = pixel_count
        .checked_mul(4)
        .ok_or(Error::SizeOverflow("decoded albedo bytes"))?;
    let mut rgba = Vec::with_capacity(rgba_size);
    for pixel in pixels {
        // texture2ddecoder returns BGRA format
        let [b, g, r, a] = pixel.to_le_bytes();
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
    ///
    /// # Errors
    ///
    /// Returns an error if the layer count is negative, a required allocation
    /// size overflows, or the packed alpha payload is truncated.
    pub fn decode_splat_alpha(&self) -> Result<SplatAlphaData> {
        let num_layers = nonnegative_usize(self.num_splat_layers, "splat layer count")?;
        if num_layers <= 1 {
            // Single layer = no blending needed
            return Ok(SplatAlphaData {
                num_layers,
                alpha_maps: Vec::new(),
            });
        }

        let num_slices = ((num_layers - 1) >> 2) + 1; // Each slice holds 4 layers
        let expected_size = alpha_payload_size(num_slices)?;

        if self.splat_alpha_data.len() < expected_size {
            return Err(Error::InvalidChunkData(format!(
                "Splat alpha data too small: expected {} bytes, have {}",
                expected_size,
                self.splat_alpha_data.len()
            )));
        }

        // Decode each overlay layer's alpha (layer 0 is the base, no alpha needed).
        // The alpha data packs 4 channels per slice. The base layer reserves
        // channel 0 (unused), so the first overlay uses channel 1:
        //   channel = layer_idx % 4,  slice = layer_idx / 4
        // This is confirmed by the num_slices formula which allocates a slot
        // for the base: ((num_splat_layers - 1) >> 2) + 1
        let mut alpha_maps = Vec::with_capacity(num_layers - 1);

        for layer_idx in 1..num_layers {
            let alpha_map = decode_layer_alpha(&self.splat_alpha_data, layer_idx)?;
            alpha_maps.push(alpha_map);
        }

        Ok(SplatAlphaData {
            num_layers,
            alpha_maps,
        })
    }

    /// Decode the packed A4R4G4B4 decal alpha data into individual layer alpha maps.
    ///
    /// Uses the same format as splat alpha data:
    /// - 64x64 pixels, 16bpp (A4R4G4B4)
    /// - 4 alpha channels per pixel
    /// - Multiple "slices" for chunks with more than 4 decal layers
    ///
    /// Returns alpha maps for decal layers (one per decal layer).
    ///
    /// # Errors
    ///
    /// Returns an error if the layer count is negative, a required allocation
    /// size overflows, or the packed alpha payload is truncated.
    pub fn decode_decal_alpha(&self) -> Result<DecalAlphaData> {
        let num_layers = nonnegative_usize(self.num_decal_layers, "decal layer count")?;
        if num_layers == 0 {
            return Ok(DecalAlphaData {
                num_layers: 0,
                alpha_maps: Vec::new(),
            });
        }

        let num_slices = ((num_layers - 1) >> 2) + 1;
        let expected_size = alpha_payload_size(num_slices)?;

        if self.decal_alpha_data.len() < expected_size {
            return Err(Error::InvalidChunkData(format!(
                "Decal alpha data too small: expected {} bytes, have {}",
                expected_size,
                self.decal_alpha_data.len()
            )));
        }

        // Decode each decal layer's alpha
        let mut alpha_maps = Vec::with_capacity(num_layers);

        for layer_idx in 0..num_layers {
            let alpha_map = decode_layer_alpha(&self.decal_alpha_data, layer_idx)?;
            alpha_maps.push(alpha_map);
        }

        Ok(DecalAlphaData {
            num_layers,
            alpha_maps,
        })
    }
}

fn nonnegative_usize(value: i32, field: &'static str) -> Result<usize> {
    usize::try_from(value).map_err(|_| Error::InvalidChunkData(format!("Invalid {field}: {value}")))
}

fn alpha_payload_size(num_slices: usize) -> Result<usize> {
    num_slices
        .checked_mul(ALPHA_TEXTURE_SIZE)
        .and_then(|size| size.checked_mul(ALPHA_TEXTURE_SIZE))
        .and_then(|size| size.checked_mul(2))
        .ok_or(Error::SizeOverflow("alpha texture payload"))
}

/// Decoded decal alpha data for a terrain chunk.
#[derive(Debug, Clone)]
pub struct DecalAlphaData {
    /// Number of decal layers in this chunk.
    pub num_layers: usize,
    /// Alpha maps for each decal layer.
    /// Each map is 64x64 = 4096 bytes.
    pub alpha_maps: Vec<Vec<u8>>,
}

/// Calculate Xbox 360 tiled texture offset for 16bpp texture.
///
/// Xbox 360 uses a tiled memory layout. This converts (x, y) to the
/// u16 index in the tiled source data.
///
/// Reverse engineered from `untile_xbox360_alpha_texture` (0x1407E34E0).
/// Translated directly from the assembly to ensure correctness.
/// Xbox 360 tiled texture offset calculation.
/// This matches the game's `untile_xbox360_alpha_texture` function at 0x1407E34E0.
fn xbox360_tiled_offset(x: u32, y: u32, width: u32) -> Option<usize> {
    // Block width calculation: (width + 31) >> 5
    let block_width = (width + 31) >> 5;

    // Pre-computed values from y (computed once per row in the game)
    let paired_row_bits = (y & 6) << 2;
    let odd_row_bit = (y & 1) << 3;
    let eighth_row_bit = (y & 8) << 1;
    let sixteenth_row_bit = (y & 0x10) << 4;
    let macro_row = (y >> 5) * block_width;
    let row_band = (y & 0xF8) << 1;

    // Per-pixel calculation (inner loop)
    let mut pixel_lane = (x & 7) + paired_row_bits;
    pixel_lane += pixel_lane;

    let mut macro_base = (macro_row << 5) + (x & 0xFFFF_FFE0);

    let aligned_lane = pixel_lane & 0xFFFF_FFF0;
    pixel_lane &= 0xF;

    let row_aligned_lane = aligned_lane + odd_row_bit;
    macro_base = row_aligned_lane + (macro_base << 2);

    let shuffled_lane = pixel_lane + (eighth_row_bit << 3);
    let tiled_pair = shuffled_lane + (macro_base << 1);

    // Extract bits for v21
    let high_region = (tiled_pair & 0xFFFF_FE00) + sixteenth_row_bit;
    let middle_region = tiled_pair & 0x1C0;
    let low_region = tiled_pair & 0x3F;

    let tiled_region = middle_region + (high_region << 1);

    // Final calculation
    let row_column_mix = row_band + x;
    let row_shuffle = (row_column_mix << 3) & 0xC0;

    let byte_offset = row_shuffle + (tiled_region << 2) + low_region;

    usize::try_from(byte_offset >> 1).ok()
}

/// Decode a single layer's alpha map from the packed alpha data.
///
/// The alpha data is stored in Xbox 360 tiled format and needs deswizzling.
/// Each 16-bit pixel contains 4 layers of 4-bit alpha packed together.
///
/// Channel extraction from A4R4G4B4 Xbox 360 alpha texture data.
///
/// # How it works
///
/// The XTT alpha data is stored as Xbox 360 tiled A4R4G4B4 pixels. The DE game's
/// `untile_xbox360_alpha_texture` (0x1407E34E0) reads each pixel as a little-endian
/// u16 and writes 4 expanded bytes per pixel. We replicate this exactly, extracting
/// one channel at a time.
///
/// The A4R4G4B4 data was authored for the Xbox 360 (big-endian). Reading the same
/// bytes as LE u16 on PC byte-swaps the two bytes, shuffling the nibble positions:
///
///   BE layout: bits 12-15=A, 8-11=R, 4-7=G, 0-3=B
///   LE layout: bits 12-15=G, 8-11=B, 4-7=A, 0-3=R
///
/// The untile function extracts nibbles by their A4R4G4B4 *field name* at each LE
/// bit position, producing bytes in "BARG" order. But because of the endian swap,
/// the actual *content* at each position is the original Xbox 360 channel data:
///
///   channel 0 → LE bits 0-3  (field "B") → content: **R**
///   channel 1 → LE bits 12-15 (field "A") → content: **G**
///   channel 2 → LE bits 8-11  (field "R") → content: **B**
///   channel 3 → LE bits 4-7   (field "G") → content: **A**
///
/// This matches the Xbox 360 GPU's native A4R4G4B4 sampling order:
///   `[0]`=.r=R, `[1]`=.g=G, `[2]`=.b=B, `[3]`=.a=A
///
/// # DE game's pipeline (for reference)
///
/// In `BTerrainIOLoader::loadXTTInternal` (0x14066C990), the DE:
/// 1. Calls `untile_xbox360_alpha_texture` to produce BARG-ordered bytes.
/// 2. The byte-swap loop at 0x14066CCB2 does **not** operate on the alpha data —
///    it endian-swaps the **splat layer ID array** (int32 texture indices) from
///    Xbox 360 big-endian to PC little-endian.
/// 3. Both buffers are passed to `processLinkerData` (0x14066D890), which stores
///    them using X-major chunk indexing: `gridZ + numXChunks * gridX`.
fn decode_layer_alpha(data: &[u8], layer_idx: usize) -> Result<Vec<u8>> {
    // layer_idx is the logical layer number. Splat layer 0 is the implicit
    // base, so channel 0 remains reserved and the first explicit overlay uses
    // channel 1. Decal layers start at logical layer/channel 0.
    let slice_idx = layer_idx / 4;
    let channel_idx = layer_idx % 4;

    let mut alpha_map = vec![0u8; ALPHA_TEXTURE_SIZE * ALPHA_TEXTURE_SIZE];

    for y in 0..ALPHA_TEXTURE_SIZE {
        for x in 0..ALPHA_TEXTURE_SIZE {
            // Use GLOBAL y coordinate - slices are stacked vertically in tiled data
            let global_y = slice_idx
                .checked_mul(ALPHA_TEXTURE_SIZE)
                .and_then(|offset| offset.checked_add(y))
                .and_then(|coordinate| u32::try_from(coordinate).ok())
                .ok_or(Error::SizeOverflow("alpha texture Y coordinate"))?;
            let pixel_x =
                u32::try_from(x).map_err(|_| Error::SizeOverflow("alpha texture X coordinate"))?;

            // Get tiled offset for this (x, global_y) position
            let tiled_index = xbox360_tiled_offset(pixel_x, global_y, 64)
                .ok_or(Error::SizeOverflow("tiled alpha index"))?;
            let byte_offset = tiled_index
                .checked_mul(2)
                .ok_or(Error::SizeOverflow("tiled alpha byte offset"))?;
            let byte_end = byte_offset
                .checked_add(2)
                .ok_or(Error::SizeOverflow("tiled alpha pixel range"))?;
            let pixel_bytes = data.get(byte_offset..byte_end).ok_or_else(|| {
                Error::InvalidChunkData(format!(
                    "Tiled alpha pixel range {byte_offset}..{byte_end} exceeds {} bytes",
                    data.len()
                ))
            })?;

            // Read as little-endian (PC/DE format, matching game's x86 uint16 read)
            let [low_byte, high_byte] = [pixel_bytes[0], pixel_bytes[1]];

            // BARG extraction: LE bit positions → original Xbox 360 channel content
            //   channel 0 → bits 0-3  ("B" field) → Xbox R
            //   channel 1 → bits 12-15 ("A" field) → Xbox G
            //   channel 2 → bits 8-11  ("R" field) → Xbox B
            //   channel 3 → bits 4-7   ("G" field) → Xbox A
            let alpha_4bit = match channel_idx {
                0 => low_byte & 0x0F,  // "B" field → Xbox R
                1 => high_byte >> 4,   // "A" field → Xbox G
                2 => high_byte & 0x0F, // "R" field → Xbox B
                3 => low_byte >> 4,    // "G" field → Xbox A
                _ => {
                    return Err(Error::InvalidChunkData(
                        "Invalid alpha channel index".into(),
                    ));
                }
            };

            // Expand 4-bit (0-15) to 8-bit (0-255)
            let alpha_8bit = (alpha_4bit << 4) | alpha_4bit;
            alpha_map[y * ALPHA_TEXTURE_SIZE + x] = alpha_8bit;
        }
    }

    Ok(alpha_map)
}

// ============================================================================
// Road Data Decoding
// ============================================================================

use alloc::string::String;

use crate::types::{RoadQNChunk, RoadVertex};
use half::f16;

/// Decode road data from the raw XTT road chunk (0x8888).
///
/// Binary format (big-endian / Xbox 360):
/// - `char[32]`: texture filename (null-terminated)
/// - `i32`: number of QN chunks
/// - For each QN chunk:
///   - `i32`: owner QN index
///   - `i32`: number of triangles
///   - `i32`: memory size in bytes
///   - Vertex data: `numTris * 3` vertices, each = 6 × float16:
///     `[posX, posY, posZ, pad, uvX, uvY]`
///
/// # Errors
///
/// Returns an error if the road chunk is truncated, contains a negative count,
/// or describes more vertices than can fit in memory.
pub fn decode_road_data(data: &[u8]) -> Result<RoadData> {
    let texture_bytes = data.get(..32).ok_or(Error::UnexpectedEof)?;
    let name_end = texture_bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(texture_bytes.len());
    let texture_name = String::from_utf8_lossy(&texture_bytes[..name_end]).into_owned();
    let mut cursor = Cursor::new(data.get(32..).ok_or(Error::UnexpectedEof)?);

    let num_qn_chunks = nonnegative_usize(cursor.read_i32_be()?, "road QN chunk count")?;
    let mut qn_chunks = Vec::with_capacity(num_qn_chunks);

    for _ in 0..num_qn_chunks {
        let qn_index = cursor.read_i32_be()?;
        let num_tris = nonnegative_usize(cursor.read_i32_be()?, "road triangle count")?;
        let memory_size = nonnegative_usize(cursor.read_i32_be()?, "road vertex memory size")?;

        let num_verts = num_tris
            .checked_mul(3)
            .ok_or(Error::SizeOverflow("road vertex count"))?;
        let expected_memory_size = num_verts
            .checked_mul(12)
            .ok_or(Error::SizeOverflow("road vertex data"))?;
        if memory_size != expected_memory_size {
            return Err(Error::InvalidChunkData(format!(
                "Road QN declares {memory_size} vertex bytes, expected {expected_memory_size} for {num_tris} triangles"
            )));
        }
        let mut vertices = Vec::with_capacity(num_verts);

        for _ in 0..num_verts {
            // Each vertex: 6 × float16 (big-endian)
            let position_x = f16::from_bits(cursor.read_u16_be()?).to_f32();
            let position_y = f16::from_bits(cursor.read_u16_be()?).to_f32();
            let position_z = f16::from_bits(cursor.read_u16_be()?).to_f32();
            let _padding = cursor.read_u16_be()?;
            let texture_u = f16::from_bits(cursor.read_u16_be()?).to_f32();
            let texture_v = f16::from_bits(cursor.read_u16_be()?).to_f32();

            vertices.push(RoadVertex {
                position: [position_x, position_y, position_z],
                uv: [texture_u, texture_v],
            });
        }

        qn_chunks.push(RoadQNChunk { qn_index, vertices });
    }

    let consumed =
        usize::try_from(cursor.position()).map_err(|_| Error::SizeOverflow("road chunk offset"))?;
    let payload_size = data.len() - 32;
    if consumed != payload_size {
        return Err(Error::InvalidChunkData(format!(
            "Road chunk has {} trailing bytes",
            payload_size.saturating_sub(consumed)
        )));
    }

    Ok(RoadData {
        texture_name,
        qn_chunks,
    })
}
