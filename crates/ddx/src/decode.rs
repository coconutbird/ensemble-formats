//! DDX texture decoding to RGBA pixels.
//!
//! Converts compressed texture data (DXT1, DXT5, DXN, etc.) to RGBA8 format.

use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

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
        let texture_data = &self.data;

        // Allocate output buffer
        let mut pixels_u32 = vec![0u32; width * height];

        // Decode based on format
        match self.info.data_format {
            DataFormat::Dxt1 => {
                // DXT1/BC1: 4x4 blocks, 8 bytes per block
                let expected_size = width.div_ceil(4) * height.div_ceil(4) * 8;
                if texture_data.len() < expected_size {
                    return Err(Error::DecompressionError(format!(
                        "DXT1 data too small: expected {} bytes, have {}",
                        expected_size,
                        texture_data.len()
                    )));
                }
                texture2ddecoder::decode_bc1(
                    &texture_data[..expected_size],
                    width,
                    height,
                    &mut pixels_u32,
                )
                .map_err(|e| Error::DecompressionError(format!("BC1 decode error: {}", e)))?;
            }
            DataFormat::Dxt3 => {
                // DXT3/BC2: 4x4 blocks, 16 bytes per block
                let expected_size = width.div_ceil(4) * height.div_ceil(4) * 16;
                if texture_data.len() < expected_size {
                    return Err(Error::DecompressionError(format!(
                        "DXT3 data too small: expected {} bytes, have {}",
                        expected_size,
                        texture_data.len()
                    )));
                }
                texture2ddecoder::decode_bc2(
                    &texture_data[..expected_size],
                    width,
                    height,
                    &mut pixels_u32,
                )
                .map_err(|e| Error::DecompressionError(format!("BC2 decode error: {}", e)))?;
            }
            DataFormat::Dxt5 | DataFormat::Dxt5N | DataFormat::Dxt5Y | DataFormat::Dxt5H => {
                // DXT5/BC3: 4x4 blocks, 16 bytes per block
                let expected_size = width.div_ceil(4) * height.div_ceil(4) * 16;
                if texture_data.len() < expected_size {
                    return Err(Error::DecompressionError(format!(
                        "DXT5 data too small: expected {} bytes, have {}",
                        expected_size,
                        texture_data.len()
                    )));
                }
                texture2ddecoder::decode_bc3(
                    &texture_data[..expected_size],
                    width,
                    height,
                    &mut pixels_u32,
                )
                .map_err(|e| Error::DecompressionError(format!("BC3 decode error: {}", e)))?;
            }
            DataFormat::Dxn => {
                // DXN/BC5: 4x4 blocks, 16 bytes per block (two-channel normal maps)
                let expected_size = width.div_ceil(4) * height.div_ceil(4) * 16;
                if texture_data.len() < expected_size {
                    return Err(Error::DecompressionError(format!(
                        "DXN data too small: expected {} bytes, have {}",
                        expected_size,
                        texture_data.len()
                    )));
                }
                texture2ddecoder::decode_bc5(
                    &texture_data[..expected_size],
                    width,
                    height,
                    &mut pixels_u32,
                )
                .map_err(|e| Error::DecompressionError(format!("BC5 decode error: {}", e)))?;
            }
            DataFormat::Bc7 => {
                // BC7: 4x4 blocks, 16 bytes per block
                let expected_size = width.div_ceil(4) * height.div_ceil(4) * 16;
                if texture_data.len() < expected_size {
                    return Err(Error::DecompressionError(format!(
                        "BC7 data too small: expected {} bytes, have {}",
                        expected_size,
                        texture_data.len()
                    )));
                }
                texture2ddecoder::decode_bc7(
                    &texture_data[..expected_size],
                    width,
                    height,
                    &mut pixels_u32,
                )
                .map_err(|e| Error::DecompressionError(format!("BC7 decode error: {}", e)))?;
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

#[cfg(test)]
mod tests {
    use alloc::vec;
    use alloc::vec::Vec;

    use crate::{DataFormat, DdxTexture};

    fn push_u32(bytes: &mut Vec<u8>, value: u32) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn solid_white_bc7_dds() -> Vec<u8> {
        let mut bytes = Vec::with_capacity(164);
        bytes.extend_from_slice(b"DDS ");
        push_u32(&mut bytes, 124); // DDS_HEADER size
        push_u32(&mut bytes, 0x000A_1007); // required flags + linear size
        push_u32(&mut bytes, 4); // height
        push_u32(&mut bytes, 4); // width
        push_u32(&mut bytes, 16); // linear size
        push_u32(&mut bytes, 0); // depth
        push_u32(&mut bytes, 1); // mip count
        for _ in 0..11 {
            push_u32(&mut bytes, 0);
        }
        push_u32(&mut bytes, 32); // DDS_PIXELFORMAT size
        push_u32(&mut bytes, 4); // DDPF_FOURCC
        bytes.extend_from_slice(b"DX10");
        for _ in 0..5 {
            push_u32(&mut bytes, 0);
        }
        push_u32(&mut bytes, 0x1000); // DDSCAPS_TEXTURE
        for _ in 0..4 {
            push_u32(&mut bytes, 0);
        }
        push_u32(&mut bytes, 98); // DXGI_FORMAT_BC7_UNORM
        push_u32(&mut bytes, 3); // D3D10_RESOURCE_DIMENSION_TEXTURE2D
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 1); // array size
        push_u32(&mut bytes, 0);

        // BC7 mode 6 with identical 255 endpoints and zero indices.
        bytes.extend_from_slice(&[
            0xC0, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x01, 0, 0, 0, 0, 0, 0, 0,
        ]);
        bytes
    }

    #[test]
    fn dx10_bc7_is_identified_and_decoded_as_bc7() {
        let texture =
            DdxTexture::from_bytes(&solid_white_bc7_dds()).expect("synthetic BC7 DDS must parse");
        assert_eq!(texture.info.data_format, DataFormat::Bc7);

        let decoded = texture
            .decode_to_rgba()
            .expect("synthetic BC7 DDS must decode");
        assert_eq!(decoded.width, 4);
        assert_eq!(decoded.height, 4);
        assert_eq!(decoded.pixels, vec![255; 4 * 4 * 4]);

        let serialized = texture.to_dds().expect("BC7 DDS must serialize");
        let reparsed = DdxTexture::from_bytes(&serialized).expect("serialized BC7 DDS must parse");
        assert_eq!(reparsed.info.data_format, DataFormat::Bc7);
        assert_eq!(reparsed.data, texture.data);
    }
}
