//! Main UGX geometry container type.

use alloc::vec::Vec;

use crate::Hw2SkinOrder;
use crate::error::Result;
use crate::vertex::packer::UnpackedVertex;

use super::aabb_tree::AabbTree;
use super::accessory::Accessory;
use super::bone::{GrannyBone, GrannyMesh};
use super::material::Material;
use super::primitives::{AABB, Sphere};
use super::section::Section;

/// Aggregate flags describing how mesh sections use skeleton data.
#[derive(Debug, Clone, Default)]
pub struct GeometryFlags {
    /// Whether every section is rigid.
    pub all_sections_rigid: bool,
    /// Whether every section is skinned.
    pub all_sections_skinned: bool,
    /// Whether any section uses global bone indices.
    pub global_bones: bool,
}

/// Parsed UGX geometry data.
#[derive(Debug, Clone)]
pub struct UgxGeom {
    /// Bounding sphere.
    pub bounding_sphere: Sphere,
    /// Axis-aligned bounding box.
    pub bounds: AABB,
    /// Materials.
    pub materials: Vec<Material>,
    /// Bones (from cached data chunk 0x700).
    pub bones: Vec<super::bone::Bone>,
    /// Granny bone data (from granny chunk 0x703) - contains inverse world matrices.
    pub granny_bones: Vec<GrannyBone>,
    /// Granny mesh data (from granny chunk 0x703) - contains mesh names and bone bindings.
    pub granny_meshes: Vec<GrannyMesh>,
    /// Skeleton LOD type from the Granny chunk (0x703). Preserved for round-tripping.
    pub skeleton_lod_type: u32,
    /// Per-bone bounding boxes.
    pub bone_bounds: Vec<AABB>,
    /// Mesh sections.
    pub sections: Vec<Section>,
    /// Raw vertex buffer.
    pub vertex_buffer: Vec<u8>,
    /// Raw index buffer (u16 indices).
    pub index_buffer: Vec<u16>,
    /// Accessories (from `BCachedData`).
    pub accessories: Vec<Accessory>,
    /// Raw indices into [`Self::accessories`] that the game considers valid.
    ///
    /// These are stored as a flat i32 array on disk. Keeping the encoded
    /// indices avoids losing duplicates or malformed/sentinel values while
    /// reading; the game-compatible writer validates them before serialization.
    pub valid_accessories: Vec<i32>,
    /// Is the entire mesh rigid (single bone)?
    pub rigid_only: bool,
    /// Rigid bone index (if `rigid_only`).
    pub rigid_bone_index: i32,
    /// Maximum number of instances for instanced rendering.
    pub max_instances: i16,
    /// Instance index multiplier (next power of two of max vertex count).
    ///
    /// The on-disk field is treated as an unsigned 16-bit bit pattern by the
    /// engine. This API retains its historical `i16` type, so 32,768 is
    /// represented as [`i16::MIN`].
    pub instance_index_multiplier: i16,
    /// Large geometry bone index (`i16::MAX` when unused).
    pub large_geom_bone_index: i16,
    /// Aggregate skeleton and section flags.
    pub flags: GeometryFlags,
    /// Parsed AABB tree (chunk 0x705, optional).
    ///
    /// Spatial acceleration structure used by the game engine for collision
    /// and ray-casting queries. When `None`, the chunk is omitted from the
    /// written ECF container.
    pub aabb_tree: Option<AabbTree>,
}

