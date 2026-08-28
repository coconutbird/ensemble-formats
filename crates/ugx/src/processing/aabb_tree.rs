//! AABB tree construction for `UgxGeom`.
//!
//! Builds a section-level spatial BVH (bounding volume hierarchy) used by
//! the engine for collision and ray-casting queries.

use alloc::vec::Vec;

use crate::types::aabb_tree::{AABB_NULL_INDEX, AabbTree, AabbTreeNode};
use crate::{Error, Result, UgxGeom};

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
    ///
    /// # Errors
    ///
    /// Returns an error if section vertex data is malformed or a node or
    /// section index cannot be represented by the UGX format.
    pub fn rebuild_aabb_tree(&mut self) -> Result<Vec<Vec<i32>>> {
        if self.sections.is_empty() {
            self.aabb_tree = None;
            return Ok(Vec::new());
        }

        // Compute per-section AABBs
        let mut sec_aabbs = Vec::new();
        for section_index in 0..self.sections.len() {
            let vertices = self.unpack_section_vertices(section_index)?;
            if vertices.is_empty() {
                continue;
            }
            let mut section_min = [f32::MAX; 3];
            let mut section_max = [f32::MIN; 3];
            for vertex in &vertices {
                for axis in 0..3 {
                    section_min[axis] = section_min[axis].min(vertex.position[axis]);
                    section_max[axis] = section_max[axis].max(vertex.position[axis]);
                }
            }
            sec_aabbs.push(SectionAABB {
                section_idx: section_index,
                min: section_min,
                max: section_max,
                centroid: [
                    f32::midpoint(section_min[0], section_max[0]),
                    f32::midpoint(section_min[1], section_max[1]),
                    f32::midpoint(section_min[2], section_max[2]),
                ],
            });
        }

        if sec_aabbs.is_empty() {
            self.aabb_tree = None;
            return Ok(Vec::new());
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
        )?;

        self.aabb_tree = Some(AabbTree { nodes });
        Ok(node_sections)
    }
}

/// Add a leaf node and its section mapping.
fn push_leaf(
    sections: &[SectionAABB],
    indices: &[usize],
    nodes: &mut Vec<AabbTreeNode>,
    leaf_sections: &mut Vec<Vec<i32>>,
    parent_index: u32,
    min: [f32; 3],
    max: [f32; 3],
) -> Result<u32> {
    let node_index = crate::checked_u32(nodes.len(), "AABB-tree node index")?;
    let section_indices = indices
        .iter()
        .map(|&index| crate::checked_i32(sections[index].section_idx, "section index"))
        .collect::<Result<Vec<_>>>()?;
    nodes.push(AabbTreeNode {
        min,
        max,
        parent: parent_index,
        children: [AABB_NULL_INDEX, AABB_NULL_INDEX],
        index: 0,
        obj_indices: Vec::new(),
        split_plane: 0.0,
    });
    let node_position = crate::checked_usize(u64::from(node_index), "AABB-tree node index")?;
    leaf_sections.resize_with(node_position, Vec::new);
    leaf_sections.push(section_indices);
    Ok(node_index)
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
) -> Result<u32> {
    let (min, max) = compute_section_aabb(secs, indices);
    let node_idx = crate::checked_u32(nodes.len(), "AABB-tree node index")?;

    // Leaf: 1-2 sections or can't split
    if indices.len() <= 2 {
        return push_leaf(secs, indices, nodes, leaf_sections, parent_idx, min, max);
    }

    // Find longest axis of centroid spread
    let mut c_min = [f32::MAX; 3];
    let mut c_max = [f32::MIN; 3];
    for &section_index in indices {
        for axis in 0..3 {
            c_min[axis] = c_min[axis].min(secs[section_index].centroid[axis]);
            c_max[axis] = c_max[axis].max(secs[section_index].centroid[axis]);
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
        return push_leaf(secs, indices, nodes, leaf_sections, parent_idx, min, max);
    }

    let split_value = f32::midpoint(c_min[split_axis], c_max[split_axis]);

    // Partition
    let mut left: Vec<usize> = Vec::new();
    let mut right: Vec<usize> = Vec::new();
    for &section_index in indices {
        if secs[section_index].centroid[split_axis] <= split_value {
            left.push(section_index);
        } else {
            right.push(section_index);
        }
    }

    // Fallback: if one side empty, split sorted in half
    if left.is_empty() || right.is_empty() {
        let mut sorted: Vec<usize> = indices.to_vec();
        sorted.sort_by(|&left_index, &right_index| {
            secs[left_index].centroid[split_axis]
                .partial_cmp(&secs[right_index].centroid[split_axis])
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
    let node_position = crate::checked_usize(u64::from(node_idx), "AABB-tree node index")?;
    leaf_sections.resize_with(node_position, Vec::new);
    leaf_sections.push(Vec::new());

    let left_idx = build_section_bvh(secs, &left, nodes, leaf_sections, node_idx)?;
    nodes
        .get_mut(node_position)
        .ok_or(Error::SizeOverflow("AABB-tree node index"))?
        .children[0] = left_idx;

    let right_idx = build_section_bvh(secs, &right, nodes, leaf_sections, node_idx)?;
    nodes
        .get_mut(node_position)
        .ok_or(Error::SizeOverflow("AABB-tree node index"))?
        .children[1] = right_idx;

    Ok(node_idx)
}

/// Compute the AABB enclosing a set of section AABBs.
fn compute_section_aabb(secs: &[SectionAABB], indices: &[usize]) -> ([f32; 3], [f32; 3]) {
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for &section_index in indices {
        for axis in 0..3 {
            min[axis] = min[axis].min(secs[section_index].min[axis]);
            max[axis] = max[axis].max(secs[section_index].max[axis]);
        }
    }
    (min, max)
}
