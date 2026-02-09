//! DDX data format definitions.

/// DDX data format enumeration.
///
/// These map to the eDDXDataFormat enum from the original source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum DataFormat {
    Invalid = 0,
    A8R8G8B8 = 1,
    A8B8G8R8 = 2,
    A8 = 3,
    /// DXT1 or DXT1A (1-bit alpha)
    Dxt1 = 4,
    /// DXT3 - explicit 4-bit alpha
    Dxt3 = 5,
    /// DXT5 - block alpha
    Dxt5 = 6,
    /// Swizzled normal map
    Dxt5N = 7,
    /// Luma/chroma DXT5, alpha is in red
    Dxt5Y = 8,
    /// DXN normal map (BC5)
    Dxn = 9,
    // 10 and 11 are unused
    /// HDR, alpha is intensity
    Dxt5H = 12,
    /// 16-bit float RGBA
    A16B16G16R16F = 13,
    /// Custom quantized DXT1
    Dxt1Q = 14,
    /// Custom quantized DXT5
    Dxt5Q = 15,
    /// Custom quantized DXT5 HDR
    Dxt5HQ = 16,
    /// Custom quantized DXN
    DxnQ = 17,
    /// Custom quantized DXT5Y
    Dxt5YQ = 18,
}

impl DataFormat {
    /// Parse from u32 value.
    pub fn from_u32(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Invalid),
            1 => Some(Self::A8R8G8B8),
            2 => Some(Self::A8B8G8R8),
            3 => Some(Self::A8),
            4 => Some(Self::Dxt1),
            5 => Some(Self::Dxt3),
            6 => Some(Self::Dxt5),
            7 => Some(Self::Dxt5N),
            8 => Some(Self::Dxt5Y),
            9 => Some(Self::Dxn),
            12 => Some(Self::Dxt5H),
            13 => Some(Self::A16B16G16R16F),
            14 => Some(Self::Dxt1Q),
            15 => Some(Self::Dxt5Q),
            16 => Some(Self::Dxt5HQ),
            17 => Some(Self::DxnQ),
            18 => Some(Self::Dxt5YQ),
            _ => None,
        }
    }

    /// Returns true if this is a DXT-compressed format.
    pub fn is_dxt(&self) -> bool {
        matches!(
            self,
            Self::Dxt1
                | Self::Dxt3
                | Self::Dxt5
                | Self::Dxt5N
                | Self::Dxt5Y
                | Self::Dxn
                | Self::Dxt5H
                | Self::Dxt1Q
                | Self::Dxt5Q
                | Self::Dxt5HQ
                | Self::DxnQ
                | Self::Dxt5YQ
        )
    }

    /// Returns true if this is a custom quantized format.
    pub fn is_dxtq(&self) -> bool {
        matches!(
            self,
            Self::Dxt1Q | Self::Dxt5Q | Self::Dxt5HQ | Self::DxnQ | Self::Dxt5YQ
        )
    }

    /// Returns the DXT block size in bytes (0 for non-DXT formats).
    pub fn dxt_block_size(&self) -> usize {
        match self {
            Self::Dxt1 | Self::Dxt1Q => 8,
            Self::Dxt3
            | Self::Dxt5
            | Self::Dxt5Y
            | Self::Dxt5N
            | Self::Dxt5H
            | Self::Dxn
            | Self::Dxt5Q
            | Self::Dxt5HQ
            | Self::Dxt5YQ
            | Self::DxnQ => 16,
            _ => 0,
        }
    }

    /// Returns bits per pixel for this format.
    pub fn bits_per_pixel(&self) -> u32 {
        match self {
            Self::A16B16G16R16F => 64,
            Self::A8R8G8B8 | Self::A8B8G8R8 => 32,
            Self::A8 => 8,
            Self::Dxt1 => 4,
            Self::Dxt3 | Self::Dxt5 | Self::Dxt5Y | Self::Dxt5N | Self::Dxt5H | Self::Dxn => 8,
            _ => 0,
        }
    }

    /// Returns true if format has alpha channel (DXT1 not counted).
    pub fn has_alpha(&self) -> bool {
        matches!(
            self,
            Self::A8R8G8B8
                | Self::A8B8G8R8
                | Self::A8
                | Self::A16B16G16R16F
                | Self::Dxt3
                | Self::Dxt5
                | Self::Dxt5Q
                | Self::Dxt5YQ
        )
    }

    /// Returns true if this is an HDR format.
    pub fn is_hdr(&self) -> bool {
        matches!(self, Self::A16B16G16R16F | Self::Dxt5H | Self::Dxt5HQ)
    }

    /// Returns true if this format has a fixed size (not variable like DXTQ).
    pub fn is_fixed_size(&self) -> bool {
        !self.is_dxtq()
    }
}
