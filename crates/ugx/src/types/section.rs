//! Mesh section type.

use alloc::vec::Vec;

use crate::vertex::packer::UnivertPacker;

/// Mesh section - a submesh with its own material and vertex format.
///
/// ## HW1 packed format (152 bytes / 0x98):
/// - +0x00: mMaterialIndex (i32)
/// - +0x04: mAccessoryIndex (i32)
/// - +0x08: mMaxBones (i32)
/// - +0x0C: mRigidBoneIndex (i32)
/// - +0x10: mIBOfs (i32, in indices not bytes)
/// - +0x14: mNumTris (i32)
/// - +0x18: mVBOfs (i32)
/// - +0x1C: mVBBytes (i32)
/// - +0x20: mVertSize (i32)
/// - +0x24: mNumVerts (i32)
/// - +0x28: BoneRemap packed array (16 bytes)
/// - +0x38: UnivertPacker (84 bytes)
/// - +0x8C: mRigidOnly (i32)
/// - +0x90: mGlobalBones (i32) - not in 2008 source!
/// - +0x94: mPadding (i32)
///
/// ## HW2 packed format (72 bytes / 0x48):
/// - +0x00: same 40 bytes of fixed fields as HW1
/// - +0x28: i32 flags (rigid_only / global_bones)
/// - +0x2C: i32 flags2
/// - +0x30: i32 unknown
/// - +0x34: i32 unknown2
/// - +0x38: BoneRemap packed array (16 bytes)
/// - No UnivertPacker (vertex format determined externally).
#[derive(Debug, Clone)]
pub struct Section {
    /// Material index.
    pub material_index: i32,
    /// Accessory index.
    pub accessory_index: i32,
    /// Maximum bones influencing this section.
    pub max_bones: i32,
    /// Rigid bone index (if rigid_only).
    pub rigid_bone_index: i32,
    /// Index buffer offset (in indices, not bytes).
    pub ib_offset: i32,
    /// Number of triangles.
    pub num_tris: i32,
    /// Vertex buffer offset (in bytes).
    pub vb_offset: i32,
    /// Vertex buffer size in bytes.
    pub vb_bytes: i32,
    /// Vertex stride in bytes.
    pub vert_size: i32,
    /// Number of vertices.
    pub num_verts: i32,
    /// Base vertex packer (HW1 only; `None` in HW2 where vertex format is external).
    pub base_vert_packer: Option<UnivertPacker>,
    /// Local-to-global bone remap table.
    /// Maps section-local bone indices to global skeleton indices.
    /// TODO: Entry size assumed u8 — may be u16/u32 for large skeletons. See ugx.rs.
    pub bone_remap: Vec<u8>,
    /// Is this section rigid (no skinning)?
    pub rigid_only: bool,
    /// Uses global bone indices (HW1-specific field).
    pub global_bones: bool,
}
