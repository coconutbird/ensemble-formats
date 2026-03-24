//! AABB tree chunk (0x705) parser.
//!
//! Reads the streamed AABB tree format loaded by `BAABBTree_load` (`0x1406b5090`)
//! in xgameFinal.exe (DE). Node parsing verified against
//! `BAABBTreeNode_readFromStream` (`0x1406b4f90`).

use alloc::vec::Vec;

use crate::bytes::{read_f32_le, read_i32_le, read_u32_le};
use crate::error::{Error, Result};
use crate::types::aabb_tree::{AABB_NULL_INDEX, AABB_TREE_VERSION, AabbTree, AabbTreeNode};

/// Size of one in-memory BNode (used to convert byte offsets → indices).
const NODE_MEM_SIZE: u32 = 72;

/// Parse an AABB tree from chunk 0x705 data.
pub(super) fn read_aabb_tree(data: &[u8]) -> Result<AabbTree> {
    let pos = &mut 0usize;

    // Version header
    let version = read_u32_le(data, pos)?;
    if version != AABB_TREE_VERSION {
        return Err(Error::InvalidVersion {
            expected: AABB_TREE_VERSION,
            actual: version,
        });
    }

    // Node count (BDynamicArray serialization: count first, then elements)
    let node_count = read_u32_le(data, pos)? as usize;

    let mut nodes = Vec::with_capacity(node_count);
    for _ in 0..node_count {
        nodes.push(read_node(data, pos)?);
    }

    // Version sentinel
    let sentinel = read_u32_le(data, pos)?;
    if sentinel != AABB_TREE_VERSION {
        return Err(Error::InvalidVersion {
            expected: AABB_TREE_VERSION,
            actual: sentinel,
        });
    }

    Ok(AabbTree { nodes })
}

/// Read a single BNode from the stream.
///
/// Stream order (from IDA: `BAABBTreeNode_readFromStream` at `0x1406b4f90`):
/// ```text
/// mBounds          → AABB (6× f32)
/// mpParent         → u32 byte offset
/// mpChildren[0]    → u32 byte offset
/// mpChildren[1]    → u32 byte offset
/// mIndex           → u32
/// mObjIndices      → BDynamicArray<int> (count + i32[])
/// mSplitPlane      → f32
/// ```
fn read_node(data: &[u8], pos: &mut usize) -> Result<AabbTreeNode> {
    // mBounds (AABB = min[3] + max[3])
    let min = [
        read_f32_le(data, pos)?,
        read_f32_le(data, pos)?,
        read_f32_le(data, pos)?,
    ];
    let max = [
        read_f32_le(data, pos)?,
        read_f32_le(data, pos)?,
        read_f32_le(data, pos)?,
    ];

    // Pointers serialized as byte offsets (via offsetize). Convert to node indices.
    let parent = offset_to_index(read_u32_le(data, pos)?);
    let child0 = offset_to_index(read_u32_le(data, pos)?);
    let child1 = offset_to_index(read_u32_le(data, pos)?);

    // mIndex
    let index = read_u32_le(data, pos)?;

    // mObjIndices (IntVec = BDynamicArray<int>, serialized as count + elements)
    let obj_count = read_u32_le(data, pos)? as usize;
    let mut obj_indices = Vec::with_capacity(obj_count);
    for _ in 0..obj_count {
        obj_indices.push(read_i32_le(data, pos)?);
    }

    // mSplitPlane
    let split_plane = read_f32_le(data, pos)?;

    Ok(AabbTreeNode {
        min,
        max,
        parent,
        children: [child0, child1],
        index,
        obj_indices,
        split_plane,
    })
}

/// Convert a byte offset to a node index. `0xFFFFFFFF` stays as-is (null).
#[inline]
fn offset_to_index(offset: u32) -> u32 {
    if offset == AABB_NULL_INDEX {
        AABB_NULL_INDEX
    } else {
        offset / NODE_MEM_SIZE
    }
}