impl UgxGeom {
    /// Infer the byte order of color and skin attributes for an HW2 section.
    ///
    /// Returns `None` for HW1, rigid, and colorless sections. HW2 stores the
    /// authoritative declaration in the external UFX shader, so this method
    /// compares both observed retail layouts using quantized skin-weight sums,
    /// influence counts, and valid joint ranges.
    ///
    /// # Errors
    ///
    /// Returns an error if the section index, stride, count, or vertex-buffer
    /// range is invalid.
    pub fn infer_hw2_skin_order(&self, section_idx: usize) -> Result<Option<Hw2SkinOrder>> {
        let section = self.sections.get(section_idx).ok_or_else(|| {
            crate::Error::UnsupportedFormat("section index is out of bounds".into())
        })?;
        if section.base_vert_packer.is_some() || section.rigid_only {
            return Ok(None);
        }
        if let Some(packer) = &section.external_vert_packer {
            return Ok(
                match (packer.pack_order.find('S'), packer.pack_order.find('D')) {
                    (Some(skin), Some(color)) if color < skin => Some(Hw2SkinOrder::ColorThenSkin),
                    (Some(_), Some(_)) => Some(Hw2SkinOrder::SkinThenColor),
                    _ => None,
                },
            );
        }
        let stride = crate::checked_usize_i32(section.vert_size, "vertex stride")?;
        if stride < 32 {
            return Ok(None);
        }
        let start = crate::checked_usize_i32(section.vb_offset, "vertex-buffer offset")?;
        let byte_count = crate::checked_usize_i32(section.vb_bytes, "vertex-buffer size")?;
        let end = start
            .checked_add(byte_count)
            .ok_or(crate::Error::SizeOverflow("vertex-buffer range"))?;
        let data =
            self.vertex_buffer
                .get(start..end)
                .ok_or_else(|| crate::Error::UnexpectedEof {
                    context: "section vertex buffer".into(),
                })?;
        let vertex_count = crate::checked_usize_i32(section.num_verts, "vertex count")?;
        let influence_limit = usize::try_from(section.max_bones).ok();
        let joint_limit = if section.bone_remap.is_empty() {
            self.bones.len().max(self.granny_bones.len())
        } else {
            section.bone_remap.len()
        };
        Ok(Some(infer_skin_order(
            data,
            stride,
            vertex_count,
            influence_limit,
            joint_limit,
        )))
    }

    /// Get unpacked vertices for a section.
    ///
    /// For HW1 sections, uses the embedded `UnivertPacker`. For HW2 sections,
    /// it uses an externally supplied packer when available and otherwise
    /// infers the retail vertex layout from `vert_size`.
    ///
    /// # Errors
    ///
    /// Returns an error if the section index or its vertex-buffer range is
    /// invalid, or if its packed vertex data cannot be decoded.
    pub fn unpack_section_vertices(&self, section_idx: usize) -> Result<Vec<UnpackedVertex>> {
        let section = self.sections.get(section_idx).ok_or_else(|| {
            crate::Error::UnsupportedFormat("section index is out of bounds".into())
        })?;
        let vb_start = crate::checked_usize_i32(section.vb_offset, "vertex-buffer offset")?;
        let vb_bytes = crate::checked_usize_i32(section.vb_bytes, "vertex-buffer size")?;
        let vb_end = vb_start
            .checked_add(vb_bytes)
            .ok_or(crate::Error::SizeOverflow("vertex-buffer range"))?;
        let vb_slice = self.vertex_buffer.get(vb_start..vb_end).ok_or_else(|| {
            crate::Error::UnexpectedEof {
                context: "section vertex buffer".into(),
            }
        })?;

        let mut vb_pos = 0usize;
        let vertex_count = crate::checked_usize_i32(section.num_verts, "vertex count")?;
        let mut vertices = Vec::with_capacity(vertex_count);

        if let Some(packer) = section.vertex_packer() {
            // Exact HW1-embedded or externally supplied HW2 declaration.
            for _ in 0..vertex_count {
                vertices.push(packer.unpack_vertex(vb_slice, &mut vb_pos)?);
            }
        } else {
            // HW2 path — infer layout from vert_size
            let vert_size = crate::checked_usize_i32(section.vert_size, "vertex stride")?;
            let is_skinned = !section.rigid_only && vert_size >= 28;
            let skin_order = if is_skinned && vert_size >= 32 {
                let influence_limit = usize::try_from(section.max_bones).ok();
                let joint_limit = if section.bone_remap.is_empty() {
                    self.bones.len().max(self.granny_bones.len())
                } else {
                    section.bone_remap.len()
                };
                infer_skin_order(
                    vb_slice,
                    vert_size,
                    vertex_count,
                    influence_limit,
                    joint_limit,
                )
            } else {
                Hw2SkinOrder::SkinThenColor
            };

            for _ in 0..vertex_count {
                let vert = crate::vertex::unpack_hw2_vertex(
                    vb_slice,
                    &mut vb_pos,
                    vert_size,
                    is_skinned,
                    skin_order,
                )?;
                vertices.push(vert);
            }
        }

        Ok(vertices)
    }

