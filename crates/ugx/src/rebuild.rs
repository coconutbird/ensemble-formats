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
    /// - Accessories and valid accessories
    /// - AABB tree (spatial acceleration structure)
    pub fn rebuild_derived_data(&mut self) {
        self.rebuild_bounds();
        self.rebuild_bone_bounds();
        self.rebuild_metadata_flags();
        self.rebuild_accessories();
        self.rebuild_aabb_tree();
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

    /// Rebuild accessories from section `accessory_index` fields.
    ///
    /// Each section stores an `accessory_index` that identifies which
    /// accessory group it belongs to. We group sections by this index,
    /// then compute the bone range (`first_bone`, `num_bones`) for each
    /// group from the bones referenced by that group's sections.
    ///
    /// For rigid sections (no skin data or `rigid_only`), the bone is
    /// `rigid_bone_index`. For skinned sections, we scan vertex bone
    /// indices using the same 1-based / remap logic as `rebuild_bone_bounds`.
    ///
    /// `valid_accessories` is left empty — the original files contain
    /// uninitialized data in this field.
    pub fn rebuild_accessories(&mut self) {
        let bone_count = self.bones.len();
        if bone_count == 0 || self.sections.is_empty() {
            self.accessories = Vec::new();
            self.valid_accessories = Vec::new();
            return;
        }

        // Find number of accessory groups
        let num_groups = self
            .sections
            .iter()
            .map(|s| s.accessory_index)
            .max()
            .unwrap_or(0) as usize
            + 1;

        // Collect section indices per group, and track min/max bone per group
        let mut group_sections: Vec<Vec<i32>> = vec![Vec::new(); num_groups];
        let mut group_bone_min: Vec<usize> = vec![usize::MAX; num_groups];
        let mut group_bone_max: Vec<usize> = vec![0; num_groups];

        for (si, section) in self.sections.iter().enumerate() {
            let gi = section.accessory_index as usize;
            if gi >= num_groups {
                continue;
            }
            group_sections[gi].push(si as i32);

            let rigid_bone = section.rigid_bone_index as usize;
            let has_skin = section
                .base_vert_packer
                .as_ref()
                .map_or(!section.rigid_only && section.vert_size >= 28, |p| {
                    p.pack_order.contains('S')
                });
            let use_rigid = (!has_skin || section.rigid_only) && rigid_bone < bone_count;

            if use_rigid {
                group_bone_min[gi] = group_bone_min[gi].min(rigid_bone);
                group_bone_max[gi] = group_bone_max[gi].max(rigid_bone);
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
                                group_bone_min[gi] = group_bone_min[gi].min(global_idx);
                                group_bone_max[gi] = group_bone_max[gi].max(global_idx);
                            }
                        }
                    }
                }
            }
        }

        // Single-accessory models always use the full skeleton range.
        // Multi-accessory models use the vertex-scanned bone range per group.
        if num_groups == 1 {
            self.accessories = vec![Accessory {
                first_bone: 0,
                num_bones: bone_count as i32,
                object_indices: group_sections[0].clone(),
            }];
        } else {
            self.accessories = (0..num_groups)
                .map(|gi| {
                    let first = if group_bone_min[gi] <= group_bone_max[gi] {
                        group_bone_min[gi] as i32
                    } else {
                        0
                    };
                    let num = if group_bone_min[gi] <= group_bone_max[gi] {
                        (group_bone_max[gi] - group_bone_min[gi] + 1) as i32
                    } else {
                        bone_count as i32
                    };
                    Accessory {
                        first_bone: first,
                        num_bones: num,
                        object_indices: group_sections[gi].clone(),
                    }
                })
                .collect();
        }
        self.valid_accessories = Vec::new();
    }
}

/// A triangle with precomputed centroid for BVH construction.
struct TriInfo {
    /// Global triangle index (into the full index buffer).
    global_tri_idx: i32,
    /// Vertex positions [v0, v1, v2].
    positions: [[f32; 3]; 3],
    /// Centroid of the triangle.
    centroid: [f32; 3],
}

impl UgxGeom {
    /// Rebuild the AABB tree from triangle data.
    ///
    /// Builds a top-down BVH (bounding volume hierarchy) by recursively
    /// splitting triangles along the longest axis of their centroid bounds.
    /// Leaf nodes contain up to `MAX_LEAF_TRIS` triangle indices.
    pub fn rebuild_aabb_tree(&mut self) {
        const MAX_LEAF_TRIS: usize = 8;

        // Collect all triangles with their positions
        let mut tris = Vec::new();
        self.collect_all_triangles(&mut tris);

        if tris.is_empty() {
            self.aabb_tree = None;
            return;
        }

        // Build the tree recursively
        let mut nodes = Vec::new();
        build_bvh_node(
            &tris,
            &mut (0..tris.len()).collect::<Vec<_>>(),
            &mut nodes,
            AABB_NULL_INDEX,
            MAX_LEAF_TRIS,
        );

        // Assign sequential indices to each node
        for (i, node) in nodes.iter_mut().enumerate() {
            node.index = i as u32;
        }

        self.aabb_tree = Some(AabbTree { nodes });
    }

