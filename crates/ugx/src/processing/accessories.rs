//! Accessory recomputation for `UgxGeom`.
//!
//! Rebuilds accessories from AABB tree node-section mappings or via
//! legacy group-by-`accessory_index` fallback.

use alloc::vec;
use alloc::vec::Vec;

use crate::UgxGeom;
use crate::types::Accessory;

impl UgxGeom {
    /// Rebuild accessories from the AABB tree's per-node section mapping.
    ///
    /// `node_section_map[i]` contains the section indices that belong to
    /// tree node `i`. The engine indexes into the accessories array by
    /// tree node index at runtime (`accessories[node_index]`), so we must
    /// produce exactly `tree.nodes.len()` entries.
    ///
    /// For each node:
    /// - Internal nodes (empty section list): `object_indices = []`
    /// - Leaf nodes: `object_indices = [section indices]`
    /// - `first_bone` / `num_bones`: computed from the bones referenced by
    ///   the node's sections (union of all bone influences).
    ///
    /// If no tree was built (e.g. HW2, or no sections), falls back to
    /// the legacy group-by-`accessory_index` strategy.
    ///
    /// `valid_accessories` is the subset of accessories that have non-empty
    /// `object_indices` (leaf nodes with actual section geometry). The engine
    /// reads this as a flat i32 index array via `BPackedArray_Simple__unpack`.
    pub fn rebuild_accessories(&mut self, node_section_map: Vec<Vec<i32>>) {
        let bone_count = self.bones.len();

        // If no tree was built, fall back to legacy accessory grouping.
        if node_section_map.is_empty() {
            self.rebuild_accessories_legacy();
            return;
        }

        // Build one accessory per tree node.
        self.accessories = node_section_map
            .iter()
            .map(|sec_indices| {
                let (first_bone, num_bones) =
                    self.compute_bone_range_for_sections(sec_indices, bone_count);
                Accessory {
                    first_bone,
                    num_bones,
                    object_indices: sec_indices.clone(),
                }
            })
            .collect();

        // valid_accessories = accessories with non-empty object_indices (leaf nodes).
        self.valid_accessories = self
            .accessories
            .iter()
            .filter(|a| !a.object_indices.is_empty())
            .cloned()
            .collect();
    }

    /// Legacy accessory rebuild: group sections by `accessory_index`.
    ///
    /// Used when no AABB tree is present (e.g. HW2 files).
    fn rebuild_accessories_legacy(&mut self) {
        let bone_count = self.bones.len();
        if bone_count == 0 || self.sections.is_empty() {
            self.accessories = Vec::new();
            self.valid_accessories = Vec::new();
            return;
        }

        let num_groups = self
            .sections
            .iter()
            .map(|s| s.accessory_index)
            .max()
            .unwrap_or(0) as usize
            + 1;

        let mut group_sections: Vec<Vec<i32>> = vec![Vec::new(); num_groups];
        for (si, section) in self.sections.iter().enumerate() {
            let gi = section.accessory_index as usize;
            if gi < num_groups {
                group_sections[gi].push(si as i32);
            }
        }

        self.accessories = (0..num_groups)
            .map(|gi| {
                let (first_bone, num_bones) =
                    self.compute_bone_range_for_sections(&group_sections[gi], bone_count);
                Accessory {
                    first_bone,
                    num_bones,
                    object_indices: group_sections[gi].clone(),
                }
            })
            .collect();
        self.valid_accessories = self
            .accessories
            .iter()
            .filter(|a| !a.object_indices.is_empty())
            .cloned()
            .collect();
    }

    /// Compute the bone range (first_bone, num_bones) for a set of sections.
    ///
    /// Scans vertex bone influences across all given sections and returns
    /// the contiguous range [first_bone, first_bone + num_bones) that
    /// covers all referenced bones.
    fn compute_bone_range_for_sections(
        &self,
        sec_indices: &[i32],
        bone_count: usize,
    ) -> (i32, i32) {
        if bone_count == 0 || sec_indices.is_empty() {
            return (0, bone_count as i32);
        }

        let mut bmin = usize::MAX;
        let mut bmax = 0usize;

        for &si in sec_indices {
            let si = si as usize;
            if si >= self.sections.len() {
                continue;
            }
            let section = &self.sections[si];
            let rigid_bone = section.rigid_bone_index as usize;
            let has_skin = section
                .base_vert_packer
                .as_ref()
                .map_or(!section.rigid_only && section.vert_size >= 28, |p| {
                    p.pack_order.contains('S')
                });
            let use_rigid = (!has_skin || section.rigid_only) && rigid_bone < bone_count;

            if use_rigid {
                bmin = bmin.min(rigid_bone);
                bmax = bmax.max(rigid_bone);
            } else if let Ok(verts) = self.unpack_section_vertices(si) {
                let bone_remap = &section.bone_remap;
                for v in &verts {
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
                            if global_idx < bone_count {
                                bmin = bmin.min(global_idx);
                                bmax = bmax.max(global_idx);
                            }
                        }
                    }
                }
            }
        }

        if bmin <= bmax {
            (bmin as i32, (bmax - bmin + 1) as i32)
        } else {
            (0, bone_count as i32)
        }
    }
}
