//! Main UGX geometry container type.

use alloc::vec::Vec;

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
    /// Get unpacked vertices for a section.
    ///
    /// For HW1 sections, uses the embedded `UnivertPacker`.
    /// For HW2 sections (no packer), infers the vertex layout from `vert_size`.
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

        if let Some(ref packer) = section.base_vert_packer {
            // HW1 path — use the embedded UnivertPacker
            for _ in 0..vertex_count {
                vertices.push(packer.unpack_vertex(vb_slice, &mut vb_pos)?);
            }
        } else {
            // HW2 path — infer layout from vert_size
            let vert_size = crate::checked_usize_i32(section.vert_size, "vertex stride")?;
            let is_skinned = !section.rigid_only && vert_size >= 28;

            for _ in 0..vertex_count {
                let vert =
                    crate::vertex::unpack_hw2_vertex(vb_slice, &mut vb_pos, vert_size, is_skinned)?;
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
