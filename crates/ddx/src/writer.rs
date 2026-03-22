//! DDX/DDS file writer.
//!
//! Writes textures in standard DDS format (Definitive Edition compatible).

use alloc::vec::Vec;

use crate::Result;
use crate::format::DataFormat;
use crate::reader::{DdxTexture, TextureInfo};

/// DDS file magic number "DDS " (0x20534444 in little-endian)
const DDS_MAGIC: u32 = 0x20534444;

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
const DDSCAPS_MIPMAP: u32 = 0x400000;
const DDSCAPS_COMPLEX: u32 = 0x8;

/// DDX/DDS file writer.
pub struct Writer;

impl Writer {
    /// Write a DDX texture as a standard DDS file.
    pub fn write(texture: &DdxTexture) -> Result<Vec<u8>> {
        texture.to_bytes()
    }
}

impl DdxTexture {
    /// Write texture as bytes (standard DDS format).
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        self.to_dds()
    }

    /// Write texture as a standard DDS file.
    pub fn to_dds(&self) -> Result<Vec<u8>> {
        let mut output = Vec::new();
        self.write_dds(&mut output)?;
        Ok(output)
    }

    /// Write texture as DDS to a `Vec<u8>`.
    pub fn write_dds(&self, out: &mut Vec<u8>) -> Result<()> {
        // Magic
        out.extend_from_slice(&DDS_MAGIC.to_le_bytes());

        // DDS_HEADER (124 bytes)
        out.extend_from_slice(&124u32.to_le_bytes()); // dwSize

        // Flags
        let mut flags = DDSD_CAPS | DDSD_HEIGHT | DDSD_WIDTH | DDSD_PIXELFORMAT;
        if self.info.num_mip_levels > 1 {
            flags |= DDSD_MIPMAPCOUNT;
        }
        if self.info.data_format.is_dxt() {
            flags |= DDSD_LINEARSIZE;
        }
        out.extend_from_slice(&flags.to_le_bytes());

        out.extend_from_slice(&self.info.height.to_le_bytes());
        out.extend_from_slice(&self.info.width.to_le_bytes());

        let linear_size =
            calculate_linear_size(self.info.width, self.info.height, self.info.data_format);
        out.extend_from_slice(&linear_size.to_le_bytes());

        out.extend_from_slice(&0u32.to_le_bytes()); // dwDepth
        out.extend_from_slice(&self.info.num_mip_levels.to_le_bytes());

        // dwReserved1[11]
        for _ in 0..11 {
            out.extend_from_slice(&0u32.to_le_bytes());
        }

        // DDS_PIXELFORMAT (32 bytes)
        write_pixel_format(out, &self.info)?;

        // dwCaps
        let mut caps = DDSCAPS_TEXTURE;
        if self.info.num_mip_levels > 1 {
            caps |= DDSCAPS_MIPMAP | DDSCAPS_COMPLEX;
        }
        out.extend_from_slice(&caps.to_le_bytes());

        out.extend_from_slice(&0u32.to_le_bytes()); // dwCaps2
        out.extend_from_slice(&0u32.to_le_bytes()); // dwCaps3
        out.extend_from_slice(&0u32.to_le_bytes()); // dwCaps4
        out.extend_from_slice(&0u32.to_le_bytes()); // dwReserved2

        // Texture data
        out.extend_from_slice(&self.data);

        Ok(())
    }
}

/// Calculate linear size for DDS header.
fn calculate_linear_size(width: u32, height: u32, format: DataFormat) -> u32 {
    if format.is_dxt() {
        // Block-compressed: ((width+3)/4) * ((height+3)/4) * block_size
        let block_width = width.div_ceil(4);
        let block_height = height.div_ceil(4);
        let block_size = format.dxt_block_size() as u32;
        block_width * block_height * block_size
    } else {
        // Uncompressed: width * height * bytes_per_pixel
        let bpp = format.bits_per_pixel();
        width * height * bpp / 8
    }
}

/// Write DDS_PIXELFORMAT structure.
fn write_pixel_format(out: &mut Vec<u8>, info: &TextureInfo) -> Result<()> {
    out.extend_from_slice(&32u32.to_le_bytes()); // dwSize

    match info.data_format {
        DataFormat::Dxt1 => {
            out.extend_from_slice(&DDPF_FOURCC.to_le_bytes());
            out.extend_from_slice(b"DXT1");
            out.extend_from_slice(&[0u8; 20]); // 5 zero DWORDs
        }
        DataFormat::Dxt3 => {
            out.extend_from_slice(&DDPF_FOURCC.to_le_bytes());
            out.extend_from_slice(b"DXT3");
            out.extend_from_slice(&[0u8; 20]);
        }
        DataFormat::Dxt5 | DataFormat::Dxt5N | DataFormat::Dxt5Y | DataFormat::Dxt5H => {
            out.extend_from_slice(&DDPF_FOURCC.to_le_bytes());
            out.extend_from_slice(b"DXT5");
            out.extend_from_slice(&[0u8; 20]);
        }
        DataFormat::Dxn => {
            out.extend_from_slice(&DDPF_FOURCC.to_le_bytes());
            out.extend_from_slice(b"ATI2");
            out.extend_from_slice(&[0u8; 20]);
        }
        _ => {
            let mut pf_flags = DDPF_RGB;
            if info.has_alpha {
                pf_flags |= DDPF_ALPHAPIXELS;
            }
            out.extend_from_slice(&pf_flags.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes()); // dwFourCC
            out.extend_from_slice(&32u32.to_le_bytes()); // dwRGBBitCount
            out.extend_from_slice(&0x00FF0000u32.to_le_bytes()); // R mask
            out.extend_from_slice(&0x0000FF00u32.to_le_bytes()); // G mask
            out.extend_from_slice(&0x000000FFu32.to_le_bytes()); // B mask
            out.extend_from_slice(&0xFF000000u32.to_le_bytes()); // A mask
        }
    }

    Ok(())
}
