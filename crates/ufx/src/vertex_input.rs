//! UFXS vertex-input declarations used to construct D3D12 input layouts.

use alloc::vec::Vec;
use core::fmt;

use crate::Error;

pub(crate) const RECORD_SIZE: usize = 10;
pub(crate) const MAX_VERTEX_INPUTS: usize = 16;
const MAX_INPUT_SLOTS: usize = 8;

/// D3D vertex semantic encoded by a UFXS input record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum VertexSemantic {
    /// Vertex position.
    Position = 0,
    /// Skinning weights.
    BlendWeight = 1,
    /// Skinning joint indices.
    BlendIndices = 2,
    /// Vertex normal.
    Normal = 3,
    /// Texture coordinate.
    TexCoord = 4,
    /// Vertex tangent.
    Tangent = 5,
    /// Vertex binormal.
    Binormal = 6,
    /// Tessellation factor.
    TessFactor = 7,
    /// Vertex color.
    Color = 8,
    /// Depth value.
    Depth = 9,
    /// Sample value.
    Sample = 10,
}

impl VertexSemantic {
    fn from_raw(value: u16) -> Option<Self> {
        Some(match value {
            0 => Self::Position,
            1 => Self::BlendWeight,
            2 => Self::BlendIndices,
            3 => Self::Normal,
            4 => Self::TexCoord,
            5 => Self::Tangent,
            6 => Self::Binormal,
            7 => Self::TessFactor,
            8 => Self::Color,
            9 => Self::Depth,
            10 => Self::Sample,
            _ => return None,
        })
    }
}

impl fmt::Display for VertexSemantic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Position => "POSITION",
            Self::BlendWeight => "BLENDWEIGHT",
            Self::BlendIndices => "BLENDINDICES",
            Self::Normal => "NORMAL",
            Self::TexCoord => "TEXCOORD",
            Self::Tangent => "TANGENT",
            Self::Binormal => "BINORMAL",
            Self::TessFactor => "TESSFACTOR",
            Self::Color => "COLOR",
            Self::Depth => "DEPTH",
            Self::Sample => "SAMPLE",
        })
    }
}

/// Storage format encoded by a UFXS input record.
///
/// The discriminants are the proprietary values stored in the UFX file, not
/// `DXGI_FORMAT` values. Use [`Self::dxgi_format`] for the runtime mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum VertexFormat {
    /// `DXGI_FORMAT_R32_FLOAT`.
    R32Float = 0,
    /// `DXGI_FORMAT_R32G32_FLOAT`.
    R32G32Float = 1,
    /// `DXGI_FORMAT_R32G32B32_FLOAT`.
    R32G32B32Float = 2,
    /// `DXGI_FORMAT_R32G32B32A32_FLOAT`.
    R32G32B32A32Float = 3,
    /// `DXGI_FORMAT_R16G16_FLOAT`.
    R16G16Float = 5,
    /// `DXGI_FORMAT_R16G16B16A16_FLOAT`.
    R16G16B16A16Float = 6,
    /// `DXGI_FORMAT_R8G8B8A8_TYPELESS`.
    R8G8B8A8Typeless = 7,
    /// `DXGI_FORMAT_R16G16_SNORM`.
    R16G16Snorm = 8,
    /// `DXGI_FORMAT_R16G16B16A16_UINT`.
    R16G16B16A16Uint = 9,
    /// `DXGI_FORMAT_R16G16_UINT`.
    R16G16Uint = 10,
    /// `DXGI_FORMAT_R16G16B16A16_SNORM`.
    R16G16B16A16Snorm = 11,
    /// `DXGI_FORMAT_R16G16_UNORM`.
    R16G16Unorm = 12,
    /// `DXGI_FORMAT_R16G16B16A16_UNORM`.
    R16G16B16A16Unorm = 13,
    /// `DXGI_FORMAT_B8G8R8A8_UNORM`.
    B8G8R8A8Unorm = 14,
    /// `DXGI_FORMAT_R8G8B8A8_UINT`.
    R8G8B8A8Uint = 15,
    /// `DXGI_FORMAT_R8G8B8A8_UNORM`.
    R8G8B8A8Unorm = 16,
    /// `DXGI_FORMAT_R11G11B10_FLOAT`.
    R11G11B10Float = 17,
    /// `DXGI_FORMAT_R10G10B10A2_UINT`.
    R10G10B10A2Uint = 18,
}

