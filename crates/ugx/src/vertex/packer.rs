//! UnivertPacker - vertex format descriptor.
//!
//! The UnivertPacker describes how vertex attributes are packed in the vertex buffer.
//! It uses a string-based "pack order" to specify which attributes are present and
//! in what order, along with type specifiers for each attribute.
//!
//! # C++ Equivalent
//!
//! This corresponds to `Unigeom::BUnpacker` from the original source.
//!
//! # Pack Order String Format
//!
//! The `pack_order` string defines the sequence of vertex attributes in the packed
//! vertex data. Each character (or character+digit) represents an attribute:
//!
//! | Character | Meaning            | Example                      |
//! |-----------|--------------------|-----------------------------|
//! | `P`       | Position           | Float4 (16 bytes)           |
//! | `B#`      | Basis (T/B/N)      | 3x Dec3N (12 bytes)         |
//! | `N`       | Normal only        | Dec3N (4 bytes)             |
//! | `T#`      | TexCoord set #     | HalfFloat2 (4 bytes)        |
//! | `S`       | Skin (idx+weights) | UByte4 + UByte4N (8 bytes)  |
//! | `D`       | Diffuse color      | D3DColor (4 bytes)          |
//! | `I`       | Vertex index       | Short2 (4 bytes)            |
//! | `X#`      | Basis scale        | HalfFloat2 (4 bytes)        |
//!
//! ## Common Pack Order Examples
//!
//! - `"PBNT0S"` - Position, Basis, Normal, TexCoord0, Skin (skinned mesh)
//! - `"PNT0"` - Position, Normal, TexCoord0 (static mesh)
//! - `"PB0NT0T1S"` - Multiple texcoords (e.g., diffuse + lightmap)
//!
//! # On-Disk Layout (84 bytes)
//!
//! The packed format in BCachedData chunk differs from in-memory (104 bytes on x64):
//! - Strings are stored as offsets (8 bytes each)
//! - Element types are stored as u32 enums
//!
//! See `ugx.rs` for the detailed byte layout.

use alloc::string::String;
use alloc::vec::Vec;

use crate::error::{Error, Result};
use crate::vertex::element::VertexElementType;

/// Vertex element specifiers used in pack order strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum VertexElementSpec {
    /// Position (P).
    Position,
    /// Basis vectors - tangent/binormal (B#).
    Basis(u8),
    /// Normal (N).
    Normal,
    /// Texture coordinates (T#).
    TexCoords(u8),
    /// Skin - bone indices and weights (S).
    Skin,
    /// Diffuse color (D).
    Diffuse,
    /// Index (I).
    Index,
    /// Basis scale (X#).
    BasisScale(u8),
}

/// Unpacked vertex data.
#[derive(Debug, Clone, Default)]
pub struct UnpackedVertex {
    /// Position [x, y, z].
    pub position: [f32; 3],
    /// Normal [x, y, z].
    pub normal: [f32; 3],
    /// Tangent [x, y, z, w] (w is handedness).
    pub tangent: [f32; 4],
    /// Binormal [x, y, z, w].
    pub binormal: [f32; 4],
    /// Texture coordinates (up to MAX_UV sets).
    pub texcoords: [[f32; 2]; MAX_UV],
    /// Number of texcoord sets.
    pub num_texcoords: usize,
    /// Bone indices [0-3].
    pub bone_indices: [u16; 4],
    /// Bone weights [0-3].
    pub bone_weights: [f32; 4],
    /// Diffuse color [r, g, b, a].
    pub diffuse: [f32; 4],
    /// Vertex index.
    pub index: i16,
}

/// Maximum number of UV coordinate sets.
pub const MAX_UV: usize = 8;

/// UnivertPacker - describes vertex format and unpacks vertices.
#[derive(Debug, Clone)]
pub struct UnivertPacker {
    /// Position element type.
    pub pos_type: VertexElementType,
    /// Basis (tangent/binormal) element type.
    pub basis_type: VertexElementType,
    /// Basis scale element type.
    pub basis_scale_type: VertexElementType,
    /// Tangent element type.
    pub tangent_type: VertexElementType,
    /// Normal element type.
    pub normal_type: VertexElementType,
    /// UV element types (up to 8 sets).
    pub uv_types: [VertexElementType; MAX_UV],
    /// Bone indices element type.
    pub indices_type: VertexElementType,
    /// Bone weights element type.
    pub weights_type: VertexElementType,
    /// Diffuse color element type.
    pub diffuse_type: VertexElementType,
    /// Index element type.
    pub index_type: VertexElementType,
    /// Pack order string (e.g., "PB0NT0S").
    pub pack_order: String,
    /// Declaration order string.
    pub decl_order: String,
}

