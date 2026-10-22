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

/// Byte order of color and skin attributes in an HW2 skinned vertex.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Hw2SkinOrder {
    /// Bone indices and weights precede an optional color.
    #[default]
    SkinThenColor,
    /// Color precedes bone indices and weights.
    ColorThenSkin,
}

/// Unpack a single HW2 vertex whose layout is inferred from `vert_size`.
///
/// HW2 vertex formats (all sizes in bytes):
///
/// |  Size | Layout                                                              |
/// |------:|---------------------------------------------------------------------|
/// |     8 | pos(half4)                                                          |
/// |    12 | pos(half4) + uv(half2)                                              |
/// |    20 | pos(half4) + uv(half2) + normal(Dec3N) + tangent(Dec3N)             |
/// |    24 | rigid: base20 + color(BGRA8)                                        |
/// |    28 | rigid: base20 + color(4) + trailing payload(4)                      |
/// |       | skinned: base20 + indices(UByte4) + weights(UByte4N)                |
/// |    32 | rigid: base20 + color(4) + trailing payload(8)                      |
/// |       | skinned: base20 + indices(4) + weights(4) + color(4)               |
/// |    36 | skinned: base20 + indices(4) + weights(4) + color(4) + payload(4)  |
///
/// HW2's UFX input declaration controls the attributes actually consumed by
/// D3D12. Retail rigid sections can have a larger buffer stride than that
/// declaration, so bytes after the first color are deliberately ignored.
pub(crate) fn unpack_hw2_vertex(
    data: &[u8],
    pos: &mut usize,
    vert_size: usize,
    is_skinned: bool,
    skin_order: Hw2SkinOrder,
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

    if is_skinned && skin_order == Hw2SkinOrder::ColorThenSkin {
        if remaining < 12 {
            return Err(Error::UnsupportedFormat(format!(
                "color-first skinned HW2 vertex stride {vert_size} truncates its color or skin data"
            )));
        }
        v.diffuse = VertexElementType::D3DColor.unpack(data, pos)?;
        remaining -= 4;
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

    if remaining >= 4 && skin_order == Hw2SkinOrder::SkinThenColor {
        v.diffuse = VertexElementType::D3DColor.unpack(data, pos)?;
    }

    // Ensure we advance exactly vert_size bytes regardless of what we consumed
    *pos = end;

    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn assert_float_bits<const N: usize>(actual: [f32; N], expected: [f32; N]) {
        assert_eq!(actual.map(f32::to_bits), expected.map(f32::to_bits));
    }

    #[test]
    fn rigid_stride_28_reads_color_before_trailing_payload() {
        let mut data = vec![0; 20];
        data.extend_from_slice(&[0x20, 0x40, 0x80, 0xFF]);
        data.extend_from_slice(&[0x00, 0x7E, 0x00, 0x7E]);
        let mut position = 0;

        let vertex =
            unpack_hw2_vertex(&data, &mut position, 28, false, Hw2SkinOrder::SkinThenColor)
                .unwrap();

        assert_eq!(position, 28);
        assert_eq!(vertex.num_texcoords, 1);
        assert_float_bits(
            vertex.diffuse,
            [128.0 / 255.0, 64.0 / 255.0, 32.0 / 255.0, 1.0],
        );
        assert!(
            vertex
                .texcoords
                .iter()
                .flatten()
                .all(|component| component.is_finite())
        );
    }

    #[test]
    fn skinned_stride_28_uses_the_tail_for_skin_data() {
        let mut data = vec![0; 20];
        data.extend_from_slice(&[1, 2, 3, 4]);
        data.extend_from_slice(&[255, 0, 0, 0]);
        let mut position = 0;

        let vertex =
            unpack_hw2_vertex(&data, &mut position, 28, true, Hw2SkinOrder::SkinThenColor).unwrap();

        assert_eq!(vertex.bone_indices, [1, 2, 3, 4]);
        assert_float_bits(vertex.bone_weights, [1.0, 0.0, 0.0, 0.0]);
        assert_float_bits(vertex.diffuse, [0.0; 4]);
    }

    #[test]
    fn color_first_skinned_vertex_uses_the_declared_order() {
        let mut data = vec![0; 20];
        data.extend_from_slice(&[0x20, 0x40, 0x80, 0xFF]);
        data.extend_from_slice(&[1, 2, 3, 4]);
        data.extend_from_slice(&[255, 0, 0, 0]);
        let mut position = 0;

        let vertex =
            unpack_hw2_vertex(&data, &mut position, 32, true, Hw2SkinOrder::ColorThenSkin).unwrap();

        assert_eq!(vertex.bone_indices, [1, 2, 3, 4]);
        assert_float_bits(vertex.bone_weights, [1.0, 0.0, 0.0, 0.0]);
        assert_float_bits(
            vertex.diffuse,
            [128.0 / 255.0, 64.0 / 255.0, 32.0 / 255.0, 1.0],
        );
    }
}
