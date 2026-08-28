//! Vertex element types and unpacking.
//!
//! This module handles the various packed vertex formats used in UGX files.
//!
//! # C++ Equivalent
//!
//! This corresponds to `VertexElement::EType` enum from the original source.
//! The enum values 0-19 match exactly with the original Xbox 360 source.
//!
//! # Common Vertex Formats
//!
//! | Attribute    | Typical Type | Size | Notes                           |
//! |--------------|--------------|------|---------------------------------|
//! | Position     | Float4       | 16   | XYZ + W (W usually 1.0)         |
//! | Normal       | `Dec3N`        | 4    | 10-bit normalized XYZ           |
//! | Tangent      | `Dec3N`        | 4    | 10-bit normalized XYZ           |
//! | Binormal     | `Dec3N`        | 4    | 10-bit normalized XYZ           |
//! | `TexCoord`     | `HalfFloat2`   | 4    | UV as half-floats               |
//! | `BoneIndices`  | `UByte4`       | 4    | 4 bone indices (0-255)          |
//! | `BoneWeights`  | `UByte4N`      | 4    | 4 weights normalized to 0-1     |
//! | `VertexColor`  | `D3DColor`     | 4    | ARGB packed as BGRA bytes       |
//!
//! # Normalization
//!
//! - `*N` types (e.g., `UByte4N`, `Short2N`) are normalized to floating point
//! - Unsigned normalized: `value / max_value` (e.g., 255 → 1.0)
//! - Signed normalized: `value / max_value` (e.g., 32767 → 1.0, -32768 → -1.0)

use alloc::vec::Vec;
use half::f16;
use num_traits::ToPrimitive;

use nostdio::{Cursor, ReadLe};

use crate::error::{Error, Result};

