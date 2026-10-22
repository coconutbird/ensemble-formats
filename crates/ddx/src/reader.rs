//! DDX file reader.
//!
//! DDX files come in two variants:
//! 1. Xbox 360 (original): ECF container with custom header and deflate-compressed mips
//! 2. Definitive Edition: Standard DDS files with .ddx extension

use alloc::format;
use alloc::vec::Vec;
use nostdio::{Cursor, ReadBe, ReadLe};

use crate::dds::{D3DFMT_A16B16G16R16F, DDS_MAGIC, DdsHeader};
use crate::format::DataFormat;
use crate::header::{
    DDX_ECF_FILE_ID, DDX_HEADER_CHUNK_ID, DDX_MIP0_CHUNK_ID, DDX_MIPCHAIN_CHUNK_ID, DdxHeader,
    Platform, ResourceType,
};
use crate::{Error, Result};

/// Read a little-endian u32 from a byte slice at the given offset.
#[inline]
fn le_u32(data: &[u8], offset: usize) -> Result<u32> {
    let mut cursor = Cursor::new(data.get(offset..).ok_or(Error::HeaderTooShort {
        expected: offset.saturating_add(4),
        actual: data.len(),
    })?);
    Ok(cursor.read_u32_le()?)
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
    ///
    /// [`DdxTexture::subresource_layout`] gives the order of the faces and mips.
    pub data: Vec<u8>,
    /// Header of the source container.
    pub source: TextureSource,
}

/// Header of the container that a [`DdxTexture`] comes from.
#[derive(Debug, Clone)]
pub enum TextureSource {
    /// Standard DDS file (Definitive Edition).
    Dds(DdsHeader),
    /// ECF-wrapped DDX file (Xbox 360).
    Ddx(DdxHeader),
}

/// DDX file reader.
pub struct Reader;

impl Reader {
    /// Read a DDX texture from a byte slice.
    ///
    /// Automatically detects whether the file is a standard DDS file
    /// (Definitive Edition) or an ECF-wrapped DDX file (Xbox 360 original).
    ///
    /// # Errors
    ///
    /// Returns an error if the DDS or ECF-wrapped DDX input is truncated,
    /// invalid, unsupported, or cannot be decompressed.
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
    ///
    /// # Errors
    ///
    /// Returns an error if the DDS or ECF-wrapped DDX input is truncated,
    /// invalid, unsupported, or cannot be decompressed.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < 4 {
            return Err(Error::DecompressionError("File too small".into()));
        }

        let magic = le_u32(data, 0)?;

        if magic == DDS_MAGIC {
            Self::from_dds(data)
        } else {
            Self::from_ecf(data)
        }
    }

    /// Parse a standard DDS file.
    fn from_dds(data: &[u8]) -> Result<Self> {
        let header = DdsHeader::from_bytes(data)?;
        let pixel_format = header.pixel_format;
        let four_cc = pixel_format.four_cc;
        let a_mask = pixel_format.a_mask;

        // Check for DX10 extended header
        let (data_format, data_start, has_alpha) = if pixel_format.has_four_cc()
            && &four_cc == b"DX10"
        {
            if data.len() < 148 {
                return Err(Error::DecompressionError(
                    "DDS file too small for DX10 header".into(),
                ));
            }
            // DDS_HEADER_DXT10 at offset 128 (20 bytes)
            let dxgi_format = le_u32(data, 128)?;

            let format = match dxgi_format {
                71 | 72 => DataFormat::Dxt1,     // BC1
                74 | 75 => DataFormat::Dxt3,     // BC2
                77 | 78 => DataFormat::Dxt5,     // BC3
                80 | 81 => DataFormat::A8,       // BC4
                83 | 84 => DataFormat::Dxn,      // BC5
                98 | 99 => DataFormat::Bc7,      // BC7 UNORM / UNORM_SRGB
                28 | 87 => DataFormat::A8R8G8B8, // RGBA8 / BGRA8 UNORM
                _ => {
                    return Err(Error::DecompressionError(format!(
                        "Unsupported DXGI format: {dxgi_format}"
                    )));
                }
            };
            let has_alpha = matches!(
                format,
                DataFormat::Dxt3 | DataFormat::Dxt5 | DataFormat::Bc7 | DataFormat::A8R8G8B8
            );
            (format, 4 + 124 + 20, has_alpha)
        } else if pixel_format.has_four_cc() {
            let format = match &four_cc {
                b"DXT1" => DataFormat::Dxt1,
                b"DXT3" => DataFormat::Dxt3,
                b"DXT5" => DataFormat::Dxt5,
                b"ATI2" | b"BC5U" => DataFormat::Dxn,
                _ if pixel_format.four_cc_code() == D3DFMT_A16B16G16R16F => {
                    DataFormat::A16B16G16R16F
                }
                _ => DataFormat::A8R8G8B8,
            };
            let has_alpha = a_mask != 0 || matches!(format, DataFormat::Dxt3 | DataFormat::Dxt5);
            (format, 4 + 124, has_alpha)
        } else {
            (DataFormat::A8R8G8B8, 4 + 124, a_mask != 0)
        };

        let resource_type = if header.is_complete_cubemap() {
            ResourceType::CubeMap
        } else {
            ResourceType::RegularMap
        };

        let texture_data = data[data_start..].to_vec();

        let info = TextureInfo {
            width: header.width,
            height: header.height,
            num_mip_levels: header.mip_levels(),
            data_format,
            resource_type,
            has_alpha,
            platform: Platform::None,
            hdr_scale: 1.0,
        };

        Ok(Self {
            info,
            data: texture_data,
            source: TextureSource::Dds(header),
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
            source: TextureSource::Ddx(header),
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
    let mut cursor = Cursor::new(*data);
    let comp_size = usize::try_from(cursor.read_u32_be()?)
        .map_err(|_| Error::SizeOverflow("compressed mip size"))?;

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
        .map_err(|e| Error::DecompressionError(format!("Deflate error: {e:?}")))?;
    output.extend_from_slice(&decompressed);

    // Advance cursor past this mip
    *data = &data[4 + comp_size..];

    Ok(())
}