impl VertexFormat {
    fn from_raw(value: u16) -> Option<Self> {
        Some(match value {
            0 => Self::R32Float,
            1 => Self::R32G32Float,
            2 => Self::R32G32B32Float,
            3 => Self::R32G32B32A32Float,
            5 => Self::R16G16Float,
            6 => Self::R16G16B16A16Float,
            7 => Self::R8G8B8A8Typeless,
            8 => Self::R16G16Snorm,
            9 => Self::R16G16B16A16Uint,
            10 => Self::R16G16Uint,
            11 => Self::R16G16B16A16Snorm,
            12 => Self::R16G16Unorm,
            13 => Self::R16G16B16A16Unorm,
            14 => Self::B8G8R8A8Unorm,
            15 => Self::R8G8B8A8Uint,
            16 => Self::R8G8B8A8Unorm,
            17 => Self::R11G11B10Float,
            18 => Self::R10G10B10A2Uint,
            _ => return None,
        })
    }

    /// Size of one element in bytes.
    #[must_use]
    pub const fn byte_size(self) -> u16 {
        match self {
            Self::R32G32B32A32Float => 16,
            Self::R32G32B32Float => 12,
            Self::R32G32Float
            | Self::R16G16B16A16Float
            | Self::R16G16B16A16Uint
            | Self::R16G16B16A16Snorm
            | Self::R16G16B16A16Unorm => 8,
            Self::R32Float
            | Self::R16G16Float
            | Self::R8G8B8A8Typeless
            | Self::R16G16Snorm
            | Self::R16G16Uint
            | Self::R16G16Unorm
            | Self::B8G8R8A8Unorm
            | Self::R8G8B8A8Uint
            | Self::R8G8B8A8Unorm
            | Self::R11G11B10Float
            | Self::R10G10B10A2Uint => 4,
        }
    }

    /// Numeric `DXGI_FORMAT` selected by the HW2 runtime.
    #[must_use]
    pub const fn dxgi_format(self) -> u32 {
        match self {
            Self::R32Float => 41,
            Self::R32G32Float => 16,
            Self::R32G32B32Float => 6,
            Self::R32G32B32A32Float => 2,
            Self::R16G16Float => 34,
            Self::R16G16B16A16Float => 10,
            Self::R8G8B8A8Typeless => 26,
            Self::R16G16Snorm => 38,
            Self::R16G16B16A16Uint => 14,
            Self::R16G16Uint => 37,
            Self::R16G16B16A16Snorm => 13,
            Self::R16G16Unorm => 35,
            Self::R16G16B16A16Unorm => 11,
            Self::B8G8R8A8Unorm => 87,
            Self::R8G8B8A8Uint => 30,
            Self::R8G8B8A8Unorm => 28,
            Self::R11G11B10Float => 25,
            Self::R10G10B10A2Uint => 24,
        }
    }
}

impl fmt::Display for VertexFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

/// One 10-byte UFXS vertex-input record decoded as the runtime uses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VertexInput {
    /// Proprietary component flags. The low two bits encode one fewer than the
    /// declared component count; higher bits carry compiler modifiers.
    pub component_flags: u16,
    /// GPU storage format.
    pub format: VertexFormat,
    /// D3D semantic name.
    pub semantic: VertexSemantic,
    /// D3D input slot.
    pub input_slot: u16,
    /// Semantic index (for example, `TEXCOORD1`).
    pub semantic_index: u8,
    /// Whether D3D advances this input once per instance rather than per vertex.
    pub per_instance: bool,
    /// Append-aligned byte offset calculated independently for this input slot.
    pub byte_offset: u16,
}

