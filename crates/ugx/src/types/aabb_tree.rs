//! AABB Tree types — spatial acceleration structure for collision/ray queries.
//!
//! # C++ Equivalents
//!
//! | Rust Type       | C++ Type (xgeom/aabbTree.h)      |
//! |-----------------|----------------------------------|
//! | `AabbTree`      | `BAABBTree`                      |
//! | `AabbTreeNode`  | `BAABBTree::BNode`               |
//!
//! # Stream Format (chunk 0x705)
//!
//! Verified against `BAABBTreeNode_readFromStream` at `0x1406b4f90` in xgameFinal.exe (HW1):
//! ```text
//! u32: version (0x33440002)
//! u32: node_count
//! for each node (BNode serialization order):
//!   f32[3]: mBounds.min
//!   f32[3]: mBounds.max
//!   u32: mpParent      (byte offset, 0xFFFFFFFF = NULL)
//!   u32: mpChildren[0] (byte offset, 0xFFFFFFFF = NULL)
//!   u32: mpChildren[1] (byte offset, 0xFFFFFFFF = NULL)
//!   u32: mIndex
//!   u32: mObjIndices.count
//!   i32[count]: mObjIndices elements (triangle indices)
//!   f32: mSplitPlane
//! u32: version sentinel (0x33440002)
//! ```
//!
//! On load, `BAABBTree_fixupPointers` (`0x1406b5210`) converts the byte
//! offsets at memory offsets +40, +48, +56 back to pointers. We normalise
//! these to node indices (offset / 72) on read and convert back on write.

use alloc::vec::Vec;

// Re-export from constants for backward compatibility.
pub use crate::constants::{AABB_NULL_INDEX, AABB_TREE_VERSION};

/// Parsed AABB tree — spatial acceleration structure.
///
/// Used by the game engine for collision detection and ray-casting queries.
/// Each UGX file may optionally contain one AABB tree (chunk 0x705).
#[derive(Debug, Clone)]
pub struct AabbTree {
    /// Tree nodes. Index 0 is the root.
    pub nodes: Vec<AabbTreeNode>,
}

/// A single node in the AABB tree (`BAABBTree::BNode`).
///
/// Interior nodes have `children[0]` and `children[1]` set.
/// Leaf nodes have `obj_indices` populated with triangle indices
/// into the mesh's index buffer.
///
/// # Memory Layout (72 bytes, x64 — verified via IDA fixup at `0x1406b5210`)
///
/// ```text
/// +0x00: AABB     mBounds        (24 bytes: min[3] + max[3])
/// +0x18: IntVec   mObjIndices    (16 bytes: BDynamicArray<int>)
/// +0x28: BNode*   mpParent       (8 bytes)  — fixup target
/// +0x30: BNode*   mpChildren[2]  (16 bytes) — fixup targets
/// +0x40: uint32   mIndex         (4 bytes)
/// +0x44: float    mSplitPlane    (4 bytes)
/// ```
#[derive(Debug, Clone)]
pub struct AabbTreeNode {
    /// AABB minimum corner [x, y, z].
    pub min: [f32; 3],
    /// AABB maximum corner [x, y, z].
    pub max: [f32; 3],
    /// Parent node index (`AABB_NULL_INDEX` = none / root).
    pub parent: u32,
    /// Child node indices (`AABB_NULL_INDEX` = none). `children[0]` = left, `children[1]` = right.
    pub children: [u32; 2],
    /// Node index (used by the engine for bookkeeping).
    pub index: u32,
    /// Object (triangle) indices for leaf nodes (empty for interior nodes).
    pub obj_indices: Vec<i32>,
    /// Split plane value (axis-aligned split position for interior nodes).
    pub split_plane: f32,
}
