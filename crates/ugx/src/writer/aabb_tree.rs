//! AABB tree chunk (0x705) writer.
//!
//! Serializes an `AabbTree` back to the streamed format expected by
//! `BAABBTree_load` (`0x1406b5090` in xgameFinal.exe).
//!
//! The engine's `BStream` reader always byte-swaps (flag `0x80` is never set),
//! so all fields must be written in **big-endian** byte order.

use alloc::vec::Vec;

use nostdio::WriteBe;

use crate::error::{Error, Result};
use crate::types::aabb_tree::{AABB_NULL_INDEX, AABB_TREE_VERSION, AabbTree, AabbTreeNode};

/// Size of one in-memory node (used to convert indices → byte offsets).
const NODE_MEM_SIZE: u32 = 72;

/// Build the AABB tree chunk (0x705) data.
pub(super) fn build_aabb_tree_data(tree: &AabbTree) -> Result<Vec<u8>> {
    // Estimate capacity: version(4) + count(4) + nodes + sentinel(4)
    let est = tree
        .nodes
        .len()
        .checked_mul(120)
        .and_then(|size| size.checked_add(12))
        .ok_or(Error::SizeOverflow("AABB-tree data"))?;
    let mut buf = Vec::with_capacity(est);

    // Version header (big-endian)
    buf.write_u32_be(AABB_TREE_VERSION)?;

    // Node count (big-endian)
    buf.write_u32_be(crate::checked_u32(
        tree.nodes.len(),
        "AABB-tree node count",
    )?)?;

    // Nodes
    for node in &tree.nodes {
        write_node(&mut buf, node)?;
    }

    // Version sentinel (big-endian)
    buf.write_u32_be(AABB_TREE_VERSION)?;

    Ok(buf)
}

/// Write a single `BNode` to the buffer.
///
/// Stream order (matching IDA: `BAABBTreeNode_readFromStream` at `0x1406b4f90`):
/// mBounds, mpParent, mpChildren[0], mpChildren[1], mIndex, mObjIndices, mSplitPlane
///
/// All fields are big-endian (the engine's `BStream` byte-swaps on read).
fn write_node(buf: &mut Vec<u8>, node: &AabbTreeNode) -> Result<()> {
    // mBounds (AABB = min[3] + max[3])
    for &v in &node.min {
        buf.write_f32_be(v)?;
    }
    for &v in &node.max {
        buf.write_f32_be(v)?;
    }

    // mpParent, mpChildren[0], mpChildren[1] as byte offsets
    buf.write_u32_be(index_to_offset(node.parent)?)?;
    buf.write_u32_be(index_to_offset(node.children[0])?)?;
    buf.write_u32_be(index_to_offset(node.children[1])?)?;

    // mIndex
    buf.write_u32_be(node.index)?;

    // mObjIndices (BDynamicArray<int>: count + i32 elements)
    buf.write_u32_be(crate::checked_u32(
        node.obj_indices.len(),
        "AABB-tree object count",
    )?)?;
    for &idx in &node.obj_indices {
        buf.write_i32_be(idx)?;
    }

    // mSplitPlane
    buf.write_f32_be(node.split_plane)?;
    Ok(())
}

/// Convert a node index to a byte offset. `AABB_NULL_INDEX` stays as-is.
#[inline]
fn index_to_offset(index: u32) -> Result<u32> {
    if index == AABB_NULL_INDEX {
        Ok(AABB_NULL_INDEX)
    } else {
        index
            .checked_mul(NODE_MEM_SIZE)
            .ok_or(Error::SizeOverflow("AABB-tree node offset"))
    }
}