/// Vertex element data types (matches C++ `VertexElement::EType` enum).
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
    #[must_use]
    pub fn from_u32(value: u32) -> Self {
        u8::try_from(value)
            .ok()
            .and_then(|value| Self::try_from(value).ok())
            .unwrap_or(Self::Invalid)
    }

    /// Returns the size in bytes of this element type.
    #[must_use]
    pub fn size(self) -> usize {
        match self {
            Self::Ignore | Self::Invalid => 0,
            Self::HalfFloat1 => 2,
            Self::Float1
            | Self::D3DColor
            | Self::UByte4
            | Self::Short2
            | Self::UByte4N
            | Self::Short2N
            | Self::UShort2N
            | Self::UDec3
            | Self::Dec3N
            | Self::HalfFloat2
            | Self::UDec3N => 4,
            Self::Float2 | Self::Short4 | Self::Short4N | Self::UShort4N | Self::HalfFloat4 => 8,
            Self::Float3 => 12,
            Self::Float4 => 16,
        }
    }

    /// Unpack this element type from raw bytes into a Vec4 [x, y, z, w].
    ///
    /// # Errors
    ///
    /// Returns an error if the input is truncated or the cursor position
    /// cannot be represented on the target platform.
    pub fn unpack(self, data: &[u8], pos: &mut usize) -> Result<[f32; 4]> {
        let remaining = data.get(*pos..).ok_or_else(|| Error::UnexpectedEof {
            context: "vertex element".into(),
        })?;
        let mut cur = Cursor::new(remaining);
        let result = match self {
            Self::Ignore => Ok([0.0, 0.0, 0.0, 1.0]),
            Self::Float1 => Ok([cur.read_f32_le()?, 0.0, 0.0, 1.0]),
            Self::Float2 => {
                let x = cur.read_f32_le()?;
                let y = cur.read_f32_le()?;
                Ok([x, y, 0.0, 1.0])
            }
            Self::Float3 => {
                let x = cur.read_f32_le()?;
                let y = cur.read_f32_le()?;
                let z = cur.read_f32_le()?;
                Ok([x, y, z, 1.0])
            }
            Self::Float4 => {
                let x = cur.read_f32_le()?;
                let y = cur.read_f32_le()?;
                let z = cur.read_f32_le()?;
                let w = cur.read_f32_le()?;
                Ok([x, y, z, w])
            }
            Self::D3DColor | Self::UByte4 | Self::UByte4N => {
                Ok(unpack_bytes(self, cur.read_u32_le()?))
            }
            Self::Short2 => {
                let x = f32::from(cur.read_i16_le()?);
                let y = f32::from(cur.read_i16_le()?);
                Ok([x, y, 0.0, 1.0])
            }
            Self::Short4 => {
                let x = f32::from(cur.read_i16_le()?);
                let y = f32::from(cur.read_i16_le()?);
                let z = f32::from(cur.read_i16_le()?);
                let w = f32::from(cur.read_i16_le()?);
                Ok([x, y, z, w])
            }
            Self::Short2N => {
                let x = f32::from(cur.read_i16_le()?) / 32767.0;
                let y = f32::from(cur.read_i16_le()?) / 32767.0;
                Ok([x, y, 0.0, 1.0])
            }
            Self::Short4N => {
                let x = f32::from(cur.read_i16_le()?) / 32767.0;
                let y = f32::from(cur.read_i16_le()?) / 32767.0;
                let z = f32::from(cur.read_i16_le()?) / 32767.0;
                let w = f32::from(cur.read_i16_le()?) / 32767.0;
                Ok([x, y, z, w])
            }
            Self::UShort2N => {
                let x = f32::from(cur.read_u16_le()?) / 65535.0;
                let y = f32::from(cur.read_u16_le()?) / 65535.0;
                Ok([x, y, 0.0, 1.0])
            }
            Self::UShort4N => {
                let x = f32::from(cur.read_u16_le()?) / 65535.0;
                let y = f32::from(cur.read_u16_le()?) / 65535.0;
                let z = f32::from(cur.read_u16_le()?) / 65535.0;
                let w = f32::from(cur.read_u16_le()?) / 65535.0;
                Ok([x, y, z, w])
            }
            Self::UDec3 | Self::Dec3N | Self::UDec3N => Ok(unpack_dec3(self, cur.read_u32_le()?)),
            Self::HalfFloat2 => {
                let x = f16::from_bits(cur.read_u16_le()?).to_f32();
                let y = f16::from_bits(cur.read_u16_le()?).to_f32();
                Ok([x, y, 0.0, 1.0])
            }
            Self::HalfFloat4 => {
                let x = f16::from_bits(cur.read_u16_le()?).to_f32();
                let y = f16::from_bits(cur.read_u16_le()?).to_f32();
                let z = f16::from_bits(cur.read_u16_le()?).to_f32();
                let w = f16::from_bits(cur.read_u16_le()?).to_f32();
                Ok([x, y, z, w])
            }
            Self::HalfFloat1 => {
                let x = f16::from_bits(cur.read_u16_le()?).to_f32();
                Ok([x, 0.0, 0.0, 1.0])
            }
            Self::Invalid => Ok([0.0, 0.0, 0.0, 0.0]),
        };
        crate::advance_position(pos, cur.position(), "binary cursor position")?;
        result
    }

    /// Unpack as raw integer indices (no normalization).
    ///
    /// Unlike `unpack()`, this always returns the raw integer values even for
    /// normalized types like `UByte4N` or `UShort4N`. Use this for bone indices
    /// where you need the actual index values, not normalized floats.
    ///
    /// # Errors
    ///
    /// Returns an error if the input is truncated or the cursor position
    /// cannot be represented on the target platform.
    pub fn unpack_as_indices(self, data: &[u8], pos: &mut usize) -> Result<[u16; 4]> {
        let remaining = data.get(*pos..).ok_or_else(|| Error::UnexpectedEof {
            context: "vertex indices".into(),
        })?;
        let mut cur = Cursor::new(remaining);
        let result = match self {
            Self::UByte4 | Self::UByte4N => {
                let packed = cur.read_u32_le()?;
                Ok([
                    masked_u16(packed, 0, 0xFF),
                    masked_u16(packed, 8, 0xFF),
                    masked_u16(packed, 16, 0xFF),
                    masked_u16(packed, 24, 0xFF),
                ])
            }
            Self::Short4 | Self::Short4N => {
                let x = cur.read_i16_le()?.max(0).cast_unsigned();
                let y = cur.read_i16_le()?.max(0).cast_unsigned();
                let z = cur.read_i16_le()?.max(0).cast_unsigned();
                let w = cur.read_i16_le()?.max(0).cast_unsigned();
                Ok([x, y, z, w])
            }
            Self::UShort4N => {
                let x = cur.read_u16_le()?;
                let y = cur.read_u16_le()?;
                let z = cur.read_u16_le()?;
                let w = cur.read_u16_le()?;
                Ok([x, y, z, w])
            }
            other => {
                crate::advance_position(pos, cur.position(), "binary cursor position")?;
                let v = other.unpack(data, pos)?;
                return Ok(v.map(float_to_u16));
            }
        };
        crate::advance_position(pos, cur.position(), "binary cursor position")?;
        result
    }

    /// Pack a Vec4 [x, y, z, w] into raw bytes (inverse of `unpack()`).
    pub fn pack(self, out: &mut Vec<u8>, value: [f32; 4]) {
        match self {
            Self::Ignore | Self::Invalid => {}
            Self::Float1 => out.extend_from_slice(&value[0].to_le_bytes()),
            Self::Float2 => {
                out.extend_from_slice(&value[0].to_le_bytes());
                out.extend_from_slice(&value[1].to_le_bytes());
            }
            Self::Float3 => {
                out.extend_from_slice(&value[0].to_le_bytes());
                out.extend_from_slice(&value[1].to_le_bytes());
                out.extend_from_slice(&value[2].to_le_bytes());
            }
            Self::Float4 => {
                out.extend_from_slice(&value[0].to_le_bytes());
                out.extend_from_slice(&value[1].to_le_bytes());
                out.extend_from_slice(&value[2].to_le_bytes());
                out.extend_from_slice(&value[3].to_le_bytes());
            }
            Self::D3DColor
            | Self::UByte4
            | Self::Short2
            | Self::Short4
            | Self::UByte4N
            | Self::Short2N
            | Self::Short4N
            | Self::UShort2N
            | Self::UShort4N
            | Self::UDec3
            | Self::Dec3N
            | Self::UDec3N => pack_quantized(self, out, value),
            Self::HalfFloat2 => {
                out.extend_from_slice(&f16::from_f32(value[0]).to_bits().to_le_bytes());
                out.extend_from_slice(&f16::from_f32(value[1]).to_bits().to_le_bytes());
            }
            Self::HalfFloat4 => {
                for v in &value {
                    out.extend_from_slice(&f16::from_f32(*v).to_bits().to_le_bytes());
                }
            }
            Self::HalfFloat1 => {
                out.extend_from_slice(&f16::from_f32(value[0]).to_bits().to_le_bytes());
            }
        }
    }

    /// Pack raw integer indices into bytes (inverse of `unpack_as_indices()`).
    pub fn pack_as_indices(self, out: &mut Vec<u8>, indices: [u16; 4]) {
        match self {
            Self::UByte4 | Self::UByte4N => {
                let packed = u32::from(indices[0])
                    | (u32::from(indices[1]) << 8)
                    | (u32::from(indices[2]) << 16)
                    | (u32::from(indices[3]) << 24);
                out.extend_from_slice(&packed.to_le_bytes());
            }
            Self::Short4 | Self::Short4N => {
                for &idx in &indices {
                    out.extend_from_slice(&idx.cast_signed().to_le_bytes());
                }
            }
            Self::UShort4N => {
                for &idx in &indices {
                    out.extend_from_slice(&idx.to_le_bytes());
                }
            }
            other => {
                let v = [
                    f32::from(indices[0]),
                    f32::from(indices[1]),
                    f32::from(indices[2]),
                    f32::from(indices[3]),
                ];
                other.pack(out, v);
            }
        }
    }
}

