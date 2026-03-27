//! Vertex format types and packing/unpacking logic.
//!
//! This module groups the vertex element type definitions and the UnivertPacker
//! which together describe how vertex attributes are packed in UGX vertex buffers.

pub mod element;
pub mod packer;

// Re-export primary types for convenient access.
pub use element::VertexElementType;
pub use packer::{MAX_UV, UnivertPacker, UnpackedVertex};

use crate::error::Result;

/// Unpack a single HW2 vertex whose layout is inferred from `vert_size`.
///
/// HW2 vertex formats (all sizes in bytes):
///
/// |  Size | Layout                                                              |
/// |------:|---------------------------------------------------------------------|
/// |     8 | pos(half4)                                                          |
/// |    12 | pos(half4) + uv(half2)                                              |
/// |    20 | pos(half4) + uv(half2) + normal(Dec3N) + tangent(Dec3N)             |
/// |    24 | pos(8) + uv(4) + normal(4) + tangent(4) + color(4)                 |
/// |    28 | rigid:  base20 + uv2(4) + color(4)                                 |
/// |       | skinned: base20 + indices(UByte4) + weights(UByte4N)                |
/// |    32 | rigid:  base20 + uv2(4) + color(4) + color2(4)                     |
/// |       | skinned: base20 + indices(4) + weights(4) + color(4)               |
/// |    36 | skinned: base20 + indices(4) + weights(4) + color(4) + color2(4)    |
pub(crate) fn unpack_hw2_vertex(
    data: &[u8],
    pos: &mut usize,
    vert_size: usize,
    is_skinned: bool,
) -> Result<UnpackedVertex> {
    let start = *pos;
    let mut v = UnpackedVertex::default();

    // -- Position: always first 8 bytes (HalfFloat4) --
    let p = VertexElementType::HalfFloat4.unpack(data, pos)?;
    v.position = [p[0], p[1], p[2]];

    if vert_size <= 8 {
        *pos = start + vert_size;
        return Ok(v);
    }

    // -- UV0: next 4 bytes (HalfFloat2) --
    let uv = VertexElementType::HalfFloat2.unpack(data, pos)?;
    v.texcoords[0] = [uv[0], uv[1]];
    v.num_texcoords = 1;

    if vert_size <= 12 {
        *pos = start + vert_size;
        return Ok(v);
    }

    // -- Normal + Tangent: 4 bytes each (Dec3N) --
    let n = VertexElementType::Dec3N.unpack(data, pos)?;
    v.normal = [n[0], n[1], n[2]];

    let t = VertexElementType::Dec3N.unpack(data, pos)?;
    v.tangent = [t[0], t[1], t[2], t[3]];

    // We're now at 20 bytes consumed.
    let remaining = vert_size - 20;

    if remaining == 0 {
        return Ok(v);
    }

    if is_skinned {
        // Skinned: indices(4) + weights(4), then optional color(s)
        if remaining >= 8 {
            v.bone_indices = VertexElementType::UByte4.unpack_as_indices(data, pos)?;
            v.bone_weights = VertexElementType::UByte4N.unpack(data, pos)?;
        }
        if remaining >= 12 {
            v.diffuse = VertexElementType::D3DColor.unpack(data, pos)?;
        }
    } else {
        // Rigid: optional color/uv2
        if remaining >= 4 {
            // 24-byte: color; 28+: uv2 first
            if remaining >= 8 {
                let uv2 = VertexElementType::HalfFloat2.unpack(data, pos)?;
                v.texcoords[1] = [uv2[0], uv2[1]];
                v.num_texcoords = 2;
            }
            // color
            if remaining >= 4 + (if remaining >= 8 { 4 } else { 0 }) {
                v.diffuse = VertexElementType::D3DColor.unpack(data, pos)?;
            }
        }
    }

    // Ensure we advance exactly vert_size bytes regardless of what we consumed
    *pos = start + vert_size;

    Ok(v)
}
