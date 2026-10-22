//! Legacy DDS file header.
//!
//! A DDS file starts with the magic `"DDS "`, a 124-byte `DDS_HEADER` and
//! the texture data. A `DX10` pixel format adds a 20-byte `DDS_HEADER_DXT10`
//! before the data.

use alloc::format;

use zerocopy::little_endian::U32;
use zerocopy::{FromBytes, Immutable, KnownLayout, Ref};

use crate::{Error, Result};

/// DDS file magic number `"DDS "` as a little-endian `u32`.
pub const DDS_MAGIC: u32 = 0x2053_4444;

/// Size of the `DDS_HEADER` structure in bytes. The magic is not included.
pub const DDS_HEADER_SIZE: u32 = 124;

/// Size of the `DDS_PIXELFORMAT` structure in bytes.
pub const DDS_PIXEL_FORMAT_SIZE: u32 = 32;

/// `DDS_HEADER` flag: the `mip_map_count` field is valid.
pub const DDSD_MIPMAPCOUNT: u32 = 0x0002_0000;

/// `DDS_PIXELFORMAT` flag: the `four_cc` field is valid.
pub const DDPF_FOURCC: u32 = 0x4;

/// `caps2` flag: the texture is a cubemap.
pub const DDSCAPS2_CUBEMAP: u32 = 0x0000_0200;

/// `caps2` flags: the cubemap flag and the flags of all six faces.
pub const DDSCAPS2_CUBEMAP_ALL_FACES: u32 = 0x0000_FE00;

/// Numeric D3D9 `FourCC` of the 64-bit `A16B16G16R16F` float format.
pub const D3DFMT_A16B16G16R16F: u32 = 113;

/// Raw on-disk DDS magic and `DDS_HEADER` (128 bytes, little-endian).
#[derive(FromBytes, KnownLayout, Immutable)]
#[repr(C)]
struct DdsHeaderRaw {
    magic: U32,
    size: U32,
    flags: U32,
    height: U32,
    width: U32,
    pitch_or_linear_size: U32,
    depth: U32,
    mip_map_count: U32,
    reserved1: [U32; 11],
    pixel_format: DdsPixelFormatRaw,
    caps: U32,
    caps2: U32,
    caps3: U32,
    caps4: U32,
    reserved2: U32,
}

/// Raw on-disk `DDS_PIXELFORMAT` (32 bytes, little-endian).
#[derive(FromBytes, KnownLayout, Immutable)]
#[repr(C)]
struct DdsPixelFormatRaw {
    size: U32,
    flags: U32,
    four_cc: [u8; 4],
    rgb_bit_count: U32,
    r_mask: U32,
    g_mask: U32,
    b_mask: U32,
    a_mask: U32,
}

/// Parsed `DDS_PIXELFORMAT` structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DdsPixelFormat {
    /// Structure size in bytes. A valid file stores [`DDS_PIXEL_FORMAT_SIZE`].
    pub size: u32,
    /// Pixel format flags, for example [`DDPF_FOURCC`].
    pub flags: u32,
    /// Four-character code, or a numeric D3D9 format code in little-endian order.
    pub four_cc: [u8; 4],
    /// Number of bits in each pixel of an RGB format.
    pub rgb_bit_count: u32,
    /// Red channel mask.
    pub r_mask: u32,
    /// Green channel mask.
    pub g_mask: u32,
    /// Blue channel mask.
    pub b_mask: u32,
    /// Alpha channel mask.
    pub a_mask: u32,
}

impl DdsPixelFormat {
    /// Returns whether the `four_cc` field is valid.
    #[must_use]
    pub fn has_four_cc(&self) -> bool {
        self.flags & DDPF_FOURCC != 0
    }

    /// Returns the `four_cc` field as a little-endian `u32`.
    ///
    /// Use this value to compare a numeric D3D9 format code, for example
    /// [`D3DFMT_A16B16G16R16F`].
    #[must_use]
    pub fn four_cc_code(&self) -> u32 {
        u32::from_le_bytes(self.four_cc)
    }
}

/// Parsed DDS `DDS_HEADER` structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DdsHeader {
    /// Header flags, for example [`DDSD_MIPMAPCOUNT`].
    pub flags: u32,
    /// Height of mip 0 in pixels.
    pub height: u32,
    /// Width of mip 0 in pixels.
    pub width: u32,
    /// Pitch or linear size of mip 0.
    pub pitch_or_linear_size: u32,
    /// Depth of a volume texture.
    pub depth: u32,
    /// Number of mip levels that the file declares.
    pub mip_map_count: u32,
    /// Pixel format.
    pub pixel_format: DdsPixelFormat,
    /// Surface capabilities.
    pub caps: u32,
    /// Cubemap and volume capabilities, for example [`DDSCAPS2_CUBEMAP`].
    pub caps2: u32,
    /// Reserved capabilities.
    pub caps3: u32,
    /// Reserved capabilities.
    pub caps4: u32,
}

impl DdsHeader {
    /// Size of the magic and the `DDS_HEADER` in bytes.
    pub const SIZE: usize = 128;

    /// Parses the magic and the `DDS_HEADER` at the start of a DDS file.
    ///
    /// # Errors
    ///
    /// Returns an error if `data` is shorter than [`DdsHeader::SIZE`], if the
    /// magic is not [`DDS_MAGIC`], or if the header size is not
    /// [`DDS_HEADER_SIZE`].
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let (raw, _): (Ref<_, DdsHeaderRaw>, _) = Ref::from_prefix(data)
            .map_err(|_| Error::DecompressionError("DDS file too small for header".into()))?;
        let magic = raw.magic.get();
        if magic != DDS_MAGIC {
            return Err(Error::InvalidDdsMagic(magic));
        }
        let header_size = raw.size.get();
        if header_size != DDS_HEADER_SIZE {
            return Err(Error::DecompressionError(format!(
                "Invalid DDS header size: {header_size}"
            )));
        }
        let pixel_format = &raw.pixel_format;
        Ok(Self {
            flags: raw.flags.get(),
            height: raw.height.get(),
            width: raw.width.get(),
            pitch_or_linear_size: raw.pitch_or_linear_size.get(),
            depth: raw.depth.get(),
            mip_map_count: raw.mip_map_count.get(),
            pixel_format: DdsPixelFormat {
                size: pixel_format.size.get(),
                flags: pixel_format.flags.get(),
                four_cc: pixel_format.four_cc,
                rgb_bit_count: pixel_format.rgb_bit_count.get(),
                r_mask: pixel_format.r_mask.get(),
                g_mask: pixel_format.g_mask.get(),
                b_mask: pixel_format.b_mask.get(),
                a_mask: pixel_format.a_mask.get(),
            },
            caps: raw.caps.get(),
            caps2: raw.caps2.get(),
            caps3: raw.caps3.get(),
            caps4: raw.caps4.get(),
        })
    }

    /// Returns the number of mip levels that a DDS reader uses.
    ///
    /// The value is `mip_map_count` if [`DDSD_MIPMAPCOUNT`] is set and the
    /// count is not zero. Otherwise the value is 1.
    #[must_use]
    pub fn mip_levels(&self) -> u32 {
        if self.flags & DDSD_MIPMAPCOUNT != 0 && self.mip_map_count > 0 {
            self.mip_map_count
        } else {
            1
        }
    }

    /// Returns whether `caps2` declares a cubemap with all six faces.
    #[must_use]
    pub fn is_complete_cubemap(&self) -> bool {
        self.caps2 & DDSCAPS2_CUBEMAP_ALL_FACES == DDSCAPS2_CUBEMAP_ALL_FACES
    }
}
