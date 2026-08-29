//! Vertex format types and packing/unpacking logic.
//!
//! This module groups the vertex element type definitions and the `UnivertPacker`
//! which together describe how vertex attributes are packed in UGX vertex buffers.

pub mod element;
pub mod packer;

use alloc::format;

// Re-export primary types for convenient access.
pub use element::VertexElementType;
pub use packer::{MAX_UV, UnivertPacker, UnpackedVertex};

use crate::error::{Error, Result};

/// Unpack a single HW2 vertex whose layout is inferred from `vert_size`.
///
/// HW2 vertex formats (all sizes in bytes):
///
/// |  Size | Layout                                                              |
/// |------:|---------------------------------------------------------------------|
/// |     8 | pos(half4)                                                          |
/// |    12 | pos(half4) + uv(half2)                                              |
/// |    20 | pos(half4) + uv(half2) + normal(Dec3N) + tangent(Dec3N)             |
/// |    24 | base20 + color(4)                                                   |
/// |    28 | rigid: base20 + uv2(4) + color(4)                                   |
/// |       | skinned: base20 + indices(UByte4) + weights(UByte4N)                |
/// |    32 | rigid: base20 + uv2(4) + uv3(4) + color(4)                          |
/// |       | skinned: base20 + indices(4) + weights(4) + color(4)               |
/// |    36 | skinned: base20 + indices(4) + weights(4) + uv2(4) + color(4)      |
pub(crate) fn unpack_hw2_vertex(
    data: &[u8],
    pos: &mut usize,
    vert_size: usize,
    is_skinned: bool,
) -> Result<UnpackedVertex> {
    let start = *pos;
    let end = start
        .checked_add(vert_size)
        .ok_or(Error::SizeOverflow("HW2 vertex range"))?;
    if data.get(start..end).is_none() {
        return Err(Error::UnexpectedEof {
            context: "HW2 vertex".into(),
        });
    }
    if vert_size < 8 {
        return Err(Error::UnsupportedFormat(format!(
            "HW2 vertex stride {vert_size} is smaller than its position"
        )));
    }
    let mut v = UnpackedVertex::default();

    // -- Position: always first 8 bytes (HalfFloat4) --
    let p = VertexElementType::HalfFloat4.unpack(data, pos)?;
    v.position = [p[0], p[1], p[2]];

    if vert_size <= 8 {
        *pos = end;
        return Ok(v);
    }

    if vert_size < 12 {
        return Err(Error::UnsupportedFormat(format!(
            "HW2 vertex stride {vert_size} truncates its first texture coordinate"
        )));
    }

    // -- UV0: next 4 bytes (HalfFloat2) --
    let uv = VertexElementType::HalfFloat2.unpack(data, pos)?;
    v.texcoords[0] = [uv[0], uv[1]];
    v.num_texcoords = 1;

    if vert_size <= 12 {
        *pos = end;
        return Ok(v);
    }

    if vert_size < 16 {
        return Err(Error::UnsupportedFormat(format!(
            "HW2 vertex stride {vert_size} truncates its normal"
        )));
    }

    // -- Normal: 4 bytes (Dec3N) --
    let n = VertexElementType::Dec3N.unpack(data, pos)?;
    v.normal = [n[0], n[1], n[2]];

    if vert_size <= 16 {
        *pos = end;
        return Ok(v);
    }

    if vert_size < 20 {
        return Err(Error::UnsupportedFormat(format!(
            "HW2 vertex stride {vert_size} truncates its tangent"
        )));
    }

    // -- Tangent: 4 bytes (Dec3N) --
    let t = VertexElementType::Dec3N.unpack(data, pos)?;
    v.tangent = [t[0], t[1], t[2], t[3]];

    // We're now at 20 bytes consumed.
    let mut remaining = vert_size - 20;

    if remaining == 0 {
        return Ok(v);
    }

    if is_skinned {
        if remaining < 8 {
            return Err(Error::UnsupportedFormat(format!(
                "skinned HW2 vertex stride {vert_size} truncates its skin data"
            )));
        }
        v.bone_indices = VertexElementType::UByte4.unpack_as_indices(data, pos)?;
        v.bone_weights = VertexElementType::UByte4N.unpack(data, pos)?;
        remaining -= 8;
    }

    if !remaining.is_multiple_of(4) {
        return Err(Error::UnsupportedFormat(format!(
            "HW2 vertex stride {vert_size} has a partial trailing element"
        )));
    }
    let trailing_elements = remaining / 4;
    let extra_texcoords = trailing_elements.saturating_sub(1);
    if extra_texcoords > MAX_UV - 1 {
        return Err(Error::UnsupportedFormat(format!(
            "HW2 vertex stride {vert_size} contains too many texture coordinates"
        )));
    }
    for texcoord_index in 1..=extra_texcoords {
        let uv = VertexElementType::HalfFloat2.unpack(data, pos)?;
        v.texcoords[texcoord_index] = [uv[0], uv[1]];
        v.num_texcoords = texcoord_index + 1;
    }
    if trailing_elements > 0 {
        v.diffuse = VertexElementType::D3DColor.unpack(data, pos)?;
    }

    // Ensure we advance exactly vert_size bytes regardless of what we consumed
    *pos = end;

    Ok(v)
}
