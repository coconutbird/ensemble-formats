//! DDX header structures.

use crate::format::DataFormat;
use crate::{Error, Result};
use byteorder::{LittleEndian, ReadBytesExt};

/// DDX header magic number.
pub const DDX_HEADER_MAGIC: u32 = 0xDDBB7738;

/// Minimum required DDX version.
pub const DDX_MIN_REQUIRED_VERSION: u16 = 6;

/// Current DDX version.
pub const DDX_CURRENT_VERSION: u16 = 7;

/// DDX ECF file ID.
pub const DDX_ECF_FILE_ID: u32 = 0x13CF5D01;

/// DDX header chunk ID (64-bit).
pub const DDX_HEADER_CHUNK_ID: u64 = 0x1D8828C6ECAF45F2;

/// DDX mip0 chunk ID (64-bit).
pub const DDX_MIP0_CHUNK_ID: u64 = 0x3F74B8E87D2B44BF;

/// DDX mip chain chunk ID (64-bit).
pub const DDX_MIPCHAIN_CHUNK_ID: u64 = 0x46F1FD3F394348B8;

/// DDX resource type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum ResourceType {
    /// Regular 2D texture.
    RegularMap = 0,
    /// Normal map.
    NormalMap = 1,
    /// Cube map (6 faces).
    CubeMap = 2,
}

impl ResourceType {
    /// Parse from u32 value.
    pub fn from_u32(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::RegularMap),
            1 => Some(Self::NormalMap),
            2 => Some(Self::CubeMap),
            _ => None,
        }
    }
}

/// DDX platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Platform {
    /// No platform-specific data.
    None = 0,
    /// Xbox 360.
    Xbox = 1,
}

impl Platform {
    /// Parse from u8 value.
    pub fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Xbox,
            _ => Self::None,
        }
    }
}

/// DDX header flags.
pub mod flags {
    /// Texture has alpha channel.
    pub const HAS_ALPHA: u32 = 1 << 0;
}

/// DDX file header.
///
/// This corresponds to BDDXHeader from the original source.
#[derive(Debug, Clone)]
pub struct DdxHeader {
    /// Header magic (should be 0xDDBB7738).
    pub magic: u32,
    /// Size of header structure.
    pub header_size: u32,
    /// Adler32 checksum of header data after this field.
    pub header_adler32: u32,
    /// Creator version.
    pub creator_version: u16,
    /// Minimum required version to read.
    pub min_required_version: u16,
    /// Width as power of 2 (actual width = 1 << dimension_pow2[0]).
    pub width_pow2: u8,
    /// Height as power of 2 (actual height = 1 << dimension_pow2[1]).
    pub height_pow2: u8,
    /// Number of mipmap levels in the mip chain (not including mip0).
    pub mip_chain_size: u8,
    /// Target platform.
    pub platform: Platform,
    /// Data format.
    pub data_format: DataFormat,
    /// Resource type.
    pub resource_type: ResourceType,
    /// Header flags.
    pub flags: u32,
    /// HDR scale factor.
    pub hdr_scale: f32,
}

impl DdxHeader {
    /// Size of the header in bytes.
    pub const SIZE: usize = 32;

    /// Parse header from bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < Self::SIZE {
            return Err(Error::HeaderTooShort {
                expected: Self::SIZE,
                actual: data.len(),
            });
        }

        let mut cursor = std::io::Cursor::new(data);

        let magic = cursor.read_u32::<LittleEndian>()?;
        if magic != DDX_HEADER_MAGIC {
            return Err(Error::InvalidMagic(magic));
        }

        let header_size = cursor.read_u32::<LittleEndian>()?;
        let header_adler32 = cursor.read_u32::<LittleEndian>()?;
        let creator_version = cursor.read_u16::<LittleEndian>()?;
        let min_required_version = cursor.read_u16::<LittleEndian>()?;

        if min_required_version > DDX_CURRENT_VERSION {
            return Err(Error::UnsupportedVersion(
                min_required_version,
                DDX_CURRENT_VERSION,
            ));
        }

        let width_pow2 = cursor.read_u8()?;
        let height_pow2 = cursor.read_u8()?;
        let mip_chain_size = cursor.read_u8()?;
        let platform_byte = cursor.read_u8()?;
        let platform = Platform::from_u8(platform_byte);

        let data_format_raw = cursor.read_u32::<LittleEndian>()?;
        let data_format = DataFormat::from_u32(data_format_raw)
            .ok_or(Error::InvalidDataFormat(data_format_raw))?;

        let resource_type_raw = cursor.read_u32::<LittleEndian>()?;
        let resource_type = ResourceType::from_u32(resource_type_raw)
            .ok_or(Error::InvalidResourceType(resource_type_raw))?;

        let flags = cursor.read_u32::<LittleEndian>()?;
        let hdr_scale = cursor.read_f32::<LittleEndian>()?;

        Ok(Self {
            magic,
            header_size,
            header_adler32,
            creator_version,
            min_required_version,
            width_pow2,
            height_pow2,
            mip_chain_size,
            platform,
            data_format,
            resource_type,
            flags,
            hdr_scale,
        })
    }

    /// Get actual width in pixels.
    pub fn width(&self) -> u32 {
        1 << self.width_pow2
    }

    /// Get actual height in pixels.
    pub fn height(&self) -> u32 {
        1 << self.height_pow2
    }

    /// Returns true if the texture has alpha.
    pub fn has_alpha(&self) -> bool {
        (self.flags & flags::HAS_ALPHA) != 0
    }

    /// Get number of mip levels (including mip0).
    pub fn num_mip_levels(&self) -> u32 {
        1 + self.mip_chain_size as u32
    }

    /// Get number of faces (6 for cubemaps, 1 otherwise).
    pub fn num_faces(&self) -> u32 {
        if self.resource_type == ResourceType::CubeMap {
            6
        } else {
            1
        }
    }
}