impl Default for UnivertPacker {
    fn default() -> Self {
        Self {
            pos_type: VertexElementType::Float3,
            basis_type: VertexElementType::Float4,
            basis_scale_type: VertexElementType::Float2,
            tangent_type: VertexElementType::Ignore,
            normal_type: VertexElementType::Float3,
            uv_types: [VertexElementType::Float2; MAX_UV],
            indices_type: VertexElementType::UByte4,
            weights_type: VertexElementType::Float4,
            diffuse_type: VertexElementType::Float4,
            index_type: VertexElementType::Short2,
            pack_order: String::new(),
            decl_order: String::new(),
        }
    }
}

impl UnivertPacker {
    /// Read a UnivertPacker from raw bytes (legacy unpacked format).
    /// Note: The packed format is read differently in ugx.rs.
    #[allow(dead_code)]
    pub fn read(data: &[u8], pos: &mut usize) -> Result<Self> {
        fn read_u8(data: &[u8], pos: &mut usize) -> Result<u8> {
            if *pos >= data.len() {
                return Err(Error::UnexpectedEof {
                    context: String::from("u8"),
                });
            }
            let v = data[*pos];
            *pos += 1;
            Ok(v)
        }

        let pos_type = VertexElementType::try_from(read_u8(data, pos)?)?;
        let basis_type = VertexElementType::try_from(read_u8(data, pos)?)?;
        let basis_scale_type = VertexElementType::try_from(read_u8(data, pos)?)?;
        let normal_type = VertexElementType::try_from(read_u8(data, pos)?)?;
        let uv_type = VertexElementType::try_from(read_u8(data, pos)?)?;
        let indices_type = VertexElementType::try_from(read_u8(data, pos)?)?;
        let weights_type = VertexElementType::try_from(read_u8(data, pos)?)?;
        let diffuse_type = VertexElementType::try_from(read_u8(data, pos)?)?;
        let index_type = VertexElementType::try_from(read_u8(data, pos)?)?;

        let pack_order = read_big_string(data, pos)?;
        let decl_order = read_big_string(data, pos)?;

        Ok(Self {
            pos_type,
            basis_type,
            basis_scale_type,
            tangent_type: VertexElementType::Ignore,
            normal_type,
            uv_types: [uv_type; MAX_UV],
            indices_type,
            weights_type,
            diffuse_type,
            index_type,
            pack_order,
            decl_order,
        })
    }

    /// Calculate the size in bytes of a single vertex.
    pub fn vertex_size(&self) -> usize {
        let mut size = 0;
        let mut chars = self.pack_order.chars().peekable();

        while let Some(c) = chars.next() {
            match c.to_ascii_uppercase() {
                'P' => size += self.pos_type.size(),
                'B' => {
                    // Skip the index digit
                    chars.next();
                    // Basis has tangent + binormal
                    size += self.basis_type.size() * 2;
                }
                'A' => {
                    // Tangent only (unlike B which has tangent+binormal)
                    chars.next();
                    size += self.tangent_type.size();
                }
                'X' => {
                    chars.next();
                    size += self.basis_scale_type.size();
                }
                'N' => size += self.normal_type.size(),
                'T' => {
                    let idx = chars.next().and_then(|c| c.to_digit(10)).unwrap_or(0) as usize;
                    if idx < MAX_UV {
                        size += self.uv_types[idx].size();
                    }
                }
                'S' => {
                    size += self.indices_type.size();
                    size += self.weights_type.size();
                }
                'D' => size += self.diffuse_type.size(),
                'I' => size += self.index_type.size(),
                _ => {}
            }
        }

        size
    }

