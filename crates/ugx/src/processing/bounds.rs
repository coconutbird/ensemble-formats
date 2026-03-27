//! Bounding volume recomputation for `UgxGeom`.
//!
//! Rebuilds global AABB, bounding sphere, and per-bone bounding boxes
//! from vertex positions and skin weights.

use alloc::vec;
use alloc::vec::Vec;

use crate::UgxGeom;
use crate::types::primitives::{AABB, Sphere};

impl UgxGeom {
    /// Recompute global `bounds` (AABB) and `bounding_sphere` from all vertices.
    pub fn rebuild_bounds(&mut self) {
        let mut min = [f32::MAX; 3];
        let mut max = [f32::MIN; 3];
        let mut any = false;

        for section_idx in 0..self.sections.len() {
            if let Ok(verts) = self.unpack_section_vertices(section_idx) {
                for v in &verts {
                    any = true;
                    for i in 0..3 {
                        min[i] = min[i].min(v.position[i]);
                        max[i] = max[i].max(v.position[i]);
                    }
                }
            }
        }

        if !any {
            self.bounds = AABB::default();
            self.bounding_sphere = Sphere::default();
            return;
        }

        self.bounds = AABB { min, max };

        let center = [
            (min[0] + max[0]) * 0.5,
            (min[1] + max[1]) * 0.5,
            (min[2] + max[2]) * 0.5,
        ];

        // The original engine computes the bounding sphere as the sphere
        // enclosing the AABB: center at AABB center, radius = half the diagonal.
        let dx = max[0] - min[0];
        let dy = max[1] - min[1];
        let dz = max[2] - min[2];
        let radius = (dx * dx + dy * dy + dz * dz).sqrt() * 0.5;

        self.bounding_sphere = Sphere { center, radius };
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
    pub fn rebuild_bone_bounds(&mut self) {
        /// Engine sentinel for empty bone bounds (bit pattern `0x7cf0bdc2`).
        const SENTINEL: f32 = 1e37;

        let bone_count = self.bones.len();
        if bone_count == 0 {
            self.bone_bounds = Vec::new();
            return;
        }

        let mut bb_min = vec![[f32::MAX; 3]; bone_count];
        let mut bb_max = vec![[f32::MIN; 3]; bone_count];
        let mut has_verts = vec![false; bone_count];

        for section_idx in 0..self.sections.len() {
            let section = &self.sections[section_idx];
            let rigid_bone = section.rigid_bone_index as usize;
            let bone_remap = section.bone_remap.clone();
            let has_skin = section
                .base_vert_packer
                .as_ref()
                .map_or(!section.rigid_only && section.vert_size >= 28, |p| {
                    p.pack_order.contains('S')
                });
            let use_rigid = (!has_skin || section.rigid_only) && rigid_bone < bone_count;

            if let Ok(verts) = self.unpack_section_vertices(section_idx) {
                for v in &verts {
                    if use_rigid {
                        has_verts[rigid_bone] = true;
                        for k in 0..3 {
                            bb_min[rigid_bone][k] = bb_min[rigid_bone][k].min(v.position[k]);
                            bb_max[rigid_bone][k] = bb_max[rigid_bone][k].max(v.position[k]);
                        }
                    } else {
                        for j in 0..4 {
                            if v.bone_weights[j] > 0.0 {
                                let raw_idx = v.bone_indices[j] as usize;
                                let global_idx = if !bone_remap.is_empty() {
                                    if raw_idx < bone_remap.len() {
                                        bone_remap[raw_idx] as usize
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
                                for k in 0..3 {
                                    bb_min[global_idx][k] =
                                        bb_min[global_idx][k].min(v.position[k]);
                                    bb_max[global_idx][k] =
                                        bb_max[global_idx][k].max(v.position[k]);
                                }
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
    }
}
