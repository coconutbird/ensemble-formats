//! Variant types and encoding for BBinaryDataTree values.
//!
//! BBinaryDataTree uses a variant system to store values efficiently. Each variant value
//! is encoded as a 32-bit integer with the following layout:
//!
//! ```text
//! Bits 31-24 (type byte):
//!   Bit 7 (0x80): OFFSET_FLAG - Value is stored at an offset in data table
//!   Bit 6 (0x40): UNSIGNED_FLAG (for integers) or VEC_SIZE high bit (for FloatVec)
//!   Bit 5 (0x20): VEC_SIZE low bit (for FloatVec, encodes 2/3/4 components)
//!   Bits 4-0: Variant type (0-10)
//!
//! Bits 23-0 (data):
//!   For direct values: The actual value (Int24, Float24, Bool, etc.)
//!   For offset values: Offset into the data/string table
//! ```

use crate::error::{Error, Result};

// ============================================================================
// Variant Type Flags
// ============================================================================

/// Type mask for extracting the variant type (bits 0-4).
#[allow(dead_code)]
pub const TYPE_MASK: u8 = 0x1F;

/// Flag indicating the value is stored as an offset (bit 7).
pub const OFFSET_FLAG: u8 = 0x80;

/// Flag indicating an unsigned integer (bit 6).
pub const UNSIGNED_FLAG: u8 = 0x40;

/// Mask for vector size bits (bits 5-6).
#[allow(dead_code)]
pub const VEC_SIZE_MASK: u8 = 0x60;

/// Shift amount to extract vector size from type byte.
#[allow(dead_code)]
pub const VEC_SIZE_SHIFT: u8 = 5;

// ============================================================================
// Variant Type Enum
// ============================================================================

/// Variant type enumeration matching the BBinaryDataTree format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
#[allow(dead_code)]
pub enum VariantType {
    Null = 0,
    Float24 = 1,
    Float = 2,
    Int24 = 3,
    Int32 = 4,
    Fract24 = 5,
    Double = 6,
    Bool = 7,
    String = 8,
    UString = 9,
    FloatVec = 10,
}

impl VariantType {
    #[allow(dead_code)]
    pub fn from_byte(byte: u8) -> Result<Self> {
        match byte & TYPE_MASK {
            0 => Ok(VariantType::Null),
            1 => Ok(VariantType::Float24),
            2 => Ok(VariantType::Float),
            3 => Ok(VariantType::Int24),
            4 => Ok(VariantType::Int32),
            5 => Ok(VariantType::Fract24),
            6 => Ok(VariantType::Double),
            7 => Ok(VariantType::Bool),
            8 => Ok(VariantType::String),
            9 => Ok(VariantType::UString),
            10 => Ok(VariantType::FloatVec),
            n => Err(Error::InvalidVariantType(n)),
        }
    }

    #[allow(dead_code)]
    pub fn always_offset(&self) -> bool {
        matches!(
            self,
            VariantType::Float | VariantType::Int32 | VariantType::Double | VariantType::FloatVec
        )
    }

    #[allow(dead_code)]
    pub fn always_direct(&self) -> bool {
        matches!(
            self,
            VariantType::Null
                | VariantType::Float24
                | VariantType::Int24
                | VariantType::Fract24
                | VariantType::Bool
        )
    }
}

// ============================================================================
// Variant Value Enum
// ============================================================================

/// A variant value in the BBinaryDataTree format.
#[derive(Debug, Clone, PartialEq)]
pub enum Variant {
    Null,
    Float(f32),
    Double(f64),
    Int(i32),
    UInt(u32),
    Bool(bool),
    String(String),
    UString(String),
    FloatVec(Vec<f32>),
}

impl Default for Variant {
    fn default() -> Self {
        Variant::Null
    }
}

impl Variant {
    pub fn to_string_value(&self) -> String {
        match self {
            Variant::Null => String::new(),
            Variant::Float(v) => v.to_string(),
            Variant::Double(v) => v.to_string(),
            Variant::Int(v) => v.to_string(),
            Variant::UInt(v) => v.to_string(),
            Variant::Bool(v) => if *v { "true" } else { "false" }.to_string(),
            Variant::String(s) | Variant::UString(s) => s.clone(),
            Variant::FloatVec(v) => v
                .iter()
                .map(|f| f.to_string())
                .collect::<Vec<_>>()
                .join(","),
        }
    }

