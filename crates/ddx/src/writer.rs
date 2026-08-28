//! DDX/DDS file writer.
//!
//! Writes textures in standard DDS format (Definitive Edition compatible).

use alloc::vec::Vec;

use nostdio::{Write, WriteLe};

use crate::Result;
use crate::format::DataFormat;
use crate::reader::{DdxTexture, TextureInfo};

/// DDS file magic number "DDS " (0x20534444 in little-endian)
const DDS_MAGIC: u32 = 0x2053_4444;

// DDS header flags
const DDSD_CAPS: u32 = 0x1;
const DDSD_HEIGHT: u32 = 0x2;
const DDSD_WIDTH: u32 = 0x4;
const DDSD_PIXELFORMAT: u32 = 0x1000;
const DDSD_MIPMAPCOUNT: u32 = 0x20000;
const DDSD_LINEARSIZE: u32 = 0x80000;

// DDS pixel format flags
const DDPF_ALPHAPIXELS: u32 = 0x1;
const DDPF_FOURCC: u32 = 0x4;
const DDPF_RGB: u32 = 0x40;

// DDS caps flags
const DDSCAPS_TEXTURE: u32 = 0x1000;
const DDSCAPS_MIPMAP: u32 = 0x0040_0000;
const DDSCAPS_COMPLEX: u32 = 0x8;

/// DDX/DDS file writer.
pub struct Writer;

impl Writer {
    /// Write a DDX texture as a standard DDS file.
    ///
    /// # Errors
    ///
    /// Returns an error if a calculated size exceeds the DDS format or the
    /// output cursor cannot accept all bytes.
    pub fn write(texture: &DdxTexture) -> Result<Vec<u8>> {
        texture.to_bytes()
    }
}

impl DdxTexture {
    /// Write texture as bytes (standard DDS format).
    ///
    /// # Errors
    ///
    /// Returns an error if a calculated size exceeds the DDS format or the
    /// output cursor cannot accept all bytes.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        self.to_dds()
    }

    /// Write texture as a standard DDS file.
    ///
    /// # Errors
    ///
    /// Returns an error if a calculated size exceeds the DDS format or the
    /// output cursor cannot accept all bytes.
    pub fn to_dds(&self) -> Result<Vec<u8>> {
        let mut output = Vec::new();
        self.write_dds(&mut output)?;
        Ok(output)
    }

    /// Write texture as DDS to a `Vec<u8>`.
    ///
    /// # Errors
    ///
    /// Returns an error if a calculated size exceeds the DDS format or the
    /// output cursor cannot accept all bytes.
    pub fn write_dds(&self, out: &mut Vec<u8>) -> Result<()> {
        // Magic
        out.write_u32_le(DDS_MAGIC)?;

        // DDS_HEADER (124 bytes)
        out.write_u32_le(124)?; // dwSize

        // Flags
        let mut flags = DDSD_CAPS | DDSD_HEIGHT | DDSD_WIDTH | DDSD_PIXELFORMAT;
        if self.info.num_mip_levels > 1 {
            flags |= DDSD_MIPMAPCOUNT;
        }
        if self.info.data_format.is_dxt() {
            flags |= DDSD_LINEARSIZE;
        }
        out.write_u32_le(flags)?;

        out.write_u32_le(self.info.height)?;
        out.write_u32_le(self.info.width)?;

        let linear_size =
            calculate_linear_size(self.info.width, self.info.height, self.info.data_format)?;
        out.write_u32_le(linear_size)?;

        out.write_u32_le(0)?; // dwDepth
        out.write_u32_le(self.info.num_mip_levels)?;

        // dwReserved1[11]
        for _ in 0..11 {
            out.write_u32_le(0)?;
        }

        // DDS_PIXELFORMAT (32 bytes)
        write_pixel_format(out, &self.info)?;

        // dwCaps
        let mut caps = DDSCAPS_TEXTURE;
        if self.info.num_mip_levels > 1 {
            caps |= DDSCAPS_MIPMAP | DDSCAPS_COMPLEX;
        }
        out.write_u32_le(caps)?;

        out.write_u32_le(0)?; // dwCaps2
        out.write_u32_le(0)?; // dwCaps3
        out.write_u32_le(0)?; // dwCaps4
        out.write_u32_le(0)?; // dwReserved2

        if self.info.data_format == DataFormat::Bc7 {
            // DDS_HEADER_DXT10. DDX doesn't preserve the sRGB variant, so
            // serialize as BC7_UNORM and let the consumer choose color space.
            out.write_u32_le(98)?; // dxgiFormat = BC7_UNORM
            out.write_u32_le(3)?; // resourceDimension = TEXTURE2D
            out.write_u32_le(0)?; // miscFlag
            out.write_u32_le(1)?; // arraySize
            out.write_u32_le(0)?; // miscFlags2
        }

        // Texture data
        out.extend_from_slice(&self.data);

        Ok(())
    }
}