    /// Get indices for a section.
    ///
    /// # Errors
    ///
    /// Returns an error if the section index or its index-buffer range is
    /// invalid or cannot be represented on the target platform.
    pub fn get_section_indices(&self, section_idx: usize) -> Result<Vec<u16>> {
        let section = self.sections.get(section_idx).ok_or_else(|| {
            crate::Error::UnsupportedFormat("section index is out of bounds".into())
        })?;
        let start = crate::checked_usize_i32(section.ib_offset, "index-buffer offset")?;
        let triangle_count = crate::checked_usize_i32(section.num_tris, "triangle count")?;
        let count = triangle_count
            .checked_mul(3)
            .ok_or(crate::Error::SizeOverflow("section index count"))?;
        let end = start
            .checked_add(count)
            .ok_or(crate::Error::SizeOverflow("index-buffer range"))?;
        self.index_buffer
            .get(start..end)
            .map(<[u16]>::to_vec)
            .ok_or_else(|| crate::Error::UnexpectedEof {
                context: "section index buffer".into(),
            })
    }

    /// Get total vertex count across all sections.
    #[must_use]
    pub fn total_vertices(&self) -> usize {
        self.sections
            .iter()
            .filter_map(|section| usize::try_from(section.num_verts).ok())
            .sum()
    }

    /// Get total triangle count across all sections.
    #[must_use]
    pub fn total_triangles(&self) -> usize {
        self.sections
            .iter()
            .filter_map(|section| usize::try_from(section.num_tris).ok())
            .sum()
    }
}

fn infer_skin_order(
    data: &[u8],
    stride: usize,
    vertex_count: usize,
    influence_limit: Option<usize>,
    joint_limit: usize,
) -> Hw2SkinOrder {
    let skin_first =
        skin_candidate_penalty(data, stride, vertex_count, 20, influence_limit, joint_limit);
    let color_first =
        skin_candidate_penalty(data, stride, vertex_count, 24, influence_limit, joint_limit);
    if color_first < skin_first {
        Hw2SkinOrder::ColorThenSkin
    } else {
        Hw2SkinOrder::SkinThenColor
    }
}

fn skin_candidate_penalty(
    data: &[u8],
    stride: usize,
    vertex_count: usize,
    skin_offset: usize,
    influence_limit: Option<usize>,
    joint_limit: usize,
) -> u64 {
    const SAMPLE_LIMIT: usize = 128;
    let sample_count = vertex_count.min(SAMPLE_LIMIT);
    let mut penalty = 0u64;
    for sample in 0..sample_count {
        let vertex_index = if sample_count <= 1 {
            0
        } else {
            sample * (vertex_count - 1) / (sample_count - 1)
        };
        let Some(start) = vertex_index
            .checked_mul(stride)
            .and_then(|offset| offset.checked_add(skin_offset))
        else {
            return u64::MAX;
        };
        let Some(skin) = data.get(start..start + 8) else {
            return u64::MAX;
        };
        let indices = &skin[..4];
        let weights = &skin[4..];
        let weight_sum = weights.iter().map(|&weight| u16::from(weight)).sum::<u16>();
        penalty = penalty.saturating_add(u64::from(weight_sum.abs_diff(255)) * 4);
        if weight_sum == 0 {
            penalty = penalty.saturating_add(1_024);
        }
        let influences = weights.iter().filter(|&&weight| weight != 0).count();
        if influence_limit.is_some_and(|limit| influences > limit) {
            penalty = penalty.saturating_add(1_024);
        }
        if joint_limit != 0 {
            for (&index, &weight) in indices.iter().zip(weights) {
                if weight != 0 && usize::from(index) >= joint_limit {
                    penalty = penalty.saturating_add(4_096);
                }
            }
        }
    }
    penalty
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn infers_both_hw2_skin_orders() {
        let skin = [1, 2, 3, 4, 255, 0, 0, 0];
        let color = [20, 40, 80, 255];

        let mut skin_first = vec![0; 20];
        skin_first.extend_from_slice(&skin);
        skin_first.extend_from_slice(&color);
        assert_eq!(
            infer_skin_order(&skin_first, 32, 1, Some(1), 8),
            Hw2SkinOrder::SkinThenColor
        );

        let mut color_first = vec![0; 20];
        color_first.extend_from_slice(&color);
        color_first.extend_from_slice(&skin);
        assert_eq!(
            infer_skin_order(&color_first, 32, 1, Some(1), 8),
            Hw2SkinOrder::ColorThenSkin
        );
    }
}
