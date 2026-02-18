//! DDX texture decoding to RGBA pixels.
//!
//! Converts compressed texture data (DXT1, DXT5, DXN, etc.) to RGBA8 format.

use crate::format::DataFormat;
use crate::reader::DdxTexture;
use crate::{Error, Result};

/// Decoded RGBA texture data.
#[derive(Debug, Clone)]
pub struct DecodedTexture {
    /// Texture width in pixels.
    pub width: u32,
    /// Texture height in pixels.
    pub height: u32,
    /// RGBA pixel data (width * height * 4 bytes).
    pub pixels: Vec<u8>,
}

impl DdxTexture {
    /// Decode the texture data to RGBA8 pixels.
    ///
    /// This decompresses DXT/BC formats to standard RGBA8.
    /// Only decodes the base mip level (mip0).
    pub fn decode_to_rgba(&self) -> Result<DecodedTexture> {
        let width = self.info.width as usize;
        let height = self.info.height as usize;

        // Allocate output buffer
        let mut pixels_u32 = vec![0u32; width * height];

        // Decode based on format
        match self.info.data_format {
            DataFormat::Dxt1 => {
                // DXT1/BC1: 4x4 blocks, 8 bytes per block
                let expected_size = ((width + 3) / 4) * ((height + 3) / 4) * 8;
                if self.data.len() < expected_size {
                    return Err(Error::DecompressionError(format!(
                        "DXT1 data too small: expected {} bytes, have {}",
                        expected_size,
                        self.data.len()
                    )));
                }
                texture2ddecoder::decode_bc1(&self.data[..expected_size], width, height, &mut pixels_u32)
                    .map_err(|e| Error::DecompressionError(format!("BC1 decode error: {}", e)))?;
            }
            DataFormat::Dxt3 => {
                // DXT3/BC2: 4x4 blocks, 16 bytes per block
                let expected_size = ((width + 3) / 4) * ((height + 3) / 4) * 16;
                if self.data.len() < expected_size {
                    return Err(Error::DecompressionError(format!(
                        "DXT3 data too small: expected {} bytes, have {}",
                        expected_size,
                        self.data.len()
                    )));
                }
                texture2ddecoder::decode_bc2(&self.data[..expected_size], width, height, &mut pixels_u32)
                    .map_err(|e| Error::DecompressionError(format!("BC2 decode error: {}", e)))?;
            }
            DataFormat::Dxt5 | DataFormat::Dxt5N | DataFormat::Dxt5Y | DataFormat::Dxt5H => {
                // DXT5/BC3: 4x4 blocks, 16 bytes per block
                let expected_size = ((width + 3) / 4) * ((height + 3) / 4) * 16;
                if self.data.len() < expected_size {
                    return Err(Error::DecompressionError(format!(
                        "DXT5 data too small: expected {} bytes, have {}",
                        expected_size,
                        self.data.len()
                    )));
                }
                texture2ddecoder::decode_bc3(&self.data[..expected_size], width, height, &mut pixels_u32)
                    .map_err(|e| Error::DecompressionError(format!("BC3 decode error: {}", e)))?;
            }
            DataFormat::Dxn => {
                // DXN/BC5: 4x4 blocks, 16 bytes per block (two-channel normal maps)
                let expected_size = ((width + 3) / 4) * ((height + 3) / 4) * 16;
                if self.data.len() < expected_size {
                    return Err(Error::DecompressionError(format!(
                        "DXN data too small: expected {} bytes, have {}",
                        expected_size,
                        self.data.len()
                    )));
                }
                texture2ddecoder::decode_bc5(&self.data[..expected_size], width, height, &mut pixels_u32)
                    .map_err(|e| Error::DecompressionError(format!("BC5 decode error: {}", e)))?;
            }
            DataFormat::A8R8G8B8 => {
                // Raw 32-bit ARGB - just copy (reorder to RGBA)
                if self.data.len() < width * height * 4 {
                    return Err(Error::DecompressionError("A8R8G8B8 data too small".into()));
                }
                for (i, chunk) in self.data.chunks(4).take(width * height).enumerate() {
                    // ARGB -> RGBA (as u32)
                    let a = chunk[0] as u32;
                    let r = chunk[1] as u32;
                    let g = chunk[2] as u32;
                    let b = chunk[3] as u32;
                    pixels_u32[i] = (a << 24) | (b << 16) | (g << 8) | r;
                }
            }
            DataFormat::A8B8G8R8 => {
                // Raw 32-bit ABGR - reorder to RGBA
                if self.data.len() < width * height * 4 {
                    return Err(Error::DecompressionError("A8B8G8R8 data too small".into()));
                }
                for (i, chunk) in self.data.chunks(4).take(width * height).enumerate() {
                    // ABGR -> RGBA (as u32)
                    let a = chunk[0] as u32;
                    let b = chunk[1] as u32;
                    let g = chunk[2] as u32;
                    let r = chunk[3] as u32;
                    pixels_u32[i] = (a << 24) | (b << 16) | (g << 8) | r;
                }
            }
            DataFormat::A8 => {
                // 8-bit alpha only - expand to grayscale RGBA
                if self.data.len() < width * height {
                    return Err(Error::DecompressionError("A8 data too small".into()));
                }
                for (i, &a) in self.data.iter().take(width * height).enumerate() {
                    let v = a as u32;
                    pixels_u32[i] = (v << 24) | (v << 16) | (v << 8) | v;
                }
            }
            _ => {
                return Err(Error::UnsupportedFormat(self.info.data_format));
            }
        }

        // Convert u32 pixels (BGRA format from texture2ddecoder) to RGBA bytes
        let mut rgba = Vec::with_capacity(width * height * 4);
        for pixel in pixels_u32 {
            let b = (pixel & 0xFF) as u8;
            let g = ((pixel >> 8) & 0xFF) as u8;
            let r = ((pixel >> 16) & 0xFF) as u8;
            let a = ((pixel >> 24) & 0xFF) as u8;
            rgba.push(r);
            rgba.push(g);
            rgba.push(b);
            rgba.push(a);
        }

        Ok(DecodedTexture {
            width: self.info.width,
            height: self.info.height,
            pixels: rgba,
        })
    }
}