    pub fn as_float(&self) -> Option<f32> {
        match self {
            Variant::Float(v) => Some(*v),
            Variant::Double(v) => Some(*v as f32),
            Variant::Int(v) => Some(*v as f32),
            Variant::UInt(v) => Some(*v as f32),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i32> {
        match self {
            Variant::Int(v) => Some(*v),
            Variant::UInt(v) => Some(*v as i32),
            Variant::Float(v) => Some(*v as i32),
            Variant::Double(v) => Some(*v as i32),
            Variant::Bool(v) => Some(if *v { 1 } else { 0 }),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Variant::Bool(v) => Some(*v),
            Variant::Int(v) => Some(*v != 0),
            Variant::UInt(v) => Some(*v != 0),
            _ => None,
        }
    }
}

// ============================================================================
// Variant Encoding/Decoding Functions
// ============================================================================

/// Pack a 32-bit float into a 24-bit representation.
pub fn pack_float24(value: f32) -> u32 {
    if value == 0.0 {
        return 0;
    }

    let bits = value.to_bits();
    let sign = (bits >> 31) & 1;
    let exp = ((bits >> 23) & 0xFF) as i32;
    let mantissa = bits & 0x7FFFFF;

    // Bias conversion: IEEE 754 uses 127, we use 31
    let new_exp = (exp - 127 + 31).clamp(0, 63) as u32;

    // Take top 17 bits of 23-bit mantissa
    let new_mantissa = mantissa >> 6;

    (sign << 23) | (new_exp << 17) | new_mantissa
}

/// Unpack a 24-bit float representation to a 32-bit float.
pub fn unpack_float24(packed: u32) -> f32 {
    if packed == 0 {
        return 0.0;
    }

    let sign = (packed >> 23) & 1;
    let exp = (packed >> 17) & 0x3F;
    let mantissa = packed & 0x1FFFF;

    // Convert exponent bias back: from 31 to 127
    let new_exp = (exp as i32 - 31 + 127) as u32;

    // Extend mantissa from 17 to 23 bits
    let new_mantissa = mantissa << 6;

    let bits = (sign << 31) | (new_exp << 23) | new_mantissa;
    f32::from_bits(bits)
}

/// Pack a float as a 24-bit fixed-point fraction (value * 10,000).
#[allow(dead_code)]
pub fn pack_fract24(value: f32) -> u32 {
    let scaled = (value * 10000.0).round() as i32;
    if scaled >= 0 {
        (scaled as u32) & 0x7FFFFF
    } else {
        ((-scaled) as u32 & 0x7FFFFF) | 0x800000
    }
}

/// Unpack a 24-bit fixed-point fraction to a float.
pub fn unpack_fract24(packed: u32) -> f32 {
    let is_negative = (packed & 0x800000) != 0;
    let magnitude = (packed & 0x7FFFFF) as f32;
    let value = magnitude / 10000.0;
    if is_negative {
        -value
    } else {
        value
    }
}

/// Pack a 24-bit signed integer.
pub fn pack_int24(value: i32) -> u32 {
    if value >= 0 {
        (value as u32) & 0x7FFFFF
    } else {
        ((-value) as u32 & 0x7FFFFF) | 0x800000
    }
}

/// Unpack a 24-bit signed integer.
pub fn unpack_int24(packed: u32) -> i32 {
    let is_negative = (packed & 0x800000) != 0;
    let magnitude = (packed & 0x7FFFFF) as i32;
    if is_negative {
        -magnitude
    } else {
        magnitude
    }
}

/// Pack a 24-bit unsigned integer.
pub fn pack_uint24(value: u32) -> u32 {
    value & 0xFFFFFF
}

/// Unpack a 24-bit unsigned integer.
#[allow(dead_code)]
pub fn unpack_uint24(packed: u32) -> u32 {
    packed & 0xFFFFFF
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_float24_roundtrip() {
        let values = [0.0f32, 1.0, -1.0, 0.5, 100.0, -0.001];
        for value in values {
            let packed = pack_float24(value);
            let unpacked = unpack_float24(packed);
            assert!(
                (value - unpacked).abs() < 0.01,
                "Float24 roundtrip failed for {}: got {}",
                value,
                unpacked
            );
        }
    }

    #[test]
    fn test_int24_roundtrip() {
        let values = [0i32, 1, -1, 1000, -1000, 8388607];
        for value in values {
            let packed = pack_int24(value);
            let unpacked = unpack_int24(packed);
            assert_eq!(value, unpacked, "Int24 roundtrip failed for {}", value);
        }
    }
}
