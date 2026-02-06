//! UnivertPacker - vertex format descriptor.
//!
//! The UnivertPacker describes how vertex attributes are packed in the vertex buffer.
//! It uses a string-based "pack order" to specify which attributes are present and
//! in what order, along with type specifiers for each attribute.

use byteorder::{LittleEndian, ReadBytesExt};
use std::io::Read;

use crate::error::Result;
use crate::vertex_element::VertexElementType;

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
    /// Texture coordinates (up to 4 sets).
    pub texcoords: [[f32; 2]; 4],
    /// Number of texcoord sets.
    pub num_texcoords: usize,
    /// Bone indices [0-3].
    pub bone_indices: [u8; 4],
    /// Bone weights [0-3].
    pub bone_weights: [f32; 4],
    /// Diffuse color [r, g, b, a].
    pub diffuse: [f32; 4],
    /// Vertex index.
    pub index: i16,
}

/// UnivertPacker - describes vertex format and unpacks vertices.
#[derive(Debug, Clone)]
pub struct UnivertPacker {
    /// Position element type.
    pub pos_type: VertexElementType,
    /// Basis (tangent/binormal) element type.
    pub basis_type: VertexElementType,
    /// Basis scale element type.
    pub basis_scale_type: VertexElementType,
    /// Normal element type.
    pub normal_type: VertexElementType,
    /// UV element type.
    pub uv_type: VertexElementType,
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
            normal_type: VertexElementType::Float3,
            uv_type: VertexElementType::Float2,
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
    /// Read a UnivertPacker from a stream.
    pub fn read<R: Read>(reader: &mut R) -> Result<Self> {
        let pos_type = VertexElementType::try_from(reader.read_u8()?)?;
        let basis_type = VertexElementType::try_from(reader.read_u8()?)?;
        let basis_scale_type = VertexElementType::try_from(reader.read_u8()?)?;
        let normal_type = VertexElementType::try_from(reader.read_u8()?)?;
        let uv_type = VertexElementType::try_from(reader.read_u8()?)?;
        let indices_type = VertexElementType::try_from(reader.read_u8()?)?;
        let weights_type = VertexElementType::try_from(reader.read_u8()?)?;
        let diffuse_type = VertexElementType::try_from(reader.read_u8()?)?;
        let index_type = VertexElementType::try_from(reader.read_u8()?)?;

        let pack_order = read_big_string(reader)?;
        let decl_order = read_big_string(reader)?;

        Ok(Self {
            pos_type,
            basis_type,
            basis_scale_type,
            normal_type,
            uv_type,
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
                'X' => {
                    chars.next();
                    size += self.basis_scale_type.size();
                }
                'N' => size += self.normal_type.size(),
                'T' => {
                    chars.next();
                    size += self.uv_type.size();
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
    pub fn unpack_vertex<R: Read>(&self, reader: &mut R) -> Result<UnpackedVertex> {
        let mut vertex = UnpackedVertex::default();
        let mut chars = self.pack_order.chars().peekable();

        while let Some(c) = chars.next() {
            match c.to_ascii_uppercase() {
                'P' => {
                    let v = self.pos_type.unpack(reader)?;
                    vertex.position = [v[0], v[1], v[2]];
                }
                'B' => {
                    // Get basis set index
                    let _idx = chars.next().and_then(|c| c.to_digit(10)).unwrap_or(0);
                    // Read tangent and binormal
                    let tangent = self.basis_type.unpack(reader)?;
                    let binormal = self.basis_type.unpack(reader)?;
                    vertex.tangent = tangent;
                    vertex.binormal = binormal;
                }
                'X' => {
                    // Basis scale
                    let _idx = chars.next().and_then(|c| c.to_digit(10)).unwrap_or(0);
                    let scale = self.basis_scale_type.unpack(reader)?;
                    // Apply scales to tangent/binormal w components
                    vertex.tangent[3] = scale[0];
                    vertex.binormal[3] = scale[1];
                }
                'N' => {
                    let v = self.normal_type.unpack(reader)?;
                    vertex.normal = [v[0], v[1], v[2]];
                }
                'T' => {
                    let idx = chars.next().and_then(|c| c.to_digit(10)).unwrap_or(0) as usize;
                    let v = self.uv_type.unpack(reader)?;
                    if idx < 4 {
                        vertex.texcoords[idx] = [v[0], v[1]];
                        if idx >= vertex.num_texcoords {
                            vertex.num_texcoords = idx + 1;
                        }
                    }
                }
                'S' => {
                    // Bone indices
                    let indices = self.indices_type.unpack(reader)?;
                    vertex.bone_indices = [
                        indices[0] as u8,
                        indices[1] as u8,
                        indices[2] as u8,
                        indices[3] as u8,
                    ];
                    // Bone weights
                    let weights = self.weights_type.unpack(reader)?;
                    vertex.bone_weights = weights;
                }
                'D' => {
                    vertex.diffuse = self.diffuse_type.unpack(reader)?;
                }
                'I' => {
                    let v = self.index_type.unpack(reader)?;
                    vertex.index = v[0] as i16;
                }
                _ => {}
            }
        }

        Ok(vertex)
    }

    /// Check if this packer is empty (no pack order).
    pub fn is_empty(&self) -> bool {
        self.pack_order.is_empty()
    }
}

/// Read a "BigString" - length-prefixed string used in UGX.
fn read_big_string<R: Read>(reader: &mut R) -> Result<String> {
    let len = reader.read_u32::<LittleEndian>()? as usize;
    if len == 0 {
        return Ok(String::new());
    }

    let mut bytes = vec![0u8; len];
    reader.read_exact(&mut bytes)?;

    // Remove null terminator if present
    if bytes.last() == Some(&0) {
        bytes.pop();
    }

    Ok(String::from_utf8(bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vertex_size_calculation() {
        let mut packer = UnivertPacker::default();
        packer.pack_order = "PNT0".to_string();
        packer.pos_type = VertexElementType::Float3;
        packer.normal_type = VertexElementType::Float3;
        packer.uv_type = VertexElementType::Float2;

        // Position (12) + Normal (12) + UV (8) = 32
        assert_eq!(packer.vertex_size(), 32);
    }

    #[test]
    fn test_vertex_size_with_skin() {
        let mut packer = UnivertPacker::default();
        packer.pack_order = "PNT0S".to_string();
        packer.pos_type = VertexElementType::Float3;
        packer.normal_type = VertexElementType::Float3;
        packer.uv_type = VertexElementType::Float2;
        packer.indices_type = VertexElementType::UByte4;
        packer.weights_type = VertexElementType::UByte4N;

        // Position (12) + Normal (12) + UV (8) + Indices (4) + Weights (4) = 40
        assert_eq!(packer.vertex_size(), 40);
    }
}
