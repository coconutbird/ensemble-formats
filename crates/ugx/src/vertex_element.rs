//! Vertex element types and unpacking.
//!
//! This module handles the various packed vertex formats used in UGX files.

use byteorder::{LittleEndian, ReadBytesExt};
use half::f16;
use std::io::Read;

use crate::error::{Error, Result};

/// Vertex element data types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum VertexElementType {
    /// Ignored/unused element.
    #[default]
    Ignore = 0,
    /// Single float (4 bytes).
    Float1 = 1,
    /// Two floats (8 bytes).
    Float2 = 2,
    /// Three floats (12 bytes).
    Float3 = 3,
    /// Four floats (16 bytes).
    Float4 = 4,
    /// D3DCOLOR format - 4 packed bytes as ARGB, normalized to 0-1.
    D3DColor = 5,
    /// Four unsigned bytes (4 bytes).
    UByte4 = 6,
    /// Two signed shorts (4 bytes).
    Short2 = 7,
    /// Four signed shorts (8 bytes).
    Short4 = 8,
    /// Four unsigned bytes, normalized to 0-1 (4 bytes).
    UByte4N = 9,
    /// Two signed shorts, normalized to -1 to 1 (4 bytes).
    Short2N = 10,
    /// Four signed shorts, normalized to -1 to 1 (8 bytes).
    Short4N = 11,
    /// Two unsigned shorts, normalized to 0-1 (4 bytes).
    UShort2N = 12,
    /// Four unsigned shorts, normalized to 0-1 (8 bytes).
    UShort4N = 13,
    /// Three 10-bit unsigned values packed into 32 bits.
    UDec3 = 14,
    /// Three 10-bit signed values packed into 32 bits, normalized.
    Dec3N = 15,
    /// Two half-floats (4 bytes).
    HalfFloat2 = 16,
    /// Four half-floats (8 bytes).
    HalfFloat4 = 17,
    /// Single half-float (2 bytes) - non-standard.
    HalfFloat1 = 18,
    /// Three 10-bit unsigned values, normalized (4 bytes).
    UDec3N = 19,
    /// Invalid/unknown type.
    Invalid = 255,
}

impl TryFrom<u8> for VertexElementType {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            0 => Ok(Self::Ignore),
            1 => Ok(Self::Float1),
            2 => Ok(Self::Float2),
            3 => Ok(Self::Float3),
            4 => Ok(Self::Float4),
            5 => Ok(Self::D3DColor),
            6 => Ok(Self::UByte4),
            7 => Ok(Self::Short2),
            8 => Ok(Self::Short4),
            9 => Ok(Self::UByte4N),
            10 => Ok(Self::Short2N),
            11 => Ok(Self::Short4N),
            12 => Ok(Self::UShort2N),
            13 => Ok(Self::UShort4N),
            14 => Ok(Self::UDec3),
            15 => Ok(Self::Dec3N),
            16 => Ok(Self::HalfFloat2),
            17 => Ok(Self::HalfFloat4),
            18 => Ok(Self::HalfFloat1),
            19 => Ok(Self::UDec3N),
            _ => Err(Error::InvalidVertexElementType(value)),
        }
    }
}

impl VertexElementType {
    /// Convert from u32 (for packed format reading).
    pub fn from_u32(value: u32) -> Self {
        if value <= 19 {
            Self::try_from(value as u8).unwrap_or(Self::Invalid)
        } else {
            Self::Invalid
        }
    }

    /// Returns the size in bytes of this element type.
    pub fn size(self) -> usize {
        match self {
            Self::Ignore | Self::Invalid => 0,
            Self::Float1 => 4,
            Self::Float2 => 8,
            Self::Float3 => 12,
            Self::Float4 => 16,
            Self::D3DColor => 4,
            Self::UByte4 => 4,
            Self::Short2 => 4,
            Self::Short4 => 8,
            Self::UByte4N => 4,
            Self::Short2N => 4,
            Self::Short4N => 8,
            Self::UShort2N => 4,
            Self::UShort4N => 8,
            Self::UDec3 => 4,
            Self::Dec3N => 4,
            Self::HalfFloat2 => 4,
            Self::HalfFloat4 => 8,
            Self::HalfFloat1 => 2,
            Self::UDec3N => 4,
        }
    }

