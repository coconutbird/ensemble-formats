//! Decoding for auxiliary AO, alpha, and lighting textures.

use alloc::format;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;

use nostdio::{Cursor, ReadBe};

use super::{AlphaData, AmbientOcclusionData, nonnegative_usize};
use crate::{Error, Result, XtdFile};

fn texture_dimensions(file: &XtdFile) -> Result<(usize, usize)> {
    let width = nonnegative_usize(file.header.num_x_verts, "terrain vertex count")?;
    if width == 0 {
        return Err(Error::InvalidChunkData(
            "Terrain vertex count must be positive".to_string(),
        ));
    }
    width
        .checked_mul(width)
        .ok_or(Error::SizeOverflow("terrain texture"))?;
    Ok((width, width))
}

fn compact_bc3_size(width: usize, height: usize) -> Result<usize> {
    width
        .div_ceil(4)
        .checked_mul(height.div_ceil(4))
        .and_then(|blocks| blocks.checked_mul(8))
        .ok_or(Error::SizeOverflow("compact BC3 texture"))
}

/// Reproduce `XTD_ExpandCompactBC3Blocks` from the game executable.
fn expand_compact_bc3(data: &[u8], expected_size: usize) -> Result<Vec<u8>> {
    if data.len() != expected_size {
        return Err(Error::InvalidChunkData(format!(
            "Invalid compact BC3 payload size: expected {expected_size}, got {}",
            data.len()
        )));
    }
    let expanded_size = expected_size
        .checked_mul(2)
        .ok_or(Error::SizeOverflow("expanded BC3 texture"))?;
    let mut expanded = Vec::with_capacity(expanded_size);
    for block in data.as_chunks::<8>().0 {
        for word in block.as_slice().as_chunks::<2>().0 {
            expanded.extend_from_slice(&[word[1], word[0]]);
        }
        expanded.extend_from_slice(&[0xFF; 8]);
    }
    Ok(expanded)
}

fn decode_compact_bc3_alpha(data: &[u8], width: usize, height: usize) -> Result<Vec<u8>> {
    let expanded = expand_compact_bc3(data, compact_bc3_size(width, height)?)?;
    let pixel_count = width
        .checked_mul(height)
        .ok_or(Error::SizeOverflow("decoded BC3 pixels"))?;
    let mut pixels = vec![0u32; pixel_count];
    texture2ddecoder::decode_bc3(&expanded, width, height, &mut pixels)
        .map_err(|error| Error::InvalidChunkData(format!("BC3 decode error: {error}")))?;
    Ok(pixels
        .into_iter()
        .map(|pixel| pixel.to_le_bytes()[3])
        .collect())
}

fn decode_bc1_rgba(data: &[u8], width: usize, height: usize) -> Result<Vec<u8>> {
    let expected_size = width
        .div_ceil(4)
        .checked_mul(height.div_ceil(4))
        .and_then(|blocks| blocks.checked_mul(8))
        .ok_or(Error::SizeOverflow("BC1 texture"))?;
    if data.len() != expected_size {
        return Err(Error::InvalidChunkData(format!(
            "Invalid BC1 payload size: expected {expected_size}, got {}",
            data.len()
        )));
    }
    let pixel_count = width
        .checked_mul(height)
        .ok_or(Error::SizeOverflow("decoded BC1 pixels"))?;
    let rgba_size = pixel_count
        .checked_mul(4)
        .ok_or(Error::SizeOverflow("decoded BC1 bytes"))?;
    let mut pixels = vec![0u32; pixel_count];
    texture2ddecoder::decode_bc1(data, width, height, &mut pixels)
        .map_err(|error| Error::InvalidChunkData(format!("BC1 decode error: {error}")))?;
    let mut rgba = Vec::with_capacity(rgba_size);
    for pixel in pixels {
        let [blue, green, red, alpha] = pixel.to_le_bytes();
        rgba.extend_from_slice(&[red, green, blue, alpha]);
    }
    Ok(rgba)
}

impl XtdFile {
    /// Decode the full-resolution ambient-occlusion texture.
    ///
    /// The source stores only the BC3 alpha half of every block. This method
    /// exactly reproduces the game's word swaps and opaque-white color half,
    /// decodes the resulting `BC3_UNORM` texture, and returns its alpha channel.
    ///
    /// # Errors
    ///
    /// Returns an error if the AO chunk is missing, has the wrong encoded size,
    /// or the terrain dimensions overflow.
    pub fn decode_ao(&self) -> Result<AmbientOcclusionData> {
        if self.ao_data.is_empty() {
            return Err(Error::InvalidChunkData("AO chunk is empty".to_string()));
        }
        let (width, height) = texture_dimensions(self)?;
        let values = decode_compact_bc3_alpha(&self.ao_data, width, height)?;
        Ok(AmbientOcclusionData {
            values,
            width,
            height,
        })
    }

