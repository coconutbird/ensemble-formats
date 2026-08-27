//! DDX file reader.
//!
//! DDX files come in two variants:
//! 1. Xbox 360 (original): ECF container with custom header and deflate-compressed mips
//! 2. Definitive Edition: Standard DDS files with .ddx extension

use alloc::format;
use alloc::vec::Vec;

use crate::format::DataFormat;
use crate::header::{
    DDX_ECF_FILE_ID, DDX_HEADER_CHUNK_ID, DDX_MIP0_CHUNK_ID, DDX_MIPCHAIN_CHUNK_ID, DdxHeader,
    Platform, ResourceType,
};
use crate::{Error, Result};

/// DDS file magic number "DDS " (0x20534444 in little-endian)
const DDS_MAGIC: u32 = 0x20534444;

/// Read a little-endian u32 from a byte slice at the given offset.
#[inline]
fn le_u32(data: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(data[off..off + 4].try_into().unwrap())
}

/// Texture information extracted from a DDX file.
#[derive(Debug, Clone)]
pub struct TextureInfo {
    /// Texture width in pixels.
    pub width: u32,
    /// Texture height in pixels.
    pub height: u32,
    /// Number of mip levels (including mip0).
    pub num_mip_levels: u32,
    /// Data format.
    pub data_format: DataFormat,
    /// Resource type.
    pub resource_type: ResourceType,
    /// Whether texture has alpha.
    pub has_alpha: bool,
    /// Platform the texture was built for.
    pub platform: Platform,
    /// HDR scale factor.
    pub hdr_scale: f32,
}

/// A parsed DDX texture.
#[derive(Debug)]
pub struct DdxTexture {
    /// Texture metadata.
    pub info: TextureInfo,
    /// Raw decompressed texture data for all mip levels and faces.
    pub data: Vec<u8>,
}

/// DDX file reader.
pub struct Reader;

impl Reader {
    /// Read a DDX texture from a byte slice.
    ///
    /// Automatically detects whether the file is a standard DDS file
    /// (Definitive Edition) or an ECF-wrapped DDX file (Xbox 360 original).
    pub fn read(data: &[u8]) -> Result<DdxTexture> {
        DdxTexture::from_bytes(data)
    }
}

impl DdxTexture {
    /// Parse a DDX file from bytes.
    ///
    /// Automatically detects whether the file is:
    /// - A standard DDS file (Definitive Edition)
    /// - An ECF-wrapped DDX file (Xbox 360 original)
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < 4 {
            return Err(Error::DecompressionError("File too small".into()));
        }

        let magic = u32::from_le_bytes(data[0..4].try_into().unwrap());