    /// Unpack a single vertex from raw bytes.
    pub fn unpack_vertex(&self, data: &[u8], pos: &mut usize) -> Result<UnpackedVertex> {
        let mut vertex = UnpackedVertex::default();
        let mut chars = self.pack_order.chars().peekable();

        while let Some(c) = chars.next() {
            match c.to_ascii_uppercase() {
                'P' => {
                    let v = self.pos_type.unpack(data, pos)?;
                    vertex.position = [v[0], v[1], v[2]];
                }
                'B' => {
                    let _idx = chars.next().and_then(|c| c.to_digit(10)).unwrap_or(0);
                    vertex.tangent = self.basis_type.unpack(data, pos)?;
                    vertex.binormal = self.basis_type.unpack(data, pos)?;
                }
                'A' => {
                    let _idx = chars.next().and_then(|c| c.to_digit(10)).unwrap_or(0);
                    vertex.tangent = self.tangent_type.unpack(data, pos)?;
                }
                'X' => {
                    let _idx = chars.next().and_then(|c| c.to_digit(10)).unwrap_or(0);
                    let scale = self.basis_scale_type.unpack(data, pos)?;
                    vertex.tangent[3] = scale[0];
                    vertex.binormal[3] = scale[1];
                }
                'N' => {
                    let v = self.normal_type.unpack(data, pos)?;
                    vertex.normal = [v[0], v[1], v[2]];
                }
                'T' => {
                    let idx = chars.next().and_then(|c| c.to_digit(10)).unwrap_or(0) as usize;
                    if idx < MAX_UV {
                        let v = self.uv_types[idx].unpack(data, pos)?;
                        vertex.texcoords[idx] = [v[0], v[1]];
                        if idx >= vertex.num_texcoords {
                            vertex.num_texcoords = idx + 1;
                        }
                    }
                }
                'S' => {
                    vertex.bone_indices = self.indices_type.unpack_as_indices(data, pos)?;
                    vertex.bone_weights = self.weights_type.unpack(data, pos)?;
                }
                'D' => {
                    vertex.diffuse = self.diffuse_type.unpack(data, pos)?;
                }
                'I' => {
                    let v = self.index_type.unpack(data, pos)?;
                    vertex.index = v[0] as i16;
                }
                _ => {}
            }
        }

        Ok(vertex)
    }

    /// Pack a single vertex into raw bytes (inverse of `unpack_vertex()`).
    pub fn pack_vertex(&self, out: &mut Vec<u8>, vertex: &UnpackedVertex) {
        let mut chars = self.pack_order.chars().peekable();

        while let Some(c) = chars.next() {
            match c.to_ascii_uppercase() {
                'P' => {
                    let v = [
                        vertex.position[0],
                        vertex.position[1],
                        vertex.position[2],
                        1.0,
                    ];
                    self.pos_type.pack(out, v);
                }
                'B' => {
                    let _idx = chars.next().and_then(|c| c.to_digit(10)).unwrap_or(0);
                    self.basis_type.pack(out, vertex.tangent);
                    self.basis_type.pack(out, vertex.binormal);
                }
                'A' => {
                    let _idx = chars.next().and_then(|c| c.to_digit(10)).unwrap_or(0);
                    self.tangent_type.pack(out, vertex.tangent);
                }
                'X' => {
                    let _idx = chars.next().and_then(|c| c.to_digit(10)).unwrap_or(0);
                    let scale = [vertex.tangent[3], vertex.binormal[3], 0.0, 1.0];
                    self.basis_scale_type.pack(out, scale);
                }
                'N' => {
                    let v = [vertex.normal[0], vertex.normal[1], vertex.normal[2], 1.0];
                    self.normal_type.pack(out, v);
                }
                'T' => {
                    let idx = chars.next().and_then(|c| c.to_digit(10)).unwrap_or(0) as usize;
                    if idx < MAX_UV {
                        let v = [vertex.texcoords[idx][0], vertex.texcoords[idx][1], 0.0, 1.0];
                        self.uv_types[idx].pack(out, v);
                    }
                }
                'S' => {
                    self.indices_type.pack_as_indices(out, vertex.bone_indices);
                    self.weights_type.pack(out, vertex.bone_weights);
                }
                'D' => {
                    self.diffuse_type.pack(out, vertex.diffuse);
                }
                'I' => {
                    let v = [vertex.index as f32, 0.0, 0.0, 1.0];
                    self.index_type.pack(out, v);
                }
                _ => {}
            }
        }
    }

    /// Check if this packer is empty (no pack order).
    pub fn is_empty(&self) -> bool {
        self.pack_order.is_empty()
    }
}

