//! AABB tree chunk (0x705) parser.
//!
//! Reads the streamed AABB tree format loaded by `BAABBTree_load` (`0x1406b5090`)
//! in xgameFinal.exe (HW1). Node parsing verified against
//! `BAABBTreeNode_readFromStream` (`0x1406b4f90`).
//!
//! Some HW1 HW1 ERA archives contain big-endian AABB tree data.
//! `BAABBTree_load` handles this via a stream endianness flag (`a1[3] & 0x80`).
//! We detect endianness by checking whether the version field reads correctly
//! as LE; if not, we try BE and read all fields in big-endian byte order.

use alloc::vec::Vec;

use nostdio::{Cursor, Endian, ReadEndian, ReadLe, Seek, SeekFrom};

use crate::error::{Error, Result};
use crate::types::aabb_tree::{AABB_NULL_INDEX, AABB_TREE_VERSION, AabbTree, AabbTreeNode};

/// Size of one in-memory `BNode` (used to convert byte offsets → indices).
const NODE_MEM_SIZE: u32 = 72;

/// Parse an AABB tree from chunk 0x705 data.
///
/// Auto-detects endianness: tries LE first, falls back to BE if the version
/// field is byte-swapped (e.g. `0x02004433` instead of `0x33440002`).
pub(super) fn read_aabb_tree(data: &[u8]) -> Result<AabbTree> {
    let mut cur = Cursor::new(data);

    // Detect endianness from the version header.
    let little_endian_version = cur.read_u32_le()?;
    let endian = if little_endian_version == AABB_TREE_VERSION {
        Endian::Little
    } else {
        // Reset and try big-endian.
        cur.seek(SeekFrom::Start(0))?;
        let big_endian_version = cur.read_u32(Endian::Big)?;
        if big_endian_version == AABB_TREE_VERSION {
            Endian::Big
        } else {
            return Err(Error::InvalidVersion {
                expected: AABB_TREE_VERSION,
                actual: little_endian_version,
            });
        }
    };

    // Node count (BDynamicArray serialization: count first, then elements)
    let node_count =
        crate::checked_usize(u64::from(cur.read_u32(endian)?), "AABB-tree node count")?;

    let mut nodes = Vec::with_capacity(node_count);
    for _ in 0..node_count {
        nodes.push(read_node(&mut cur, endian)?);
    }

    // Version sentinel
    let sentinel = cur.read_u32(endian)?;
    if sentinel != AABB_TREE_VERSION {
        return Err(Error::InvalidVersion {
            expected: AABB_TREE_VERSION,
            actual: sentinel,
        });
    }

    Ok(AabbTree { nodes })
}

/// Read a single `BNode` from the stream.
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
fn read_node(cur: &mut Cursor<&[u8]>, endian: Endian) -> Result<AabbTreeNode> {
    // mBounds (AABB = min[3] + max[3])
    let min = [
        cur.read_f32(endian)?,
        cur.read_f32(endian)?,
        cur.read_f32(endian)?,
    ];
    let max = [
        cur.read_f32(endian)?,
        cur.read_f32(endian)?,
        cur.read_f32(endian)?,
    ];

    // Pointers serialized as byte offsets (via offsetize). Convert to node indices.
    let parent = offset_to_index(cur.read_u32(endian)?);
    let child0 = offset_to_index(cur.read_u32(endian)?);
    let child1 = offset_to_index(cur.read_u32(endian)?);

    // mIndex
    let index = cur.read_u32(endian)?;

    // mObjIndices (IntVec = BDynamicArray<int>, serialized as count + elements)
    let obj_count =
        crate::checked_usize(u64::from(cur.read_u32(endian)?), "AABB-tree object count")?;
    let mut obj_indices = Vec::with_capacity(obj_count);
    for _ in 0..obj_count {
        obj_indices.push(cur.read_i32(endian)?);
    }

    // mSplitPlane
    let split_plane = cur.read_f32(endian)?;

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