    /// Unpack this element type from raw bytes into a Vec4 [x, y, z, w].
    pub fn unpack<R: Read>(self, reader: &mut R) -> Result<[f32; 4]> {
        match self {
            Self::Ignore => Ok([0.0, 0.0, 0.0, 1.0]),

            Self::Float1 => {
                let x = reader.read_f32::<LittleEndian>()?;
                Ok([x, 0.0, 0.0, 1.0])
            }

            Self::Float2 => {
                let x = reader.read_f32::<LittleEndian>()?;
                let y = reader.read_f32::<LittleEndian>()?;
                Ok([x, y, 0.0, 1.0])
            }

            Self::Float3 => {
                let x = reader.read_f32::<LittleEndian>()?;
                let y = reader.read_f32::<LittleEndian>()?;
                let z = reader.read_f32::<LittleEndian>()?;
                Ok([x, y, z, 1.0])
            }

            Self::Float4 => {
                let x = reader.read_f32::<LittleEndian>()?;
                let y = reader.read_f32::<LittleEndian>()?;
                let z = reader.read_f32::<LittleEndian>()?;
                let w = reader.read_f32::<LittleEndian>()?;
                Ok([x, y, z, w])
            }

            Self::D3DColor => {
                // ARGB packed as u32, expand to RGBA floats
                let packed = reader.read_u32::<LittleEndian>()?;
                let a = ((packed >> 24) & 0xFF) as f32 / 255.0;
                let r = ((packed >> 16) & 0xFF) as f32 / 255.0;
                let g = ((packed >> 8) & 0xFF) as f32 / 255.0;
                let b = (packed & 0xFF) as f32 / 255.0;
                Ok([r, g, b, a])
            }

            Self::UByte4 => {
                let packed = reader.read_u32::<LittleEndian>()?;
                let x = (packed & 0xFF) as f32;
                let y = ((packed >> 8) & 0xFF) as f32;
                let z = ((packed >> 16) & 0xFF) as f32;
                let w = ((packed >> 24) & 0xFF) as f32;
                Ok([x, y, z, w])
            }

            Self::Short2 => {
                let x = reader.read_i16::<LittleEndian>()? as f32;
                let y = reader.read_i16::<LittleEndian>()? as f32;
                Ok([x, y, 0.0, 1.0])
            }

            Self::Short4 => {
                let x = reader.read_i16::<LittleEndian>()? as f32;
                let y = reader.read_i16::<LittleEndian>()? as f32;
                let z = reader.read_i16::<LittleEndian>()? as f32;
                let w = reader.read_i16::<LittleEndian>()? as f32;
                Ok([x, y, z, w])
            }

            Self::UByte4N => {
                let packed = reader.read_u32::<LittleEndian>()?;
                let x = (packed & 0xFF) as f32 / 255.0;
                let y = ((packed >> 8) & 0xFF) as f32 / 255.0;
                let z = ((packed >> 16) & 0xFF) as f32 / 255.0;
                let w = ((packed >> 24) & 0xFF) as f32 / 255.0;
                Ok([x, y, z, w])
            }

            Self::Short2N => {
                let x = reader.read_i16::<LittleEndian>()? as f32 / 32767.0;
                let y = reader.read_i16::<LittleEndian>()? as f32 / 32767.0;
                Ok([x, y, 0.0, 1.0])
            }

            Self::Short4N => {
                let x = reader.read_i16::<LittleEndian>()? as f32 / 32767.0;
                let y = reader.read_i16::<LittleEndian>()? as f32 / 32767.0;
                let z = reader.read_i16::<LittleEndian>()? as f32 / 32767.0;
                let w = reader.read_i16::<LittleEndian>()? as f32 / 32767.0;
                Ok([x, y, z, w])
            }

            Self::UShort2N => {
                let x = reader.read_u16::<LittleEndian>()? as f32 / 65535.0;
                let y = reader.read_u16::<LittleEndian>()? as f32 / 65535.0;
                Ok([x, y, 0.0, 1.0])
            }

            Self::UShort4N => {
                let x = reader.read_u16::<LittleEndian>()? as f32 / 65535.0;
                let y = reader.read_u16::<LittleEndian>()? as f32 / 65535.0;
                let z = reader.read_u16::<LittleEndian>()? as f32 / 65535.0;
                let w = reader.read_u16::<LittleEndian>()? as f32 / 65535.0;
                Ok([x, y, z, w])
            }

            Self::UDec3 => {
                // 10-10-10-2 unsigned format
                let packed = reader.read_u32::<LittleEndian>()?;
                let x = (packed & 0x3FF) as f32;
                let y = ((packed >> 10) & 0x3FF) as f32;
                let z = ((packed >> 20) & 0x3FF) as f32;
                Ok([x, y, z, 1.0])
            }

            Self::Dec3N => {
                // 10-10-10-2 signed normalized format
                let packed = reader.read_u32::<LittleEndian>()?;
                let x = sign_extend_10bit((packed & 0x3FF) as i32) as f32 / 511.0;
                let y = sign_extend_10bit(((packed >> 10) & 0x3FF) as i32) as f32 / 511.0;
                let z = sign_extend_10bit(((packed >> 20) & 0x3FF) as i32) as f32 / 511.0;
                Ok([x, y, z, 1.0])
            }

            Self::HalfFloat2 => {
                let x = f16::from_bits(reader.read_u16::<LittleEndian>()?).to_f32();
                let y = f16::from_bits(reader.read_u16::<LittleEndian>()?).to_f32();
                Ok([x, y, 0.0, 1.0])
            }

            Self::HalfFloat4 => {
                let x = f16::from_bits(reader.read_u16::<LittleEndian>()?).to_f32();
                let y = f16::from_bits(reader.read_u16::<LittleEndian>()?).to_f32();
                let z = f16::from_bits(reader.read_u16::<LittleEndian>()?).to_f32();
                let w = f16::from_bits(reader.read_u16::<LittleEndian>()?).to_f32();
                Ok([x, y, z, w])
            }

            Self::HalfFloat1 => {
                let x = f16::from_bits(reader.read_u16::<LittleEndian>()?).to_f32();
                Ok([x, 0.0, 0.0, 1.0])
            }

            Self::UDec3N => {
                // 10-10-10-2 unsigned normalized format
                let packed = reader.read_u32::<LittleEndian>()?;
                let x = (packed & 0x3FF) as f32 / 1023.0;
                let y = ((packed >> 10) & 0x3FF) as f32 / 1023.0;
                let z = ((packed >> 20) & 0x3FF) as f32 / 1023.0;
                Ok([x, y, z, 1.0])
            }

            Self::Invalid => {
                // Invalid type - return zeros
                Ok([0.0, 0.0, 0.0, 0.0])
            }
        }
    }

