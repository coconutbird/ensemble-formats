//! Bounding volume recomputation for `UgxGeom`.
//!
//! Rebuilds global AABB, bounding sphere, and per-bone bounding boxes
//! from vertex positions and skin weights.

use alloc::vec;
use alloc::vec::Vec;

use crate::types::primitives::{AABB, Sphere};
use crate::{Result, UgxGeom};

impl UgxGeom {
    /// Recompute global `bounds` (AABB) and `bounding_sphere` from all vertices.
    ///
    /// # Errors
    ///
    /// Returns an error if any section's vertex-buffer range or packed vertex
    /// data is invalid.
    pub fn rebuild_bounds(&mut self) -> Result<()> {
        let mut min = [f32::MAX; 3];
        let mut max = [f32::MIN; 3];
        let mut any = false;

        for section_idx in 0..self.sections.len() {
            let vertices = self.unpack_section_vertices(section_idx)?;
            for vertex in &vertices {
                any = true;
                for axis in 0..3 {
                    min[axis] = min[axis].min(vertex.position[axis]);
                    max[axis] = max[axis].max(vertex.position[axis]);
                }
            }
        }

        if !any {
            self.bounds = AABB::default();
            self.bounding_sphere = Sphere::default();
            return Ok(());
        }

        self.bounds = AABB { min, max };

        let center = [
            f32::midpoint(min[0], max[0]),
            f32::midpoint(min[1], max[1]),
            f32::midpoint(min[2], max[2]),
        ];

        // The original engine computes the bounding sphere as the sphere
        // enclosing the AABB: center at AABB center, radius = half the diagonal.
        let dx = max[0] - min[0];
        let dy = max[1] - min[1];
        let dz = max[2] - min[2];
        let radius = (dx * dx + dy * dy + dz * dz).sqrt() * 0.5;

        self.bounding_sphere = Sphere { center, radius };
        Ok(())
    }

    /// Recompute per-bone bounding boxes from vertex positions and skin weights.
    ///
    /// For each bone, the AABB is the min/max of all vertex positions that have
    /// a non-zero weight on that bone. Section-local bone indices are remapped
    /// to global skeleton indices via the section's `bone_remap` table (when
    /// `global_bones` is false).
    ///
    /// Bones with no influenced vertices get the engine's sentinel "empty AABB"
    /// pattern: `min = [+SENTINEL; 3], max = [-SENTINEL; 3]` (min > max),
    /// matching the bit pattern `0x7cf0bdc2` (~1e37) found in original files.
    ///
    /// # Errors
    ///
    /// Returns an error if any section's vertex-buffer range or packed vertex
    /// data is invalid.
    pub fn rebuild_bone_bounds(&mut self) -> Result<()> {
        /// Engine sentinel for empty bone bounds (bit pattern `0x7cf0bdc2`).
        const SENTINEL: f32 = 1e37;

        let bone_count = self.bones.len();
        if bone_count == 0 {
            self.bone_bounds = Vec::new();
            return Ok(());
        }

        let mut bb_min = vec![[f32::MAX; 3]; bone_count];
        let mut bb_max = vec![[f32::MIN; 3]; bone_count];
        let mut has_verts = vec![false; bone_count];

        for section_idx in 0..self.sections.len() {
            let section = &self.sections[section_idx];
            let rigid_bone = usize::try_from(section.rigid_bone_index).unwrap_or(usize::MAX);
            let bone_remap = section.bone_remap.clone();
            let has_skin = section
                .vertex_packer()
                .map_or(!section.rigid_only && section.vert_size >= 28, |p| {
                    p.pack_order.contains('S')
                });
            let use_rigid = (!has_skin || section.rigid_only) && rigid_bone < bone_count;

            let vertices = self.unpack_section_vertices(section_idx)?;
            for vertex in &vertices {
                if use_rigid {
                    has_verts[rigid_bone] = true;
                    for axis in 0..3 {
                        bb_min[rigid_bone][axis] =
                            bb_min[rigid_bone][axis].min(vertex.position[axis]);
                        bb_max[rigid_bone][axis] =
                            bb_max[rigid_bone][axis].max(vertex.position[axis]);
                    }
                } else {
                    for influence_index in 0..4 {
                        if vertex.bone_weights[influence_index] > 0.0 {
                            let raw_idx = usize::from(vertex.bone_indices[influence_index]);
                            let global_idx = if !bone_remap.is_empty() {
                                if raw_idx < bone_remap.len() {
                                    usize::from(bone_remap[raw_idx])
                                } else {
                                    continue;
                                }
                            } else if raw_idx > 0 {
                                raw_idx - 1
                            } else {
                                continue;
                            };
                            if global_idx >= bone_count {
                                continue;
                            }
                            has_verts[global_idx] = true;
                            for axis in 0..3 {
                                bb_min[global_idx][axis] =
                                    bb_min[global_idx][axis].min(vertex.position[axis]);
                                bb_max[global_idx][axis] =
                                    bb_max[global_idx][axis].max(vertex.position[axis]);
                            }
                        }
                    }
                }
            }
        }

        self.bone_bounds = (0..bone_count)
            .map(|i| {
                if has_verts[i] {
                    AABB {
                        min: bb_min[i],
                        max: bb_max[i],
                    }
                } else {
                    AABB {
                        min: [SENTINEL; 3],
                        max: [-SENTINEL; 3],
                    }
                }
            })
            .collect();
        Ok(())
    }
}
