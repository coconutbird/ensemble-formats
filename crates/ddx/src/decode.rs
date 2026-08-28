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
    ///
    /// # Errors
    ///
    /// Returns an error if dimensions overflow this platform, the base mip is
    /// truncated, the texture format is unsupported, or block decoding fails.
    pub fn decode_to_rgba(&self) -> Result<DecodedTexture> {
        let width = self.info.width as usize;
        let height = self.info.height as usize;
        let texture_data = &self.data;

        // Allocate output buffer
        let mut pixels_u32 = vec![0u32; width * height];

        // Decode based on format
        if decode_block_format(
            self.info.data_format,
            texture_data,
            width,
            height,
            &mut pixels_u32,
        )? {
            // Block-compressed formats are handled by the helper above.
        } else {
            match self.info.data_format {
                DataFormat::A8R8G8B8 => {
                    // Raw 32-bit ARGB - just copy (reorder to RGBA)
                    if self.data.len() < width * height * 4 {
                        return Err(Error::DecompressionError("A8R8G8B8 data too small".into()));
                    }
                    for (i, chunk) in self.data.chunks(4).take(width * height).enumerate() {
                        // ARGB -> RGBA (as u32)
                        let a = u32::from(chunk[0]);
                        let r = u32::from(chunk[1]);
                        let g = u32::from(chunk[2]);
                        let b = u32::from(chunk[3]);
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
                        let a = u32::from(chunk[0]);
                        let b = u32::from(chunk[1]);
                        let g = u32::from(chunk[2]);
                        let r = u32::from(chunk[3]);
                        pixels_u32[i] = (a << 24) | (b << 16) | (g << 8) | r;
                    }
                }
                DataFormat::A8 => {
                    // 8-bit alpha only - expand to grayscale RGBA
                    if self.data.len() < width * height {
                        return Err(Error::DecompressionError("A8 data too small".into()));
                    }
                    for (i, &a) in self.data.iter().take(width * height).enumerate() {
                        let v = u32::from(a);
                        pixels_u32[i] = (v << 24) | (v << 16) | (v << 8) | v;
                    }
                }
                _ => {
                    return Err(Error::UnsupportedFormat(self.info.data_format));
                }
            }
        }

        // Convert u32 pixels (BGRA format from texture2ddecoder) to RGBA bytes
        let mut rgba = Vec::with_capacity(width * height * 4);
        for pixel in pixels_u32 {
            let [b, g, r, a] = pixel.to_le_bytes();
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

fn decode_block_format(
    format: DataFormat,
    data: &[u8],
    width: usize,
    height: usize,
    pixels: &mut [u32],
) -> Result<bool> {
    let (block_size, label) = match format {
        DataFormat::Dxt1 => (8, "DXT1"),
        DataFormat::Dxt3 => (16, "DXT3"),
        DataFormat::Dxt5 | DataFormat::Dxt5N | DataFormat::Dxt5Y | DataFormat::Dxt5H => {
            (16, "DXT5")
        }
        DataFormat::Dxn => (16, "DXN"),
        DataFormat::Bc7 => (16, "BC7"),
        _ => return Ok(false),
    };
    let expected_size = width
        .div_ceil(4)
        .checked_mul(height.div_ceil(4))
        .and_then(|blocks| blocks.checked_mul(block_size))
        .ok_or(Error::SizeOverflow("decoded texture size"))?;
    let compressed = data.get(..expected_size).ok_or_else(|| {
        Error::DecompressionError(format!(
            "{label} data too small: expected {expected_size} bytes, have {}",
            data.len()
        ))
    })?;
    let result = match format {
        DataFormat::Dxt1 => texture2ddecoder::decode_bc1(compressed, width, height, pixels),
        DataFormat::Dxt3 => texture2ddecoder::decode_bc2(compressed, width, height, pixels),
        DataFormat::Dxt5 | DataFormat::Dxt5N | DataFormat::Dxt5Y | DataFormat::Dxt5H => {
            texture2ddecoder::decode_bc3(compressed, width, height, pixels)
        }
        DataFormat::Dxn => texture2ddecoder::decode_bc5(compressed, width, height, pixels),
        DataFormat::Bc7 => texture2ddecoder::decode_bc7(compressed, width, height, pixels),
        _ => return Ok(false),
    };
    result.map_err(|error| Error::DecompressionError(format!("{label} decode error: {error}")))?;
    Ok(true)
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
