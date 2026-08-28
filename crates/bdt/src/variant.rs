//! Variant types and encoding for `BBinaryDataTree` values.
//!
//! `BBinaryDataTree` uses a variant system to store values efficiently. Each variant value
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

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use num_traits::ToPrimitive;

use crate::error::{Error, Result};

/// Type mask for extracting the variant type (bits 0-4).
pub const TYPE_MASK: u8 = 0x1F;

/// Flag indicating the value is stored as an offset (bit 7).
pub const OFFSET_FLAG: u8 = 0x80;

/// Flag indicating an unsigned integer (bit 6).
pub const UNSIGNED_FLAG: u8 = 0x40;

/// Mask for vector size bits (bits 5-6).
pub const VEC_SIZE_MASK: u8 = 0x60;

/// Shift amount to extract vector size from type byte.
pub const VEC_SIZE_SHIFT: u8 = 5;

/// On-disk type code stored in the upper byte of a packed variant value.
///
/// These codes occupy bits 0–3 of the type byte. The remaining bits carry
/// flags ([`OFFSET_FLAG`], [`UNSIGNED_FLAG`], vector size).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum VariantType {
    /// No value.
    Null = 0,
    /// 24-bit packed float (direct, no offset).
    Float24 = 1,
    /// 32-bit IEEE 754 float (stored at offset in data table).
    Float = 2,
    /// 24-bit packed signed/unsigned integer (direct).
    Int24 = 3,
    /// 32-bit integer (stored at offset in data table).
    Int32 = 4,
    /// 24-bit fixed-point fraction (value × 10 000).
    Fract24 = 5,
    /// 64-bit IEEE 754 double (stored at offset).
    Double = 6,
    /// Boolean (direct, 0 or 1).
    Bool = 7,
    /// Null-terminated UTF-8 string.
    String = 8,
    /// Null-terminated UTF-16 string.
    UString = 9,
    /// Vector of 2–4 floats (stored at offset).
    FloatVec = 10,
}

impl VariantType {
    /// Decode a type code from the lower 5 bits of a byte.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidVariantType`] when the masked type code is not
    /// one of the supported [`VariantType`] values.
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

    /// Returns `true` if this type is always stored at an offset in the data table.
    #[must_use]
    pub fn always_offset(&self) -> bool {
        matches!(
            self,
            VariantType::Float | VariantType::Int32 | VariantType::Double | VariantType::FloatVec
        )
    }

    /// Returns `true` if this type is always encoded directly in the 24-bit data field.
    #[must_use]
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

/// A dynamically-typed value in the `BBinaryDataTree` format.
///
/// Each node's text content and each attribute value is stored as a `Variant`.
/// The variant type determines how the value is serialized in the packed binary
/// format (see [`VariantType`] for the on-disk type codes).
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Variant {
    /// No value / absent.
    #[default]
    Null,
    /// 32-bit IEEE 754 float.
    Float(f32),
    /// 64-bit IEEE 754 double.
    Double(f64),
    /// Signed 32-bit integer (may be packed as 24-bit on disk).
    Int(i32),
    /// Unsigned 32-bit integer (may be packed as 24-bit on disk).
    UInt(u32),
    /// Boolean flag.
    Bool(bool),
    /// UTF-8 string (narrow).
    String(String),
    /// UTF-16 string (wide), stored as a Rust `String` after decoding.
    UString(String),
    /// Vector of 2–4 floats (e.g. position, color).
    FloatVec(Vec<f32>),
    /// 24-bit fixed-point fraction (value × 10 000, sign-magnitude).
    ///
    /// Stored as `f32` like other numeric types.  The `"%u.%04u"` formatting
    /// is applied in [`Variant::to_string_value`].
    Fract24(f32),
}

impl Variant {
    /// Format the value as a human-readable string.
    ///
    /// - `Null` → `""`
    /// - `FloatVec` → comma-separated (e.g. `"1.0,2.0,3.0"`)
    /// - All others → their natural `ToString` representation.
    #[must_use]
    pub fn to_string_value(&self) -> String {
        match self {
            Variant::Null => String::new(),
            Variant::Float(v) => v.to_string(),
            Variant::Double(v) => v.to_string(),
            Variant::Int(v) => v.to_string(),
            Variant::UInt(v) => v.to_string(),
            Variant::Bool(v) => if *v { "true" } else { "false" }.to_string(),
            Variant::Fract24(v) => {
                let packed = pack_fract24(*v);
                let is_negative = (packed & 0x0080_0000) != 0;
                let magnitude = packed & 0x007F_FFFF;
                let integer_part = magnitude / 10000;
                let fract_part = magnitude % 10000;
                if is_negative {
                    alloc::format!("-{integer_part}.{fract_part:04}")
                } else {
                    alloc::format!("{integer_part}.{fract_part:04}")
                }
            }
            Variant::String(s) | Variant::UString(s) => s.clone(),
            Variant::FloatVec(v) => v
                .iter()
                .map(alloc::string::ToString::to_string)
                .collect::<Vec<_>>()
                .join(","),
        }
    }