/// Read a "BigString" - length-prefixed string used in UGX.
fn read_big_string(data: &[u8], pos: &mut usize) -> Result<String> {
    let end = *pos + 4;
    if end > data.len() {
        return Err(Error::UnexpectedEof {
            context: String::from("big_string length"),
        });
    }
    let len =
        u32::from_le_bytes([data[*pos], data[*pos + 1], data[*pos + 2], data[*pos + 3]]) as usize;
    *pos = end;

    if len == 0 {
        return Ok(String::new());
    }

    let str_end = *pos + len;
    if str_end > data.len() {
        return Err(Error::UnexpectedEof {
            context: String::from("big_string data"),
        });
    }
    let mut bytes = data[*pos..str_end].to_vec();
    *pos = str_end;

    // Remove null terminator if present
    if bytes.last() == Some(&0) {
        bytes.pop();
    }

    Ok(String::from_utf8(bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    #[test]
    fn test_vertex_size_calculation() {
        let packer = UnivertPacker {
            pack_order: "PNT0".to_string(),
            pos_type: VertexElementType::Float3,
            normal_type: VertexElementType::Float3,
            uv_types: [VertexElementType::Float2; MAX_UV],
            ..Default::default()
        };

        // Position (12) + Normal (12) + UV (8) = 32
        assert_eq!(packer.vertex_size(), 32);
    }

    #[test]
    fn test_vertex_size_with_skin() {
        let packer = UnivertPacker {
            pack_order: "PNT0S".to_string(),
            pos_type: VertexElementType::Float3,
            normal_type: VertexElementType::Float3,
            uv_types: [VertexElementType::Float2; MAX_UV],
            indices_type: VertexElementType::UByte4,
            weights_type: VertexElementType::UByte4N,
            ..Default::default()
        };

        // Position (12) + Normal (12) + UV (8) + Indices (4) + Weights (4) = 40
        assert_eq!(packer.vertex_size(), 40);
    }

    #[test]
    fn test_pack_unpack_vertex_roundtrip() {
        let packer = UnivertPacker {
            pack_order: "PNT0".to_string(),
            pos_type: VertexElementType::Float3,
            normal_type: VertexElementType::Float3,
            uv_types: [VertexElementType::Float2; MAX_UV],
            ..Default::default()
        };

        let original = UnpackedVertex {
            position: [1.0, 2.0, 3.0],
            normal: [0.0, 1.0, 0.0],
            texcoords: {
                let mut tc = [[0.0; 2]; MAX_UV];
                tc[0] = [0.5, 0.75];
                tc
            },
            num_texcoords: 1,
            ..Default::default()
        };

        let mut buf = Vec::new();
        packer.pack_vertex(&mut buf, &original);
        assert_eq!(buf.len(), packer.vertex_size());

        let mut pos = 0;
        let unpacked = packer.unpack_vertex(&buf, &mut pos).unwrap();

        assert_eq!(unpacked.position, original.position);
        assert_eq!(unpacked.normal, original.normal);
        assert_eq!(unpacked.texcoords[0], original.texcoords[0]);
    }

    #[test]
    fn test_pack_unpack_vertex_with_skin_roundtrip() {
        let packer = UnivertPacker {
            pack_order: "PNT0S".to_string(),
            pos_type: VertexElementType::Float3,
            normal_type: VertexElementType::Float3,
            uv_types: [VertexElementType::Float2; MAX_UV],
            indices_type: VertexElementType::UByte4,
            weights_type: VertexElementType::Float4,
            ..Default::default()
        };

        let original = UnpackedVertex {
            position: [-1.0, 5.0, 0.0],
            normal: [1.0, 0.0, 0.0],
            texcoords: {
                let mut tc = [[0.0; 2]; MAX_UV];
                tc[0] = [0.25, 0.5];
                tc
            },
            num_texcoords: 1,
            bone_indices: [3, 1, 0, 0],
            bone_weights: [0.7, 0.3, 0.0, 0.0],
            ..Default::default()
        };

        let mut buf = Vec::new();
        packer.pack_vertex(&mut buf, &original);
        assert_eq!(buf.len(), packer.vertex_size());

        let mut pos = 0;
        let unpacked = packer.unpack_vertex(&buf, &mut pos).unwrap();

        assert_eq!(unpacked.position, original.position);
        assert_eq!(unpacked.normal, original.normal);
        assert_eq!(unpacked.texcoords[0], original.texcoords[0]);
        assert_eq!(unpacked.bone_indices, original.bone_indices);
        assert_eq!(unpacked.bone_weights, original.bone_weights);
    }
}