    /// Unpack as raw integer indices (no normalization).
    ///
    /// Unlike `unpack()`, this always returns the raw integer values even for
    /// normalized types like UByte4N or UShort4N. Use this for bone indices
    /// where you need the actual index values, not normalized floats.
    pub fn unpack_as_indices<R: Read>(self, reader: &mut R) -> Result<[u16; 4]> {
        match self {
            Self::UByte4 | Self::UByte4N => {
                let packed = reader.read_u32::<LittleEndian>()?;
                Ok([
                    (packed & 0xFF) as u16,
                    ((packed >> 8) & 0xFF) as u16,
                    ((packed >> 16) & 0xFF) as u16,
                    ((packed >> 24) & 0xFF) as u16,
                ])
            }
            Self::Short4 | Self::Short4N => {
                let x = reader.read_i16::<LittleEndian>()?.max(0) as u16;
                let y = reader.read_i16::<LittleEndian>()?.max(0) as u16;
                let z = reader.read_i16::<LittleEndian>()?.max(0) as u16;
                let w = reader.read_i16::<LittleEndian>()?.max(0) as u16;
                Ok([x, y, z, w])
            }
            Self::UShort4N => {
                let x = reader.read_u16::<LittleEndian>()?;
                let y = reader.read_u16::<LittleEndian>()?;
                let z = reader.read_u16::<LittleEndian>()?;
                let w = reader.read_u16::<LittleEndian>()?;
                Ok([x, y, z, w])
            }
            // Fall back to unpack() and truncate for other types
            other => {
                let v = other.unpack(reader)?;
                Ok([v[0] as u16, v[1] as u16, v[2] as u16, v[3] as u16])
            }
        }
    }
}