impl VertexInput {
    /// Declared vector component count decoded from the low component-flag bits.
    #[must_use]
    pub const fn component_count(self) -> u16 {
        (self.component_flags & 3) + 1
    }

    /// First byte after this element in its input slot.
    ///
    /// Returns `None` for a manually constructed value whose range exceeds the
    /// runtime's 16-bit slot offset.
    #[must_use]
    pub const fn end_offset(self) -> Option<u16> {
        self.byte_offset.checked_add(self.format.byte_size())
    }
}

pub(crate) fn parse(data: &[u8], header_offset: usize) -> Result<Vec<VertexInput>, Error> {
    let header_end = header_offset
        .checked_add(8)
        .ok_or(Error::InvalidVertexInputRange {
            offset: 0,
            count: 0,
            len: data.len(),
        })?;
    let offset = super::read_u32(data, header_offset).ok_or(Error::TruncatedVertexInputHeader {
        len: data.len(),
        needed: header_end,
    })?;
    let count =
        super::read_u32(data, header_offset + 4).ok_or(Error::TruncatedVertexInputHeader {
            len: data.len(),
            needed: header_end,
        })?;
    if count == 0 {
        return Ok(Vec::new());
    }

    let start = usize::try_from(offset).map_err(|_| Error::InvalidVertexInputRange {
        offset,
        count,
        len: data.len(),
    })?;
    let count_usize = usize::try_from(count).map_err(|_| Error::InvalidVertexInputRange {
        offset,
        count,
        len: data.len(),
    })?;
    if count_usize > MAX_VERTEX_INPUTS {
        return Err(Error::TooManyVertexInputs { count });
    }
    let byte_count =
        count_usize
            .checked_mul(RECORD_SIZE)
            .ok_or(Error::InvalidVertexInputRange {
                offset,
                count,
                len: data.len(),
            })?;
    let end = start
        .checked_add(byte_count)
        .ok_or(Error::InvalidVertexInputRange {
            offset,
            count,
            len: data.len(),
        })?;
    let records = data.get(start..end).ok_or(Error::InvalidVertexInputRange {
        offset,
        count,
        len: data.len(),
    })?;

    let mut slot_offsets = [0u16; MAX_INPUT_SLOTS];
    let mut inputs = Vec::with_capacity(count_usize);
    for (input_index, record) in records.as_chunks::<RECORD_SIZE>().0.iter().enumerate() {
        let component_flags = read_u16(record, 0);
        let format_raw = read_u16(record, 2);
        let semantic_raw = read_u16(record, 4);
        let input_slot = read_u16(record, 6);
        let format = VertexFormat::from_raw(format_raw).ok_or(Error::InvalidVertexInputValue {
            input_index,
            field: "format",
            value: format_raw,
        })?;
        let semantic =
            VertexSemantic::from_raw(semantic_raw).ok_or(Error::InvalidVertexInputValue {
                input_index,
                field: "semantic",
                value: semantic_raw,
            })?;
        let slot = usize::from(input_slot);
        let byte_offset = *slot_offsets
            .get(slot)
            .ok_or(Error::InvalidVertexInputValue {
                input_index,
                field: "input slot",
                value: input_slot,
            })?;
        slot_offsets[slot] = byte_offset
            .checked_add(format.byte_size())
            .ok_or(Error::VertexInputStrideOverflow { input_slot })?;
        inputs.push(VertexInput {
            component_flags,
            format,
            semantic,
            input_slot,
            semantic_index: record[8],
            per_instance: record[9] != 0,
            byte_offset,
        });
    }
    Ok(inputs)
}

fn read_u16(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([data[offset], data[offset + 1]])
}
