//! AABB tree construction for `UgxGeom`.
//!
//! Builds a section-level spatial BVH (bounding volume hierarchy) used by
//! the engine for collision and ray-casting queries.

use alloc::vec::Vec;

use crate::UgxGeom;
use crate::types::aabb_tree::{AABB_NULL_INDEX, AabbTree, AabbTreeNode};

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
            obj_indices: Vec::new(),
            split_plane: 0.0,
        });
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
