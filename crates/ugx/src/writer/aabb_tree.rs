//! AABB tree chunk (0x705) writer.
//!
//! Serializes an `AabbTree` back to the streamed format expected by
//! `BAABBTree_load` (`0x1406b5090` in xgameFinal.exe DE).

use alloc::vec::Vec;

use crate::error::Result;
use crate::types::aabb_tree::{AABB_NULL_INDEX, AABB_TREE_VERSION, AabbTree, AabbTreeNode};

/// Size of one in-memory node (used to convert indices → byte offsets).
const NODE_MEM_SIZE: u32 = 72;

/// Build the AABB tree chunk (0x705) data.
pub(super) fn build_aabb_tree_data(tree: &AabbTree) -> Result<Vec<u8>> {
    // Estimate capacity: version(4) + count(4) + nodes + sentinel(4)
    let est = 12 + tree.nodes.len() * 120; // generous estimate for variable tri_indices
    let mut buf = Vec::with_capacity(est);

    // Version header
    buf.extend_from_slice(&AABB_TREE_VERSION.to_le_bytes());

    // Node count
    buf.extend_from_slice(&(tree.nodes.len() as u32).to_le_bytes());

    // Nodes
    for node in &tree.nodes {
        write_node(&mut buf, node);
    }

    // Version sentinel
    buf.extend_from_slice(&AABB_TREE_VERSION.to_le_bytes());

    Ok(buf)
}

/// Write a single BNode to the buffer.
///
/// Stream order (matching IDA: `BAABBTreeNode_readFromStream` at `0x1406b4f90`):
/// mBounds, mpParent, mpChildren[0], mpChildren[1], mIndex, mObjIndices, mSplitPlane
fn write_node(buf: &mut Vec<u8>, node: &AabbTreeNode) {
    // mBounds (AABB = min[3] + max[3])
    for &v in &node.min {
        buf.extend_from_slice(&v.to_le_bytes());
    }
    for &v in &node.max {
        buf.extend_from_slice(&v.to_le_bytes());
    }

    // mpParent, mpChildren[0], mpChildren[1] as byte offsets
    buf.extend_from_slice(&index_to_offset(node.parent).to_le_bytes());
    buf.extend_from_slice(&index_to_offset(node.children[0]).to_le_bytes());
    buf.extend_from_slice(&index_to_offset(node.children[1]).to_le_bytes());

    // mIndex
    buf.extend_from_slice(&node.index.to_le_bytes());

    // mObjIndices (BDynamicArray<int>: count + i32 elements)
    buf.extend_from_slice(&(node.obj_indices.len() as u32).to_le_bytes());
    for &idx in &node.obj_indices {
        buf.extend_from_slice(&idx.to_le_bytes());
    }

    // mSplitPlane
    buf.extend_from_slice(&node.split_plane.to_le_bytes());
}

/// Convert a node index to a byte offset. `AABB_NULL_INDEX` stays as-is.
#[inline]
fn index_to_offset(index: u32) -> u32 {
    if index == AABB_NULL_INDEX {
        AABB_NULL_INDEX
    } else {
        index * NODE_MEM_SIZE
    }
}