    /// Decode the full-resolution terrain transparency texture.
    ///
    /// This uses the same compact BC3-alpha representation as AO. No sentinel
    /// or placeholder patterns are special-cased because the game always runs
    /// the block expansion.
    ///
    /// # Errors
    ///
    /// Returns an error if the alpha chunk is missing, has the wrong encoded
    /// size, or the terrain dimensions overflow.
    pub fn decode_alpha(&self) -> Result<AlphaData> {
        if self.alpha_data.is_empty() {
            return Err(Error::InvalidChunkData("Alpha chunk is empty".to_string()));
        }
        let (width, height) = texture_dimensions(self)?;
        let values = decode_compact_bc3_alpha(&self.alpha_data, width, height)?;
        Ok(AlphaData {
            values,
            width,
            height,
        })
    }

    /// Decode the size-prefixed full-resolution BC1 lighting texture to RGBA.
    ///
    /// IDA shows `BTerrainVisual::initLightingData` creates engine format 22,
    /// which maps to `DXGI_FORMAT_BC1_UNORM` (71), at
    /// `num_x_verts × num_x_verts`.
    ///
    /// # Errors
    ///
    /// Returns an error if the chunk is absent, its size prefix or payload is
    /// inconsistent, or BC1 decoding fails.
    pub fn decode_lighting(&self) -> Result<LightingData> {
        if self.lighting_data.is_empty() {
            return Err(Error::InvalidChunkData(
                "Lighting chunk is empty".to_string(),
            ));
        }

        let mut cursor = Cursor::new(self.lighting_data.as_slice());
        let encoded_size = nonnegative_usize(cursor.read_i32_be()?, "lighting payload size")?;
        let payload_end = 4usize
            .checked_add(encoded_size)
            .ok_or(Error::SizeOverflow("lighting payload"))?;
        if payload_end != self.lighting_data.len() {
            return Err(Error::InvalidChunkData(format!(
                "Lighting size prefix describes {encoded_size} bytes, but chunk contains {}",
                self.lighting_data.len().saturating_sub(4)
            )));
        }
        let payload = self
            .lighting_data
            .get(4..payload_end)
            .ok_or(Error::UnexpectedEof)?;
        let (width, height) = texture_dimensions(self)?;
        let pixels = decode_bc1_rgba(payload, width, height)?;

        Ok(LightingData {
            pixels,
            width,
            height,
        })
    }
}

/// Decoded full-resolution BC1 lighting texture.
#[derive(Debug, Clone)]
pub struct LightingData {
    /// RGBA8 pixels in row-major order.
    pub pixels: Vec<u8>,
    /// Texture width (the terrain vertex count).
    pub width: usize,
    /// Texture height (the terrain vertex count).
    pub height: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{XTD_VERSION, XtdHeader};

    fn file_with_width(width: i32) -> XtdFile {
        XtdFile {
            header: XtdHeader {
                version: XTD_VERSION,
                num_x_verts: width,
                ..XtdHeader::default()
            },
            ..XtdFile::default()
        }
    }

    #[test]
    fn compact_bc3_expansion_matches_game_transform() {
        let source = [0, 1, 2, 3, 4, 5, 6, 7];
        let expanded = expand_compact_bc3(&source, 8).unwrap();
        assert_eq!(
            expanded,
            [
                1, 0, 3, 2, 5, 4, 7, 6, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF
            ]
        );
    }

    #[test]
    fn compact_bc3_decodes_at_full_resolution() {
        let mut file = file_with_width(4);
        file.ao_data = vec![0; 8];
        let decoded = file.decode_ao().unwrap();
        assert_eq!((decoded.width, decoded.height), (4, 4));
        assert_eq!(decoded.values, vec![0; 16]);
    }

    #[test]
    fn lighting_is_full_resolution_bc1_rgba() {
        let mut file = file_with_width(4);
        file.lighting_data.extend_from_slice(&8i32.to_be_bytes());
        file.lighting_data.extend_from_slice(&[0; 8]);
        let decoded = file.decode_lighting().unwrap();
        assert_eq!((decoded.width, decoded.height), (4, 4));
        assert_eq!(decoded.pixels.len(), 4 * 4 * 4);
    }
}