    /// Collect all triangle positions across all sections.
    fn collect_all_triangles(&self, out: &mut Vec<TriInfo>) {
        let mut global_tri_offset = 0i32;

        for section_idx in 0..self.sections.len() {
            let section = &self.sections[section_idx];
            let verts = match self.unpack_section_vertices(section_idx) {
                Ok(v) => v,
                Err(_) => {
                    global_tri_offset += section.num_tris;
                    continue;
                }
            };
            let indices = self.get_section_indices(section_idx);

            for tri in 0..section.num_tris as usize {
                let i0 = indices[tri * 3] as usize;
                let i1 = indices[tri * 3 + 1] as usize;
                let i2 = indices[tri * 3 + 2] as usize;

                if i0 >= verts.len() || i1 >= verts.len() || i2 >= verts.len() {
                    global_tri_offset += 1;
                    continue;
                }

                let p0 = verts[i0].position;
                let p1 = verts[i1].position;
                let p2 = verts[i2].position;
                let centroid = [
                    (p0[0] + p1[0] + p2[0]) / 3.0,
                    (p0[1] + p1[1] + p2[1]) / 3.0,
                    (p0[2] + p1[2] + p2[2]) / 3.0,
                ];

                out.push(TriInfo {
                    global_tri_idx: global_tri_offset,
                    positions: [p0, p1, p2],
                    centroid,
                });
                global_tri_offset += 1;
            }
        }
    }
}

/// Compute the AABB enclosing a set of triangles.
fn compute_tri_aabb(tris: &[TriInfo], indices: &[usize]) -> ([f32; 3], [f32; 3]) {
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for &idx in indices {
        for p in &tris[idx].positions {
            for k in 0..3 {
                min[k] = min[k].min(p[k]);
                max[k] = max[k].max(p[k]);
            }
        }
    }
    (min, max)
}

/// Recursively build a BVH node. Returns the index of the created node.
fn build_bvh_node(
    tris: &[TriInfo],
    indices: &mut [usize],
    nodes: &mut Vec<AabbTreeNode>,
    parent_idx: u32,
    max_leaf: usize,
) -> u32 {
    let (min, max) = compute_tri_aabb(tris, indices);

    // This node's index
    let node_idx = nodes.len() as u32;

    // Determine the longest axis of the centroid spread
    let mut c_min = [f32::MAX; 3];
    let mut c_max = [f32::MIN; 3];
    for &idx in indices.iter() {
        for k in 0..3 {
            c_min[k] = c_min[k].min(tris[idx].centroid[k]);
            c_max[k] = c_max[k].max(tris[idx].centroid[k]);
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
    let split_value = (c_min[split_axis] + c_max[split_axis]) * 0.5;

    // Leaf node condition: few enough triangles or can't split further
    if indices.len() <= max_leaf || extents[split_axis] < 1e-7 {
        let obj_indices: Vec<i32> = indices.iter().map(|&i| tris[i].global_tri_idx).collect();
        nodes.push(AabbTreeNode {
            min,
            max,
            parent: parent_idx,
            children: [AABB_NULL_INDEX, AABB_NULL_INDEX],
            index: 0, // will be set later
            obj_indices,
            split_plane: split_value,
        });
        return node_idx;
    }

    // Partition indices into left/right by centroid position vs split plane
    let mut left = Vec::new();
    let mut right = Vec::new();
    for &idx in indices.iter() {
        if tris[idx].centroid[split_axis] <= split_value {
            left.push(idx);
        } else {
            right.push(idx);
        }
    }

    // Fallback: if one side is empty, split in half
    if left.is_empty() || right.is_empty() {
        indices.sort_by(|&a, &b| {
            tris[a].centroid[split_axis]
                .partial_cmp(&tris[b].centroid[split_axis])
                .unwrap_or(core::cmp::Ordering::Equal)
        });
        let mid = indices.len() / 2;
        left = indices[..mid].to_vec();
        right = indices[mid..].to_vec();
    }

    // Push placeholder node (children will be filled after recursion)
    nodes.push(AabbTreeNode {
        min,
        max,
        parent: parent_idx,
        children: [AABB_NULL_INDEX, AABB_NULL_INDEX],
        index: 0,
        obj_indices: Vec::new(),
        split_plane: split_value,
    });

    // Recurse left
    let left_idx = build_bvh_node(tris, &mut left, nodes, node_idx, max_leaf);
    nodes[node_idx as usize].children[0] = left_idx;

    // Recurse right
    let right_idx = build_bvh_node(tris, &mut right, nodes, node_idx, max_leaf);
    nodes[node_idx as usize].children[1] = right_idx;

    node_idx
}