/// Sign-extend a 10-bit value to i32.
fn sign_extend_10bit(value: i32) -> i32 {
    if value & 0x200 != 0 {
        value | !0x3FF
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_float3_unpack() {
        let data: [u8; 12] = [
            0x00, 0x00, 0x80, 0x3F, // 1.0f
            0x00, 0x00, 0x00, 0x40, // 2.0f
            0x00, 0x00, 0x40, 0x40, // 3.0f
        ];
        let mut cursor = Cursor::new(&data);
        let result = VertexElementType::Float3.unpack(&mut cursor).unwrap();
        assert_eq!(result, [1.0, 2.0, 3.0, 1.0]);
    }

    #[test]
    fn test_ubyte4n_unpack() {
        let data: [u8; 4] = [255, 128, 0, 255];
        let mut cursor = Cursor::new(&data);
        let result = VertexElementType::UByte4N.unpack(&mut cursor).unwrap();
        assert!((result[0] - 1.0).abs() < 0.01);
        assert!((result[1] - 0.5).abs() < 0.01);
        assert!((result[2] - 0.0).abs() < 0.01);
        assert!((result[3] - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_element_sizes() {
        assert_eq!(VertexElementType::Float3.size(), 12);
        assert_eq!(VertexElementType::Float4.size(), 16);
        assert_eq!(VertexElementType::HalfFloat2.size(), 4);
        assert_eq!(VertexElementType::Dec3N.size(), 4);
        assert_eq!(VertexElementType::UByte4.size(), 4);
    }

    #[test]
    fn test_unpack_as_indices_ubyte4() {
        // Bytes: [5, 10, 200, 0] packed little-endian
        let data: [u8; 4] = [5, 10, 200, 0];
        let mut cursor = Cursor::new(&data);
        let result = VertexElementType::UByte4.unpack_as_indices(&mut cursor).unwrap();
        assert_eq!(result, [5, 10, 200, 0]);
    }

    #[test]
    fn test_unpack_as_indices_ubyte4n_not_normalized() {
        // UByte4N.unpack() would return [1.0, 0.5, 0.0, 1.0]
        // unpack_as_indices() must return the raw byte values instead
        let data: [u8; 4] = [255, 128, 0, 255];
        let mut cursor = Cursor::new(&data);
        let result = VertexElementType::UByte4N.unpack_as_indices(&mut cursor).unwrap();
        assert_eq!(result, [255, 128, 0, 255]);
    }

    #[test]
    fn test_unpack_as_indices_short4_positive() {
        // Two positive i16 values: 300, 1
        let mut data = Vec::new();
        data.extend_from_slice(&300i16.to_le_bytes());
        data.extend_from_slice(&1i16.to_le_bytes());
        data.extend_from_slice(&0i16.to_le_bytes());
        data.extend_from_slice(&0i16.to_le_bytes());
        let mut cursor = Cursor::new(&data);
        let result = VertexElementType::Short4.unpack_as_indices(&mut cursor).unwrap();
        assert_eq!(result, [300, 1, 0, 0]);
    }

    #[test]
    fn test_unpack_as_indices_short4_negative_clamped() {
        // Negative i16 should clamp to 0
        let mut data = Vec::new();
        data.extend_from_slice(&(-1i16).to_le_bytes());
        data.extend_from_slice(&5i16.to_le_bytes());
        data.extend_from_slice(&(-100i16).to_le_bytes());
        data.extend_from_slice(&0i16.to_le_bytes());
        let mut cursor = Cursor::new(&data);
        let result = VertexElementType::Short4.unpack_as_indices(&mut cursor).unwrap();
        assert_eq!(result, [0, 5, 0, 0]);
    }

    #[test]
    fn test_unpack_as_indices_ushort4n_not_normalized() {
        // UShort4N.unpack() would normalize to 0.0-1.0
        // unpack_as_indices() must return raw u16 values
        let mut data = Vec::new();
        data.extend_from_slice(&500u16.to_le_bytes());
        data.extend_from_slice(&65535u16.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&1u16.to_le_bytes());
        let mut cursor = Cursor::new(&data);
        let result = VertexElementType::UShort4N.unpack_as_indices(&mut cursor).unwrap();
        assert_eq!(result, [500, 65535, 0, 1]);
    }
}