/// Extract a masked integer component known to fit in `u16`.
fn masked_u16(packed: u32, shift: u32, mask: u32) -> u16 {
    u16::try_from((packed >> shift) & mask).unwrap_or_default()
}

/// Unpack one of the four-byte component formats.
fn unpack_bytes(element_type: VertexElementType, packed: u32) -> [f32; 4] {
    let bytes = [
        masked_u16(packed, 0, 0xFF),
        masked_u16(packed, 8, 0xFF),
        masked_u16(packed, 16, 0xFF),
        masked_u16(packed, 24, 0xFF),
    ];
    match element_type {
        VertexElementType::D3DColor => [
            f32::from(bytes[2]) / 255.0,
            f32::from(bytes[1]) / 255.0,
            f32::from(bytes[0]) / 255.0,
            f32::from(bytes[3]) / 255.0,
        ],
        VertexElementType::UByte4 => bytes.map(f32::from),
        VertexElementType::UByte4N => bytes.map(|value| f32::from(value) / 255.0),
        _ => [0.0; 4],
    }
}

/// Unpack a three-component 10-bit format.
fn unpack_dec3(element_type: VertexElementType, packed: u32) -> [f32; 4] {
    let components = [
        masked_u16(packed, 0, 0x3FF),
        masked_u16(packed, 10, 0x3FF),
        masked_u16(packed, 20, 0x3FF),
    ];
    match element_type {
        VertexElementType::UDec3 => {
            let [x, y, z] = components.map(f32::from);
            [x, y, z, 1.0]
        }
        VertexElementType::UDec3N => {
            let [x, y, z] = components.map(|value| f32::from(value) / 1023.0);
            [x, y, z, 1.0]
        }
        VertexElementType::Dec3N => {
            let [x, y, z] = components.map(|value| f32::from(sign_extend_10bit(value)) / 511.0);
            let w = if packed >> 30 == 0b10 { -1.0 } else { 1.0 };
            [x, y, z, w]
        }
        _ => [0.0; 4],
    }
}