    /// Try to interpret the value as an `f32`.
    ///
    /// Converts `Float`, `Double`, `Int`, `UInt`, and `Fract24` variants
    /// directly. For `String`/`UString`, attempts `parse::<f32>()` after
    /// stripping a trailing `f`/`F` suffix (C-style float literal).
    /// Returns `None` for bools, vecs, null, and unparseable strings.
    #[must_use]
    pub fn as_float(&self) -> Option<f32> {
        match self {
            Variant::Float(v) | Variant::Fract24(v) => Some(*v),
            Variant::Double(v) => v.to_f32(),
            Variant::Int(v) => v.to_f32(),
            Variant::UInt(v) => v.to_f32(),
            Variant::String(s) | Variant::UString(s) => {
                // Strip optional trailing 'f'/'F' (C-style float literal).
                let trimmed = s
                    .strip_suffix('f')
                    .or_else(|| s.strip_suffix('F'))
                    .unwrap_or(s);
                trimmed.parse::<f32>().ok()
            }
            _ => None,
        }
    }

    /// Try to interpret the value as an `i32`.
    ///
    /// Converts numeric and boolean variants; returns `None` for strings,
    /// vecs, and null.
    #[must_use]
    pub fn as_int(&self) -> Option<i32> {
        match self {
            Variant::Int(v) => Some(*v),
            Variant::UInt(v) => v.to_i32(),
            Variant::Float(v) => v.to_i32(),
            Variant::Double(v) => v.to_i32(),
            Variant::Bool(v) => Some(i32::from(*v)),
            _ => None,
        }
    }

    /// Try to interpret the value as a `bool`.
    ///
    /// Converts `Bool`, `Int`, and `UInt` (non-zero = true); returns `None`
    /// for other types.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Variant::Bool(v) => Some(*v),
            Variant::Int(v) => Some(*v != 0),
            Variant::UInt(v) => Some(*v != 0),
            _ => None,
        }
    }
}

/// Pack a 32-bit float into a 24-bit representation.
#[must_use]
pub fn pack_float24(value: f32) -> u32 {
    if value == 0.0 {
        return 0;
    }

    let bits = value.to_bits();
    let sign = (bits >> 31) & 1;
    let exp = i32::from(((bits >> 23) & 0xFF).to_le_bytes()[0]);
    let mantissa = bits & 0x007F_FFFF;

    // Bias conversion: IEEE 754 uses 127, we use 31
    let new_exp = (exp - 127 + 31).clamp(0, 63).cast_unsigned();

    // Take top 17 bits of 23-bit mantissa
    let new_mantissa = mantissa >> 6;

    (sign << 23) | (new_exp << 17) | new_mantissa
}

/// Unpack a 24-bit float representation to a 32-bit float.
///
/// Layout: bit 23 = sign, bits 17-22 = exponent (6-bit, biased by +96 relative
/// to IEEE 754's bias of 127), bits 0-16 = mantissa (17-bit).
///
/// When the exponent field is zero the game returns ±0.0 regardless of the
/// mantissa, so we do the same.
#[must_use]
pub fn unpack_float24(packed: u32) -> f32 {
    let sign = (packed >> 23) & 1;
    let exp = (packed >> 17) & 0x3F;
    let mantissa = packed & 0x1FFFF;

    if exp == 0 {
        // Game returns ±0.0 when exponent is zero.
        return if sign != 0 { -0.0 } else { 0.0 };
    }

    // Convert exponent bias: stored + 96 = IEEE exponent (same as stored - 31 + 127)
    let new_exp = exp + 96;

    // Extend mantissa from 17 to 23 bits
    let new_mantissa = mantissa << 6;

    let bits = (sign << 31) | (new_exp << 23) | new_mantissa;
    f32::from_bits(bits)
}

/// Pack a float as a 24-bit fixed-point fraction (value × 10 000, sign-magnitude).
#[must_use]
pub fn pack_fract24(value: f32) -> u32 {
    let scaled = (value * 10_000.0)
        .round()
        .clamp(-8_388_607.0, 8_388_607.0)
        .to_i32()
        .unwrap_or(0);
    if scaled >= 0 {
        scaled.cast_unsigned() & 0x007F_FFFF
    } else {
        (scaled.unsigned_abs() & 0x007F_FFFF) | 0x0080_0000
    }
}

