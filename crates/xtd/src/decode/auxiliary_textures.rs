//! Decoding for auxiliary AO, alpha, and lighting textures.

use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;

use nostdio::{Cursor, ReadBe};

use super::{AlphaData, AmbientOcclusionData, nonnegative_usize, untile_r8_texture};
use crate::{Error, Result, XtdFile};

fn half_resolution_dimensions(file: &XtdFile) -> Result<(usize, usize, usize)> {
    let width = nonnegative_usize(file.header.num_x_verts, "terrain vertex count")?;
    if width == 0 {
        return Err(Error::InvalidChunkData(
            "Terrain vertex count must be positive".to_string(),
        ));
    }
    let height = width / 2;
    let texel_count = width
        .checked_mul(height)
        .ok_or(Error::SizeOverflow("half-resolution terrain texture"))?;
    Ok((width, height, texel_count))
}

fn decompress_r8_blocks(data: &[u8]) -> Vec<u8> {
    let mut decompressed = Vec::with_capacity(data.len() / 8 * 8);
    let (blocks, _) = data.as_chunks::<8>();
    for block in blocks {
        let (words, _) = block.as_slice().as_chunks::<2>();
        for word in words {
            let [high, low] = *word;
            decompressed.extend_from_slice(&[low, high]);
        }
    }
    decompressed
}

fn resize_texture(mut data: Vec<u8>, expected_size: usize, fill: u8) -> Vec<u8> {
    if data.len() < expected_size {
        data.resize(expected_size, fill);
    } else {
        data.truncate(expected_size);
    }
    data
}

impl XtdFile {
    /// Decode ambient occlusion data from the AO chunk.
    ///
    /// Based on IDA reverse engineering of the game's decompression (`sub_1407E3440)`:
    /// - Input: 524,288 bytes (8 bytes per block × 65,536 blocks)
    /// - For each 8-byte input block:
    ///   - Read 4× 16-bit big-endian values
    ///   - Byte-swap each to little-endian
    ///   - Write 8 bytes of swapped data + 8 bytes of 0xFF padding (16 bytes total)
    /// - Game allocates (8 * `chunk_size`) >> 2 = 2× input size for output buffer
    ///
    /// The actual AO data is the first 8 bytes of each 16-byte decompressed block.
    /// Total actual data: 65,536 blocks × 8 bytes = 524,288 bytes = 512×1024 R8 texture
    ///
    /// The game samples this half-resolution texture with bilinear filtering and
    /// applies it in the vertex shader via `gVertSampler_ao_Texture`.
    ///
    /// Returns AO values at half resolution (512×1024 for a 1024×1024 terrain).
    ///
    /// # Errors
    ///
    /// Returns an error if the AO chunk is missing or the terrain dimensions
    /// are invalid or too large.
    pub fn decode_ao(&self) -> Result<AmbientOcclusionData> {
        if self.ao_data.is_empty() {
            return Err(Error::InvalidChunkData("AO chunk is empty".to_string()));
        }

        let (width, height, expected_size) = half_resolution_dimensions(self)?;
        let tiled_data = resize_texture(decompress_r8_blocks(&self.ao_data), expected_size, 255);

        // Xbox 360 R8 textures are stored in tiled format
        // For R8 format, tiles are typically 64 bytes arranged as 8x8 pixels
        // The data needs to be un-tiled to linear row-major order
        let values = untile_r8_texture(&tiled_data, width, height);

        Ok(AmbientOcclusionData {
            values,
            width,
            height,
        })
    }

    /// Decode alpha (transparency) data from the Alpha chunk.
    ///
    /// Uses the same decompression as AO data.
    /// Returns alpha values at half resolution.
    ///
    /// # Errors
    ///
    /// Returns an error if the alpha chunk is missing or the terrain dimensions
    /// are invalid or too large.
    pub fn decode_alpha(&self) -> Result<AlphaData> {
        if self.alpha_data.is_empty() {
            return Err(Error::InvalidChunkData("Alpha chunk is empty".to_string()));
        }

        let (width, height, expected_size) = half_resolution_dimensions(self)?;

        // Check for "placeholder" alpha pattern: [255, 255, 0, 0, 0, 0, 0, 0] repeating
        // This indicates no terrain holes - return all-opaque texture
        // Blood Gulch and other maps without terrain holes use this pattern
        let is_placeholder = self.alpha_data.len() >= 8 && {
            let pattern = &[255u8, 255, 0, 0, 0, 0, 0, 0];
            self.alpha_data
                .chunks(8)
                .take(100)
                .all(|chunk| chunk == pattern)
        };

        if is_placeholder {
            return Ok(AlphaData {
                values: vec![255u8; expected_size],
                width,
                height,
            });
        }

        let tiled_data = resize_texture(decompress_r8_blocks(&self.alpha_data), expected_size, 255);

        // Xbox 360 R8 textures are stored in tiled format (same as AO)
        let values = untile_r8_texture(&tiled_data, width, height);

        Ok(AlphaData {
            values,
            width,
            height,
        })
    }

    /// Decode lighting data from the Lighting chunk (0xBBBB).
    ///
    /// The lighting chunk stores a size-prefixed raw L8 (R8) texture at
    /// **full resolution** (`num_x_verts × num_x_verts`).
    ///
    /// Unlike AO/Alpha, the binary does **not** run `decompressToPhysical` on
    /// this data — it is passed directly to `BTerrainVisual::initLightingData`
    /// which creates a `D3DFMT_L8` texture.
    ///
    /// The first 4 bytes are a big-endian i32 size, followed by the raw texels.
    ///
    /// # Errors
    ///
    /// Returns an error if the lighting chunk or its size prefix is invalid, or
    /// the terrain width is not positive.
    pub fn decode_lighting(&self) -> Result<LightingData> {
        if self.lighting_data.is_empty() {
            return Err(Error::InvalidChunkData(
                "Lighting chunk is empty".to_string(),
            ));
        }

        let mut cursor = Cursor::new(self.lighting_data.as_slice());
        let size = nonnegative_usize(cursor.read_i32_be()?, "lighting payload size")?;
        let payload_end = 4usize
            .checked_add(size)
            .ok_or(Error::SizeOverflow("lighting payload"))?;
        let texels = if size > 0
            && let Some(payload) = self.lighting_data.get(4..payload_end)
        {
            payload.to_vec()
        } else {
            // Fall back to everything after the size prefix
            self.lighting_data
                .get(4..)
                .ok_or(Error::UnexpectedEof)?
                .to_vec()
        };

        // Width is always num_x_verts; height is derived from actual data length.
        // Some maps store lighting at half height (num_x_verts × num_x_verts/2),
        // others at full resolution (num_x_verts × num_x_verts).
        let width = nonnegative_usize(self.header.num_x_verts, "terrain vertex count")?;
        if width == 0 {
            return Err(Error::InvalidChunkData(
                "Terrain vertex count must be positive".to_string(),
            ));
        }
        let height = texels.len() / width;

        Ok(LightingData {
            values: texels,
            width,
            height,
        })
    }
}

/// Decoded lighting data (L8/R8 texture).
#[derive(Debug, Clone)]
pub struct LightingData {
    /// Raw L8 luminance values.
    pub values: Vec<u8>,
    /// Texture width (== `num_x_verts`).
    pub width: usize,
    /// Texture height (derived from data length; may be `num_x_verts` or `num_x_verts / 2`).
    pub height: usize,
}