/// Convert a floating-point value to a format-sized unsigned integer.
fn float_to_u16(value: f32) -> u16 {
    value
        .clamp(0.0, f32::from(u16::MAX))
        .to_u16()
        .unwrap_or_default()
}

/// Convert a floating-point value to a format-sized signed integer.
fn float_to_i16(value: f32) -> i16 {
    value
        .clamp(f32::from(i16::MIN), f32::from(i16::MAX))
        .to_i16()
        .unwrap_or_default()
}

/// Quantize an unsigned normalized component to the requested maximum.
fn quantize_unsigned(value: f32, maximum: f32) -> u32 {
    (value.clamp(0.0, 1.0) * maximum)
        .round()
        .to_u32()
        .unwrap_or_default()
}

/// Quantize a signed normalized component to an `i16` storage value.
fn quantize_signed(value: f32) -> i16 {
    (value.clamp(-1.0, 1.0) * 32767.0)
        .round()
        .to_i16()
        .unwrap_or_default()
}

/// Pack four byte-sized components into a little-endian word.
fn pack_bytes(values: [u32; 4]) -> u32 {
    values[0] | (values[1] << 8) | (values[2] << 16) | (values[3] << 24)
}

/// Pack the integer and normalized vertex formats.
fn pack_quantized(element_type: VertexElementType, out: &mut Vec<u8>, value: [f32; 4]) {
    match element_type {
        VertexElementType::D3DColor => {
            let [red, green, blue, alpha] =
                value.map(|component| quantize_unsigned(component, 255.0));
            out.extend_from_slice(&pack_bytes([blue, green, red, alpha]).to_le_bytes());
        }
        VertexElementType::UByte4 => {
            let bytes = value.map(|component| u32::from(float_to_u16(component).min(255)));
            out.extend_from_slice(&pack_bytes(bytes).to_le_bytes());
        }
        VertexElementType::Short2 => {
            for component in &value[..2] {
                out.extend_from_slice(&float_to_i16(*component).to_le_bytes());
            }
        }
        VertexElementType::Short4 => {
            for component in value {
                out.extend_from_slice(&float_to_i16(component).to_le_bytes());
            }
        }
        VertexElementType::UByte4N => {
            let bytes = value.map(|component| quantize_unsigned(component, 255.0));
            out.extend_from_slice(&pack_bytes(bytes).to_le_bytes());
        }
        VertexElementType::Short2N => {
            for component in &value[..2] {
                out.extend_from_slice(&quantize_signed(*component).to_le_bytes());
            }
        }
        VertexElementType::Short4N => {
            for component in value {
                out.extend_from_slice(&quantize_signed(component).to_le_bytes());
            }
        }
        VertexElementType::UShort2N => {
            for component in &value[..2] {
                let packed = quantize_unsigned(*component, 65535.0);
                out.extend_from_slice(&u16::try_from(packed).unwrap_or_default().to_le_bytes());
            }
        }
        VertexElementType::UShort4N => {
            for component in value {
                let packed = quantize_unsigned(component, 65535.0);
                out.extend_from_slice(&u16::try_from(packed).unwrap_or_default().to_le_bytes());
            }
        }
        VertexElementType::UDec3 => {
            let components =
                value.map(|component| component.clamp(0.0, 1023.0).to_u32().unwrap_or_default());
            let packed = components[0] | (components[1] << 10) | (components[2] << 20);
            out.extend_from_slice(&packed.to_le_bytes());
        }
        VertexElementType::Dec3N => {
            let components = value.map(|component| {
                (component * 511.0)
                    .round()
                    .clamp(-512.0, 511.0)
                    .to_i16()
                    .unwrap_or_default()
            });
            let x = u32::from(components[0].cast_unsigned()) & 0x3FF;
            let y = u32::from(components[1].cast_unsigned()) & 0x3FF;
            let z = u32::from(components[2].cast_unsigned()) & 0x3FF;
            let handedness = u32::from(value[3] < 0.0) << 31;
            out.extend_from_slice(&(x | (y << 10) | (z << 20) | handedness).to_le_bytes());
        }
        VertexElementType::UDec3N => {
            let [x, y, z, _] = value.map(|component| quantize_unsigned(component, 1023.0));
            out.extend_from_slice(&(x | (y << 10) | (z << 20)).to_le_bytes());
        }
        _ => {}
    }
}

/// Sign-extend a 10-bit value to `i16`.
fn sign_extend_10bit(value: u16) -> i16 {
    let value = i16::try_from(value).unwrap_or_default();
    if value & 0x200 != 0 {
        value | !0x3FF
    } else {
        value
    }
}

#[cfg(test)]
#[path = "element_tests.rs"]
mod tests;
