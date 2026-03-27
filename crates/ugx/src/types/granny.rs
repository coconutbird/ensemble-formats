//! Granny2 type system — member types, type definitions, and variant values.
//!
//! This module contains the data structures used to represent the Granny2
//! serialization format's type system. The Granny2 format uses a self-describing
//! type tree where each struct is described by an array of `GrannyTypeMember`
//! entries, and data is stored as recursive `GrannyVariant` values.
//!
//! These types are used by both the reader (to parse bone extended data from
//! chunk 0x703) and the writer (to re-serialize the type tree and data blobs).

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

/// Granny2 member type IDs.
///
/// From the engine's `GrannyMemberTypeSizeTable` at `0x141462A2C` and
/// `Granny_TraverseTreeForRebase` at `0x1408CED00`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum GrannyMemberType {
    /// Terminator — marks the end of a type definition array.
    End = 0,
    /// Inline nested struct (recursive, size computed from referenced type).
    Inline = 1,
    /// Single pointer to data described by `reference_type`.
    Reference = 2,
    /// Pointer to an array whose count is stored as an i32 before the pointer.
    ReferenceToArray = 3,
    /// Array of pointers (count as i32 before pointer array).
    ArrayOfReferences = 4,
    /// Variant reference — `{type_def_ptr, data_ptr}` pair.
    VariantReference = 5,
    /// Pointer to a variant array (count + type_def + data).
    ReferenceToVariantArray = 7,
    /// Pointer to null-terminated string.
    StringMember = 8,
    /// 68-byte Granny transform (flags + pos + quat + scale_shear).
    Transform = 9,
    /// 32-bit float.
    Real32 = 10,
    /// 8-bit signed integer.
    Int8 = 11,
    /// 8-bit unsigned integer.
    UInt8 = 12,
    /// Normalized 8-bit signed integer.
    BinormalInt8 = 13,
    /// Normalized 8-bit unsigned integer.
    NormalUInt8 = 14,
    /// 16-bit signed integer.
    Int16 = 15,
    /// 16-bit unsigned integer.
    UInt16 = 16,
    /// Normalized 16-bit signed integer.
    BinormalInt16 = 17,
    /// Normalized 16-bit unsigned integer.
    NormalUInt16 = 18,
    /// 32-bit signed integer.
    Int32 = 19,
    /// 32-bit unsigned integer.
    UInt32 = 20,
    /// 16-bit half-float.
    Real16 = 21,
    /// Empty reference (null pointer, no data).
    EmptyReference = 22,
}

impl GrannyMemberType {
    /// Convert from raw u32. Returns `None` for unknown type IDs.
    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            0 => Some(Self::End),
            1 => Some(Self::Inline),
            2 => Some(Self::Reference),
            3 => Some(Self::ReferenceToArray),
            4 => Some(Self::ArrayOfReferences),
            5 => Some(Self::VariantReference),
            7 => Some(Self::ReferenceToVariantArray),
            8 => Some(Self::StringMember),
            9 => Some(Self::Transform),
            10 => Some(Self::Real32),
            11 => Some(Self::Int8),
            12 => Some(Self::UInt8),
            13 => Some(Self::BinormalInt8),
            14 => Some(Self::NormalUInt8),
            15 => Some(Self::Int16),
            16 => Some(Self::UInt16),
            17 => Some(Self::BinormalInt16),
            18 => Some(Self::NormalUInt16),
            19 => Some(Self::Int32),
            20 => Some(Self::UInt32),
            21 => Some(Self::Real16),
            22 => Some(Self::EmptyReference),
            _ => None,
        }
    }

    /// Size in bytes of a single element of this type (not counting ArrayWidth).
    ///
    /// Returns `None` for types whose size depends on context (Inline, End).
    pub fn unit_size(self) -> Option<usize> {
        match self {
            Self::End => Some(0),
            Self::Inline => None,                      // computed recursively
            Self::Reference => Some(8),                // u64 pointer
            Self::ReferenceToArray => Some(12),        // u32 count + u64 pointer
            Self::ArrayOfReferences => Some(12),       // u32 count + u64 pointer
            Self::VariantReference => Some(16),        // u64 type_ptr + u64 data_ptr
            Self::ReferenceToVariantArray => Some(20), // u32 count + u64 type_ptr + u64 data_ptr (but engine says 0)
            Self::StringMember => Some(8),             // u64 pointer
            Self::Transform => Some(68),               // 4 + 12 + 16 + 36
            Self::Real32 | Self::Int32 | Self::UInt32 => Some(4),
            Self::Int8 | Self::UInt8 | Self::BinormalInt8 | Self::NormalUInt8 => Some(1),
            Self::Int16
            | Self::UInt16
            | Self::BinormalInt16
            | Self::NormalUInt16
            | Self::Real16 => Some(2),
            Self::EmptyReference => Some(0),
        }
    }
}

/// A single member in a Granny2 type definition array.
///
/// On disk this is 44 bytes (11 DWORDs) with the layout:
/// ```text
/// +0x00  u32  MemberType
/// +0x04  u64  Name          (pointer → null-terminated string)
/// +0x0C  u64  ReferenceType (pointer → nested GrannyDataTypeDefinition[])
/// +0x14  u32  ArrayWidth    (element count, 0 or 1 for scalar)
/// +0x18  u32  Extra[3]
/// +0x24  u32  Unused[2]
/// ```
#[derive(Debug, Clone)]
pub struct GrannyTypeMember {
    /// The member type (Real32, String, Reference, etc.).
    pub member_type: GrannyMemberType,
    /// Member name (e.g. "TrackMask", "UserDefinedProperties").
    pub name: String,
    /// For Reference/Inline types, the nested type definition.
    pub reference_type: Option<Vec<GrannyTypeMember>>,
    /// Array width (0 or 1 = scalar, >1 = fixed-size array).
    pub array_width: u32,
    /// Extra data (3 × u32).
    pub extra: [u32; 3],
}

/// A parsed Granny2 variant value.
///
/// This is the recursive data tree described by `GrannyDataTypeDefinition` arrays.
/// Each bone's ExtendedData is one of these.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum GrannyVariant {
    /// A struct with named fields.
    Struct(Vec<(String, GrannyVariant)>),
    /// 32-bit float (possibly an array if ArrayWidth > 1).
    Real32(Vec<f32>),
    /// 8-bit signed integer array.
    Int8(Vec<i8>),
    /// 8-bit unsigned integer array.
    UInt8(Vec<u8>),
    /// 16-bit signed integer array.
    Int16(Vec<i16>),
    /// 16-bit unsigned integer array.
    UInt16(Vec<u16>),
    /// 32-bit signed integer array.
    Int32(Vec<i32>),
    /// 32-bit unsigned integer array.
    UInt32(Vec<u32>),
    /// Null-terminated string.
    StringVal(String),
    /// Reference to another variant (possibly null).
    Reference(Option<Box<GrannyVariant>>),
    /// Variant reference (type + data, used for ExtendedData itself).
    VariantReference(Option<Box<GrannyVariant>>),
    /// Empty/null reference.
    #[default]
    Empty,
    /// Raw bytes for types we don't fully parse (Transform, etc.).
    RawBytes(Vec<u8>),
}
