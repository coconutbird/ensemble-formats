//! Bone types: Bone, `GrannyBone`, `GrannyMesh`, and related skeletal data.

use alloc::string::String;
use alloc::vec::Vec;

use super::granny::{GrannyTypeMember, GrannyVariant};
use super::math::Matrix4x4;

// ---------------------------------------------------------------------------
// Bone types
// ---------------------------------------------------------------------------

/// Bone definition.
#[derive(Debug, Clone, Default)]
pub struct Bone {
    /// Bone name.
    pub name: String,
    /// Parent bone index (-1 for root).
    pub parent_index: i32,
    /// Model-to-bone transform (4x4 matrix in packed format).
    pub model_to_bone: Matrix4x4,
}

/// Local transform data from the Granny bone struct (68 bytes at +0x0C).
///
/// Stored verbatim to enable bit-perfect round-tripping. When `None`, the
/// writer will derive local transforms from the inverse world matrices
/// (legacy behaviour, lossy due to floating-point decomposition).
#[derive(Debug, Clone)]
pub struct GrannyLocalTransform {
    /// Transform flags (bitmask: 0x1 = position, 0x2 = orientation, 0x4 = `scale_shear`).
    pub flags: u32,
    /// Local position (xyz).
    pub position: [f32; 3],
    /// Local orientation quaternion (xyzw).
    pub orientation: [f32; 4],
    /// Local scale/shear matrix (3×3 row-major).
    pub scale_shear: [[f32; 3]; 3],
}

impl Default for GrannyLocalTransform {
    fn default() -> Self {
        Self {
            flags: 0,
            position: [0.0; 3],
            orientation: [0.0, 0.0, 0.0, 1.0],
            scale_shear: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        }
    }
}

/// Bone data from granny chunk (0x703).
/// This has the correct inverse world matrix for positioning bones.
#[derive(Debug, Clone, Default)]
pub struct GrannyBone {
    /// Bone name.
    pub name: String,
    /// Parent bone index (-1 for root).
    pub parent_index: i32,
    /// Local transform (flags, position, orientation, `scale_shear`).
    /// Preserved from the original file for bit-perfect round-tripping.
    /// When `None`, the writer derives transforms from inverse world matrices.
    pub local_transform: Option<GrannyLocalTransform>,
    /// Inverse world matrix (4x4) - read from offset 80 in granny bone struct.
    /// To get world matrix: invert then transpose this matrix.
    pub inverse_world_matrix: Matrix4x4,
    /// LOD error value (f32 at +0x90 in granny bone struct).
    /// Preserved from original for round-tripping. Defaults to 0.0.
    pub lod_error: f32,
    /// Extended data from the Granny2 variant system.
    /// Contains track masks, user-defined properties, etc.
    /// `None` means the bone has no extended data (both type and data pointers are null).
    pub extended_data: Option<GrannyVariant>,
    /// The raw type definition members for this bone's extended data.
    /// Stored so we can emit the exact same type layout on write.
    pub extended_data_type: Option<Vec<GrannyTypeMember>>,
}

/// A single bone binding entry within a Granny mesh (44 bytes on disk).
///
/// ```text
/// +0x00: u64  BoneName ptr
/// +0x08: f32  OBBMin[3]      (oriented bounding box minimum)
/// +0x14: f32  OBBMax[3]      (oriented bounding box maximum)
/// +0x20: i32  TriangleCount  (ReferenceToArray count)
/// +0x24: u64  TriangleIndices ptr (ReferenceToArray pointer)
/// ```
#[derive(Debug, Clone, Default)]
pub struct GrannyBoneBinding {
    /// Name of the bone this binding references.
    pub bone_name: String,
    /// Oriented bounding box minimum (per-bone culling).
    pub obb_min: [f32; 3],
    /// Oriented bounding box maximum (per-bone culling).
    pub obb_max: [f32; 3],
    /// Triangle indices for this bone binding (raw i32 values).
    /// Typically empty in UGX files but preserved for round-tripping.
    pub triangle_indices: Vec<i32>,
}

/// Mesh data from granny chunk (0x703).
/// Each mesh has a name and a list of bone bindings for skinning.
#[derive(Debug, Clone, Default)]
pub struct GrannyMesh {
    /// Mesh name (e.g., "`marine_01`", "optionalAssaultRifle").
    pub name: String,
    /// Full bone binding entries with OBB data.
    pub bone_bindings: Vec<GrannyBoneBinding>,
}