/// Unpack a 24-bit fixed-point fraction to `f32`.
///
/// Bit 23 is a sign flag (sign-magnitude), bits 0-22 hold the magnitude.
/// The value is `magnitude / 10 000`.
#[must_use]
pub fn unpack_fract24(packed: u32) -> f32 {
    let is_negative = (packed & 0x0080_0000) != 0;
    let magnitude = (packed & 0x007F_FFFF).to_f32().unwrap_or_default();
    let value = magnitude / 10_000.0;
    if is_negative { -value } else { value }
}

/// Pack a 24-bit signed integer (two's complement).
#[must_use]
pub fn pack_int24(value: i32) -> u32 {
    value.cast_unsigned() & 0x00FF_FFFF
}

/// Unpack a 24-bit signed integer (two's complement, sign-extended to 32 bits).
#[must_use]
pub fn unpack_int24(packed: u32) -> i32 {
    let val = packed & 0x00FF_FFFF;
    // Sign-extend from 24-bit to 32-bit
    if val & 0x0080_0000 != 0 {
        (val | 0xFF00_0000).cast_signed()
    } else {
        val.cast_signed()
    }
}

/// Pack a 24-bit unsigned integer.
#[must_use]
pub fn pack_uint24(value: u32) -> u32 {
    value & 0x00FF_FFFF
}

/// Unpack a 24-bit unsigned integer.
#[must_use]
pub fn unpack_uint24(packed: u32) -> u32 {
    packed & 0x00FF_FFFF
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
                "Float24 roundtrip failed for {value}: got {unpacked}"
            );
        }
    }

    #[test]
    fn test_int24_roundtrip() {
        // Two's complement 24-bit range: -8388608 to 8388607
        let values = [0i32, 1, -1, 1000, -1000, 8_388_607, -8_388_608];
        for value in values {
            let packed = pack_int24(value);
            let unpacked = unpack_int24(packed);
            assert_eq!(value, unpacked, "Int24 roundtrip failed for {value}");
        }
    }

    #[test]
    fn test_int24_twos_complement() {
        // -1 should pack as 0xFFFFFF (all 24 bits set)
        assert_eq!(pack_int24(-1), 0x00FF_FFFF);
        assert_eq!(unpack_int24(0x00FF_FFFF), -1);

        // -2 should pack as 0xFFFFFE
        assert_eq!(pack_int24(-2), 0x00FF_FFFE);
        assert_eq!(unpack_int24(0x00FF_FFFE), -2);

        // 1 should pack as 0x000001
        assert_eq!(pack_int24(1), 0x0000_0001);
        assert_eq!(unpack_int24(0x0000_0001), 1);
    }

    #[test]
    fn test_float24_zero_denorm() {
        // exp==0 should return ±0.0 regardless of mantissa (matches game)
        assert_eq!(unpack_float24(0).to_bits(), 0.0f32.to_bits());
        assert!(unpack_float24(0).is_sign_positive());

        // sign=1, exp=0, mantissa=0 → -0.0
        assert!(unpack_float24(0x0080_0000).is_sign_negative());
        assert_eq!(unpack_float24(0x0080_0000).to_bits(), (-0.0f32).to_bits());

        // sign=0, exp=0, mantissa=nonzero → still 0.0
        assert_eq!(unpack_float24(0x0000_0001).to_bits(), 0.0f32.to_bits());
    }

    #[test]
    fn test_fract24_roundtrip() {
        let cases: &[(u32, f32)] = &[
            (10500, 1.05),
            (0, 0.0),
            (1, 0.0001),
            (10000, 1.0),
            (0x0080_0000 | 0x2904, -1.05),
        ];
        for &(packed, expected) in cases {
            let v = unpack_fract24(packed);
            assert!(
                (v - expected).abs() < 1e-5,
                "unpack_fract24({packed}) = {v}, expected {expected}"
            );
            let repacked = pack_fract24(v);
            assert_eq!(repacked, packed, "Fract24 roundtrip failed for {expected}");
        }
    }

    #[test]
    fn test_fract24_formatting() {
        // to_string_value uses "%u.%04u" format matching the game
        assert_eq!(Variant::Fract24(1.05).to_string_value(), "1.0500");
        assert_eq!(Variant::Fract24(0.0).to_string_value(), "0.0000");
        assert_eq!(Variant::Fract24(0.0001).to_string_value(), "0.0001");
        assert_eq!(Variant::Fract24(1.0).to_string_value(), "1.0000");
        assert_eq!(Variant::Fract24(-1.05).to_string_value(), "-1.0500");
    }
}
