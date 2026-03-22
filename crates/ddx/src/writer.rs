//! DDX/DDS file writer.
//!
//! Writes textures in standard DDS format (Definitive Edition compatible).

use crate::Result;
use crate::format::DataFormat;
use crate::reader::{DdxTexture, TextureInfo};
use byteorder::{LittleEndian, WriteBytesExt};
use std::io::Write;

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

    /// Write texture as DDS to a writer.
    pub fn write_dds<W: Write>(&self, writer: &mut W) -> Result<()> {
        // Magic
        writer.write_u32::<LittleEndian>(DDS_MAGIC)?;

        // DDS_HEADER (124 bytes)
        writer.write_u32::<LittleEndian>(124)?; // dwSize

        // Flags
        let mut flags = DDSD_CAPS | DDSD_HEIGHT | DDSD_WIDTH | DDSD_PIXELFORMAT;
        if self.info.num_mip_levels > 1 {
            flags |= DDSD_MIPMAPCOUNT;
        }
        if self.info.data_format.is_dxt() {
            flags |= DDSD_LINEARSIZE;
        }
        writer.write_u32::<LittleEndian>(flags)?;

        writer.write_u32::<LittleEndian>(self.info.height)?; // dwHeight
        writer.write_u32::<LittleEndian>(self.info.width)?; // dwWidth

        // dwPitchOrLinearSize
        let linear_size =
            calculate_linear_size(self.info.width, self.info.height, self.info.data_format);
        writer.write_u32::<LittleEndian>(linear_size)?;

        writer.write_u32::<LittleEndian>(0)?; // dwDepth
        writer.write_u32::<LittleEndian>(self.info.num_mip_levels)?; // dwMipMapCount

        // dwReserved1[11]
        for _ in 0..11 {
            writer.write_u32::<LittleEndian>(0)?;
        }

        // DDS_PIXELFORMAT (32 bytes)
        write_pixel_format(writer, &self.info)?;

        // dwCaps
        let mut caps = DDSCAPS_TEXTURE;
        if self.info.num_mip_levels > 1 {
            caps |= DDSCAPS_MIPMAP | DDSCAPS_COMPLEX;
        }
        writer.write_u32::<LittleEndian>(caps)?;

        writer.write_u32::<LittleEndian>(0)?; // dwCaps2
        writer.write_u32::<LittleEndian>(0)?; // dwCaps3
        writer.write_u32::<LittleEndian>(0)?; // dwCaps4
        writer.write_u32::<LittleEndian>(0)?; // dwReserved2

        // Texture data
        writer.write_all(&self.data)?;

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
fn write_pixel_format<W: Write>(writer: &mut W, info: &TextureInfo) -> Result<()> {
    writer.write_u32::<LittleEndian>(32)?; // dwSize

    match info.data_format {
        DataFormat::Dxt1 => {
            writer.write_u32::<LittleEndian>(DDPF_FOURCC)?;
            writer.write_all(b"DXT1")?;
            writer.write_u32::<LittleEndian>(0)?; // dwRGBBitCount
            writer.write_u32::<LittleEndian>(0)?; // dwRBitMask
            writer.write_u32::<LittleEndian>(0)?; // dwGBitMask
            writer.write_u32::<LittleEndian>(0)?; // dwBBitMask
            writer.write_u32::<LittleEndian>(0)?; // dwABitMask
        }
        DataFormat::Dxt3 => {
            writer.write_u32::<LittleEndian>(DDPF_FOURCC)?;
            writer.write_all(b"DXT3")?;
            writer.write_u32::<LittleEndian>(0)?;
            writer.write_u32::<LittleEndian>(0)?;
            writer.write_u32::<LittleEndian>(0)?;
            writer.write_u32::<LittleEndian>(0)?;
            writer.write_u32::<LittleEndian>(0)?;
        }
        DataFormat::Dxt5 | DataFormat::Dxt5N | DataFormat::Dxt5Y | DataFormat::Dxt5H => {
            writer.write_u32::<LittleEndian>(DDPF_FOURCC)?;
            writer.write_all(b"DXT5")?;
            writer.write_u32::<LittleEndian>(0)?;
            writer.write_u32::<LittleEndian>(0)?;
            writer.write_u32::<LittleEndian>(0)?;
            writer.write_u32::<LittleEndian>(0)?;
            writer.write_u32::<LittleEndian>(0)?;
        }
        DataFormat::Dxn => {
            writer.write_u32::<LittleEndian>(DDPF_FOURCC)?;
            writer.write_all(b"ATI2")?;
            writer.write_u32::<LittleEndian>(0)?;
            writer.write_u32::<LittleEndian>(0)?;
            writer.write_u32::<LittleEndian>(0)?;
            writer.write_u32::<LittleEndian>(0)?;
            writer.write_u32::<LittleEndian>(0)?;
        }
        _ => {
            // Uncompressed ARGB (A8R8G8B8 and others)
            let mut pf_flags = DDPF_RGB;
            if info.has_alpha {
                pf_flags |= DDPF_ALPHAPIXELS;
            }
            writer.write_u32::<LittleEndian>(pf_flags)?;
            writer.write_u32::<LittleEndian>(0)?; // dwFourCC
            writer.write_u32::<LittleEndian>(32)?; // dwRGBBitCount
            writer.write_u32::<LittleEndian>(0x00FF0000)?; // R mask
            writer.write_u32::<LittleEndian>(0x0000FF00)?; // G mask
            writer.write_u32::<LittleEndian>(0x000000FF)?; // B mask
            writer.write_u32::<LittleEndian>(0xFF000000)?; // A mask
        }
    }

    Ok(())
}
