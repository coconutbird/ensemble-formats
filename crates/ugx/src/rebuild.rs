//! Rebuild derived data for `UgxGeom`.
//!
//! After a glTF round-trip, several engine-computed fields are lost (AABB tree,
//! bone bounds, accessories, metadata flags). The methods here recompute all of
//! them from the primary mesh/skeleton data so that re-exported UGX files are
//! byte-level functional equivalents of the originals.

use alloc::vec;
use alloc::vec::Vec;

use crate::UgxGeom;
use crate::types::Accessory;
use crate::types::aabb_tree::{AABB_NULL_INDEX, AabbTree, AabbTreeNode};
use crate::types::primitives::{AABB, Sphere};

impl UgxGeom {
    /// Rebuild **all** derived data from the primary mesh/skeleton data.
    ///
    /// This is the one-stop call after a glTF import. It recomputes:
    /// - Global bounding volumes (`bounds`, `bounding_sphere`)
    /// - Per-bone bounding boxes (`bone_bounds`)
    /// - Metadata flags (`rigid_only`, `all_sections_rigid`, etc.)
    /// - AABB tree (spatial acceleration structure)
    /// - Accessories (must be built AFTER the tree — accessories are indexed
    ///   by tree node index at runtime)
    pub fn rebuild_derived_data(&mut self) {
        self.rebuild_bounds();
        self.rebuild_bone_bounds();
        self.rebuild_metadata_flags();
        // Tree must be built first — the per-node section mapping it produces
        // is consumed by rebuild_accessories to satisfy the engine invariant:
        //   accessories[node_index].object_indices == sections for that node.
        let node_section_map = self.rebuild_aabb_tree();
        self.rebuild_accessories(node_section_map);
    }

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
            // Check if the vertex format includes skin data.
            // DE: check pack_order for 'S'. HW2 (no packer): infer from vert_size.
            let has_skin = section
                .base_vert_packer
                .as_ref()
                .map_or(!section.rigid_only && section.vert_size >= 28, |p| {
                    p.pack_order.contains('S')
                });
            // Use rigid path when: no skin data, OR explicitly rigid with valid bone
            let use_rigid = (!has_skin || section.rigid_only) && rigid_bone < bone_count;

