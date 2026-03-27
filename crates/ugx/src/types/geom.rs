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
    /// Accessories (from BCachedData).
    pub accessories: Vec<Accessory>,
    /// Valid accessories subset (from BCachedData).
    pub valid_accessories: Vec<Accessory>,
    /// Is the entire mesh rigid (single bone)?
    pub rigid_only: bool,
    /// Rigid bone index (if rigid_only).
    pub rigid_bone_index: i32,
    /// Maximum number of instances for instanced rendering.
    pub max_instances: i16,
    /// Instance index multiplier (next power of two of max vertex count).
    pub instance_index_multiplier: i16,
    /// Large geometry bone index (i16::MAX when unused).
    pub large_geom_bone_index: i16,
    /// Are all sections rigid (multi-bone rigid)?
    pub all_sections_rigid: bool,
    /// Are all sections skinned?
    pub all_sections_skinned: bool,
    /// Use global bones?
    pub global_bones: bool,
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
    /// For DE sections, uses the embedded `UnivertPacker`.
    /// For HW2 sections (no packer), infers the vertex layout from `vert_size`.
    pub fn unpack_section_vertices(&self, section_idx: usize) -> Result<Vec<UnpackedVertex>> {
        let section = &self.sections[section_idx];
        let vb_start = section.vb_offset as usize;
        let vb_end = vb_start + section.vb_bytes as usize;
        let vb_slice = &self.vertex_buffer[vb_start..vb_end];

        let mut vb_pos = 0usize;
        let mut vertices = Vec::with_capacity(section.num_verts as usize);

        if let Some(ref packer) = section.base_vert_packer {
            // DE path — use the embedded UnivertPacker
            for _ in 0..section.num_verts {
                vertices.push(packer.unpack_vertex(vb_slice, &mut vb_pos)?);
            }
        } else {
            // HW2 path — infer layout from vert_size
            let vert_size = section.vert_size as usize;
            let is_skinned = !section.rigid_only && vert_size >= 28;

            for _ in 0..section.num_verts {
                let vert =
                    crate::vertex::unpack_hw2_vertex(vb_slice, &mut vb_pos, vert_size, is_skinned)?;
                vertices.push(vert);
            }
        }

        Ok(vertices)
    }

    /// Get indices for a section.
    pub fn get_section_indices(&self, section_idx: usize) -> Vec<u16> {
        let section = &self.sections[section_idx];
        let start = section.ib_offset as usize;
        let count = section.num_tris as usize * 3;
        self.index_buffer[start..start + count].to_vec()
    }

    /// Get total vertex count across all sections.
    pub fn total_vertices(&self) -> usize {
        self.sections.iter().map(|s| s.num_verts as usize).sum()
    }

    /// Get total triangle count across all sections.
    pub fn total_triangles(&self) -> usize {
        self.sections.iter().map(|s| s.num_tris as usize).sum()
    }
}