        if magic == DDS_MAGIC {
            Self::from_dds(data)
        } else {
            Self::from_ecf(data)
        }
    }

    /// Parse a standard DDS file.
    fn from_dds(data: &[u8]) -> Result<Self> {
        if data.len() < 128 {
            return Err(Error::DecompressionError(
                "DDS file too small for header".into(),
            ));
        }

        // DDS_HEADER structure (124 bytes) starts at offset 4 (after magic)
        let header_size = le_u32(data, 4);
        if header_size != 124 {
            return Err(Error::DecompressionError(format!(
                "Invalid DDS header size: {}",
                header_size
            )));
        }

        let flags = le_u32(data, 8);
        let height = le_u32(data, 12);
        let width = le_u32(data, 16);
        // _pitch_or_linear_size at 20, _depth at 24
        let mip_map_count = le_u32(data, 28);
        // reserved[11] at 32..76

        // DDS_PIXELFORMAT at offset 76 (32 bytes)
        // _pf_size at 76
        let pf_flags = le_u32(data, 80);
        let four_cc: [u8; 4] = data[84..88].try_into().unwrap();
        // _rgb_bit_count at 88, _r_mask at 92, _g_mask at 96, _b_mask at 100
        let a_mask = le_u32(data, 104);
        // _caps at 108, _caps2 at 112, _caps3 at 116, _caps4 at 120, _reserved2 at 124

        // Check for DX10 extended header
        let (data_format, data_start, has_alpha) = if pf_flags & 0x4 != 0 && &four_cc == b"DX10" {
            if data.len() < 148 {
                return Err(Error::DecompressionError(
                    "DDS file too small for DX10 header".into(),
                ));
            }
            // DDS_HEADER_DXT10 at offset 128 (20 bytes)
            let dxgi_format = le_u32(data, 128);

            let format = match dxgi_format {
                71 | 72 => DataFormat::Dxt1, // BC1
                74 | 75 => DataFormat::Dxt3, // BC2
                77 | 78 => DataFormat::Dxt5, // BC3
                80 | 81 => DataFormat::A8,   // BC4
                83 | 84 => DataFormat::Dxn,  // BC5
                98 | 99 => DataFormat::Bc7,  // BC7 UNORM / UNORM_SRGB
                28 => DataFormat::A8R8G8B8,  // R8G8B8A8_UNORM
                87 => DataFormat::A8R8G8B8,  // B8G8R8A8_UNORM
                _ => {
                    return Err(Error::DecompressionError(format!(
                        "Unsupported DXGI format: {}",
                        dxgi_format
                    )));
                }
            };
            let has_alpha = matches!(
                format,
                DataFormat::Dxt3 | DataFormat::Dxt5 | DataFormat::Bc7 | DataFormat::A8R8G8B8
            );
            (format, 4 + 124 + 20, has_alpha)
        } else if pf_flags & 0x4 != 0 {
            let format = match &four_cc {
                b"DXT1" => DataFormat::Dxt1,
                b"DXT3" => DataFormat::Dxt3,
                b"DXT5" => DataFormat::Dxt5,
                b"ATI2" | b"BC5U" => DataFormat::Dxn,
                _ => DataFormat::A8R8G8B8,
            };
            let has_alpha = a_mask != 0 || matches!(format, DataFormat::Dxt3 | DataFormat::Dxt5);
            (format, 4 + 124, has_alpha)
        } else {
            (DataFormat::A8R8G8B8, 4 + 124, a_mask != 0)
        };

        let num_mip_levels = if flags & 0x20000 != 0 && mip_map_count > 0 {
            mip_map_count
        } else {
            1
        };

        let texture_data = data[data_start..].to_vec();

        let info = TextureInfo {
            width,
            height,
            num_mip_levels,
            data_format,
            resource_type: ResourceType::RegularMap,
            has_alpha,
            platform: Platform::None,
            hdr_scale: 1.0,
        };

        Ok(Self {
            info,
            data: texture_data,
        })
    }

    /// Parse an ECF-wrapped DDX file (Xbox 360 format).
    fn from_ecf(data: &[u8]) -> Result<Self> {
        let ecf_reader = ecf::Reader::new(data)?;

        // Verify ECF file ID
        if ecf_reader.header().id != DDX_ECF_FILE_ID {
            return Err(Error::InvalidEcfFileId(ecf_reader.header().id));
        }

        // Find and parse header chunk (ECF chunk IDs are u64)
        let header_data = ecf_reader
            .chunk_data_by_id(DDX_HEADER_CHUNK_ID)
            .map_err(|_| Error::MissingHeaderChunk)?;
        let header = DdxHeader::from_bytes(&header_data)?;

        // Find mip0 data chunk
        let mip0_data = ecf_reader
            .chunk_data_by_id(DDX_MIP0_CHUNK_ID)
            .map_err(|_| Error::MissingMip0Chunk)?;

        // Find optional mip chain chunk
        let mipchain_data = ecf_reader.chunk_data_by_id(DDX_MIPCHAIN_CHUNK_ID).ok();

        // Decompress texture data
        let texture_data = decompress_texture(&header, &mip0_data, mipchain_data.as_deref())?;

        let info = TextureInfo {
            width: header.width(),
            height: header.height(),
            num_mip_levels: header.num_mip_levels(),
            data_format: header.data_format,
            resource_type: header.resource_type,
            has_alpha: header.has_alpha(),
            platform: header.platform,
            hdr_scale: header.hdr_scale,
        };

        Ok(Self {
            info,
            data: texture_data,
        })
    }
}

/// Decompress texture data from mip0 and optional mip chain.
fn decompress_texture(
    header: &DdxHeader,
    mip0_data: &[u8],
    mipchain_data: Option<&[u8]>,
) -> Result<Vec<u8>> {
    let mut output = Vec::new();

    // For DXTQ formats, data is not deflate-compressed, just stored with size prefix
    if header.data_format.is_dxtq() {
        // DXTQ formats use custom compression - for now just return raw data
        output.extend_from_slice(mip0_data);
        return Ok(output);
    }

    // Decompress mip0 for each face
    let mut mip0_cursor = mip0_data;
    for _face in 0..header.num_faces() {
        decompress_mip_data(&mut mip0_cursor, &mut output)?;
    }

    // Decompress mip chain if present and requested
    if let Some(mipchain) = mipchain_data
        && header.mip_chain_size > 0
    {
        let mut mipchain_cursor = mipchain;
        for _face in 0..header.num_faces() {
            for _mip in 0..header.mip_chain_size {
                decompress_mip_data(&mut mipchain_cursor, &mut output)?;
            }
        }
    }

    Ok(output)
}

/// Decompress a single mip level.
///
/// Format: 4-byte big-endian compressed size, then deflate data.
fn decompress_mip_data(data: &mut &[u8], output: &mut Vec<u8>) -> Result<()> {
    if data.len() < 4 {
        return Err(Error::DecompressionError(
            "Mip data too short for size header".into(),
        ));
    }

    // Read 4-byte big-endian compressed size
    let comp_size = u32::from_be_bytes(data[0..4].try_into().unwrap()) as usize;

    if comp_size == 0 {
        return Err(Error::DecompressionError("Compressed size is 0".into()));
    }

    if data.len() < 4 + comp_size {
        return Err(Error::DecompressionError(format!(
            "Mip data too short: need {} bytes, have {}",
            4 + comp_size,
            data.len()
        )));
    }

    // Decompress using deflate (raw deflate, no zlib/gzip wrapper)
    let compressed = &data[4..4 + comp_size];
    let decompressed = miniz_oxide::inflate::decompress_to_vec(compressed)
        .map_err(|e| Error::DecompressionError(format!("Deflate error: {:?}", e)))?;
    output.extend_from_slice(&decompressed);

    // Advance cursor past this mip
    *data = &data[4 + comp_size..];

    Ok(())
}