/// Calculate linear size for DDS header.
fn calculate_linear_size(width: u32, height: u32, format: DataFormat) -> Result<u32> {
    if format.is_dxt() {
        // Block-compressed: ((width+3)/4) * ((height+3)/4) * block_size
        let block_width = width.div_ceil(4);
        let block_height = height.div_ceil(4);
        block_width
            .checked_mul(block_height)
            .and_then(|blocks| blocks.checked_mul(format.dxt_block_size()))
            .ok_or(crate::Error::SizeOverflow("linear texture size"))
    } else {
        // Uncompressed: width * height * bytes_per_pixel
        let bpp = format.bits_per_pixel();
        width
            .checked_mul(height)
            .and_then(|pixels| pixels.checked_mul(bpp))
            .map(|bits| bits / 8)
            .ok_or(crate::Error::SizeOverflow("linear texture size"))
    }
}

/// Write `DDS_PIXELFORMAT` structure.
fn write_pixel_format(out: &mut Vec<u8>, info: &TextureInfo) -> Result<()> {
    out.write_u32_le(32)?; // dwSize

    match info.data_format {
        DataFormat::Dxt1 => {
            out.write_u32_le(DDPF_FOURCC)?;
            out.write_all(b"DXT1")?;
            out.write_all(&[0u8; 20])?; // 5 zero DWORDs
        }
        DataFormat::Dxt3 => {
            out.write_u32_le(DDPF_FOURCC)?;
            out.write_all(b"DXT3")?;
            out.write_all(&[0u8; 20])?;
        }
        DataFormat::Dxt5 | DataFormat::Dxt5N | DataFormat::Dxt5Y | DataFormat::Dxt5H => {
            out.write_u32_le(DDPF_FOURCC)?;
            out.write_all(b"DXT5")?;
            out.write_all(&[0u8; 20])?;
        }
        DataFormat::Dxn => {
            out.write_u32_le(DDPF_FOURCC)?;
            out.write_all(b"ATI2")?;
            out.write_all(&[0u8; 20])?;
        }
        DataFormat::Bc7 => {
            out.write_u32_le(DDPF_FOURCC)?;
            out.write_all(b"DX10")?;
            out.write_all(&[0u8; 20])?;
        }
        _ => {
            let mut pf_flags = DDPF_RGB;
            if info.has_alpha {
                pf_flags |= DDPF_ALPHAPIXELS;
            }
            out.write_u32_le(pf_flags)?;
            out.write_u32_le(0)?; // dwFourCC
            out.write_u32_le(32)?; // dwRGBBitCount
            out.write_u32_le(0x00FF_0000)?; // R mask
            out.write_u32_le(0x0000_FF00)?; // G mask
            out.write_u32_le(0x0000_00FF)?; // B mask
            out.write_u32_le(0xFF00_0000)?; // A mask
        }
    }

    Ok(())
}