            if let Ok(verts) = self.unpack_section_vertices(section_idx) {
                for v in &verts {
                    if use_rigid {
                        // All vertices belong to rigid_bone_index
                        has_verts[rigid_bone] = true;
                        for k in 0..3 {
                            bb_min[rigid_bone][k] = bb_min[rigid_bone][k].min(v.position[k]);
                            bb_max[rigid_bone][k] = bb_max[rigid_bone][k].max(v.position[k]);
                        }
                    } else {
                        // Skinned section: use all vertex bone influences
                        for j in 0..4 {
                            if v.bone_weights[j] > 0.0 {
                                let raw_idx = v.bone_indices[j] as usize;
                                // Remap to global bone index:
                                // - With bone_remap: indices are 0-based local
                                // - Without bone_remap: indices are 1-based global
                                //   (0 = no bone, actual index = raw - 1)
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
                    // Sentinel "empty" AABB — min > max, matching original engine
                    AABB {
                        min: [SENTINEL; 3],
                        max: [-SENTINEL; 3],
                    }
                }
            })
            .collect();
    }

    /// Recompute metadata flags from section data.
    ///
    /// Updates: `rigid_only`, `rigid_bone_index`, `all_sections_rigid`,
    /// `all_sections_skinned`, `global_bones`, `instance_index_multiplier`,
    /// `max_instances`, `large_geom_bone_index`.
    pub fn rebuild_metadata_flags(&mut self) {
        // A section is "effectively rigid" if:
        // - its rigid_only flag is true, OR
        // - it uses global_bones with max_bones == 1 (single-bone global binding)
        let section_is_rigid = |s: &crate::types::Section| -> bool {
            s.rigid_only || (s.global_bones && s.max_bones <= 1)
        };

        let all_rigid = self.sections.iter().all(&section_is_rigid);
        let all_skinned = self.sections.iter().all(|s| !section_is_rigid(s));
        let any_global = self.sections.iter().any(|s| s.global_bones);

        // rigid_only: true when all sections are effectively rigid AND all bound
        // to the same bone (single rigid_bone_index across all sections).
        let same_rigid_bone = all_rigid
            && !self.sections.is_empty()
            && self
                .sections
                .iter()
                .all(|s| s.rigid_bone_index == self.sections[0].rigid_bone_index);
        self.rigid_only = same_rigid_bone;
        // all_sections_rigid uses the broader "effectively rigid" test
        self.all_sections_rigid = all_rigid;
        self.global_bones = any_global;

        // all_sections_skinned: true only when every section is skinned AND
        // no section uses global_bones (matching original engine logic).
        self.all_sections_skinned = !any_global && !all_rigid && all_skinned;

        // rigid_bone_index: if a single rigid bone is used across all rigid
        // sections, use it; otherwise 0.
        if all_rigid && !self.sections.is_empty() {
            let first_rigid = self.sections[0].rigid_bone_index;
            if self
                .sections
                .iter()
                .all(|s| s.rigid_bone_index == first_rigid)
            {
                self.rigid_bone_index = first_rigid;
            } else {
                self.rigid_bone_index = 0;
            }
        } else {
            self.rigid_bone_index = 0;
        }

        // instance_index_multiplier: next power of two of max vertex count
        let max_verts = self
            .sections
            .iter()
            .map(|s| s.num_verts as u32)
            .max()
            .unwrap_or(1);
        self.instance_index_multiplier = max_verts.next_power_of_two() as i16;

        // max_instances defaults to 1 (set by artist tooling, not derivable)
        // large_geom_bone_index defaults to i16::MAX (no large geom)
        self.max_instances = 1;
        self.large_geom_bone_index = i16::MAX;
    }

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
    /// `valid_accessories` is left empty — the original files contain
    /// uninitialized data in this field.
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
        self.valid_accessories = Vec::new();
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
        self.valid_accessories = Vec::new();
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

/// Per-section AABB + centroid used for section-level BVH construction.
struct SectionAABB {
    section_idx: usize,
    min: [f32; 3],
    max: [f32; 3],
    centroid: [f32; 3],
}

impl UgxGeom {
    /// Rebuild the AABB tree as a section-level spatial hierarchy.
    ///
    /// Returns a per-node section mapping: `result[node_idx]` contains the
    /// section indices that belong to that node (empty for internal nodes,
    /// populated for leaf nodes). This mapping must be passed to
    /// `rebuild_accessories` to satisfy the engine invariant:
    ///   `accessories.len() == tree.nodes.len()`
    ///   `accessories[i].object_indices == sections drawn when node i is visible`
    ///
    /// Produces a tree matching the original engine's pattern:
    /// - Node `obj_indices` are always empty (the engine reads section mappings
    ///   from the accessories array at runtime, not from the tree nodes).
    /// - `node.index` is always 0 (matching originals).
    /// - Leaf nodes group spatially-close sections; internal nodes are pure
    ///   bounding volumes.
    pub fn rebuild_aabb_tree(&mut self) -> Vec<Vec<i32>> {
        if self.sections.is_empty() {
            self.aabb_tree = None;
            return Vec::new();
        }

        // Compute per-section AABBs
        let mut sec_aabbs = Vec::new();
        for si in 0..self.sections.len() {
            let verts = match self.unpack_section_vertices(si) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if verts.is_empty() {
                continue;
            }
            let mut smin = [f32::MAX; 3];
            let mut smax = [f32::MIN; 3];
            for v in &verts {
                for k in 0..3 {
                    smin[k] = smin[k].min(v.position[k]);
                    smax[k] = smax[k].max(v.position[k]);
                }
            }
            sec_aabbs.push(SectionAABB {
                section_idx: si,
                min: smin,
                max: smax,
                centroid: [
                    (smin[0] + smax[0]) * 0.5,
                    (smin[1] + smax[1]) * 0.5,
                    (smin[2] + smax[2]) * 0.5,
                ],
            });
        }

        if sec_aabbs.is_empty() {
            self.aabb_tree = None;
            return Vec::new();
        }

        // Build a section-level BVH. Each leaf holds a set of section indices.
        let mut nodes: Vec<AabbTreeNode> = Vec::new();
        let mut node_sections: Vec<Vec<i32>> = Vec::new();
        let indices: Vec<usize> = (0..sec_aabbs.len()).collect();
        build_section_bvh(
            &sec_aabbs,
            &indices,
            &mut nodes,
            &mut node_sections,
            AABB_NULL_INDEX,
        );

        self.aabb_tree = Some(AabbTree { nodes });
        node_sections
    }
}

/// Recursively build a section-level BVH. Returns the index of the created node.
///
/// `leaf_sections[node_idx]` will contain section indices for leaf nodes,
/// and an empty vec for internal nodes.
fn build_section_bvh(
    secs: &[SectionAABB],
    indices: &[usize],
    nodes: &mut Vec<AabbTreeNode>,
    leaf_sections: &mut Vec<Vec<i32>>,
    parent_idx: u32,
) -> u32 {
    let (min, max) = compute_section_aabb(secs, indices);
    let node_idx = nodes.len() as u32;

    // Leaf: 1-2 sections or can't split
    if indices.len() <= 2 {
        let sec_indices: Vec<i32> = indices
            .iter()
            .map(|&i| secs[i].section_idx as i32)
            .collect();
        nodes.push(AabbTreeNode {
            min,
            max,
            parent: parent_idx,
            children: [AABB_NULL_INDEX, AABB_NULL_INDEX],
            index: 0,
            obj_indices: Vec::new(), // always empty — matches originals
            split_plane: 0.0,
        });
        // Pad leaf_sections to match node index
        while leaf_sections.len() < node_idx as usize {
            leaf_sections.push(Vec::new());
        }
        leaf_sections.push(sec_indices);
        return node_idx;
    }

    // Find longest axis of centroid spread
    let mut c_min = [f32::MAX; 3];
    let mut c_max = [f32::MIN; 3];
    for &i in indices {
        for k in 0..3 {
            c_min[k] = c_min[k].min(secs[i].centroid[k]);
            c_max[k] = c_max[k].max(secs[i].centroid[k]);
        }
    }
    let extents = [
        c_max[0] - c_min[0],
        c_max[1] - c_min[1],
        c_max[2] - c_min[2],
    ];
    let split_axis = if extents[0] >= extents[1] && extents[0] >= extents[2] {
        0
    } else if extents[1] >= extents[2] {
        1
    } else {
        2
    };

    // If no spatial spread, make a single leaf with all sections
    if extents[split_axis] < 1e-7 {
        let sec_indices: Vec<i32> = indices
            .iter()
            .map(|&i| secs[i].section_idx as i32)
            .collect();
        nodes.push(AabbTreeNode {
            min,
            max,
            parent: parent_idx,
            children: [AABB_NULL_INDEX, AABB_NULL_INDEX],
            index: 0,
            obj_indices: Vec::new(),
            split_plane: 0.0,
        });
        while leaf_sections.len() < node_idx as usize {
            leaf_sections.push(Vec::new());
        }
        leaf_sections.push(sec_indices);
        return node_idx;
    }

    let split_value = (c_min[split_axis] + c_max[split_axis]) * 0.5;

    // Partition
    let mut left: Vec<usize> = Vec::new();
    let mut right: Vec<usize> = Vec::new();
    for &i in indices {
        if secs[i].centroid[split_axis] <= split_value {
            left.push(i);
        } else {
            right.push(i);
        }
    }

    // Fallback: if one side empty, split sorted in half
    if left.is_empty() || right.is_empty() {
        let mut sorted: Vec<usize> = indices.to_vec();
        sorted.sort_by(|&a, &b| {
            secs[a].centroid[split_axis]
                .partial_cmp(&secs[b].centroid[split_axis])
                .unwrap_or(core::cmp::Ordering::Equal)
        });
        let mid = sorted.len() / 2;
        left = sorted[..mid].to_vec();
        right = sorted[mid..].to_vec();
    }

    // Push internal node (children filled after recursion)
    nodes.push(AabbTreeNode {
        min,
        max,
        parent: parent_idx,
        children: [AABB_NULL_INDEX, AABB_NULL_INDEX],
        index: 0,
        obj_indices: Vec::new(),
        split_plane: 0.0,
    });
    // Internal nodes have empty section lists
    while leaf_sections.len() < node_idx as usize {
        leaf_sections.push(Vec::new());
    }

    leaf_sections.push(Vec::new());

    let left_idx = build_section_bvh(secs, &left, nodes, leaf_sections, node_idx);
    nodes[node_idx as usize].children[0] = left_idx;

    let right_idx = build_section_bvh(secs, &right, nodes, leaf_sections, node_idx);
    nodes[node_idx as usize].children[1] = right_idx;

    node_idx
}

/// Compute the AABB enclosing a set of section AABBs.
fn compute_section_aabb(secs: &[SectionAABB], indices: &[usize]) -> ([f32; 3], [f32; 3]) {
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for &idx in indices {
        for k in 0..3 {
            min[k] = min[k].min(secs[idx].min[k]);
            max[k] = max[k].max(secs[idx].max[k]);
        }
    }
    (min, max)
}
