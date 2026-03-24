//! Bone types: Bone, GrannyBone, GrannyMesh.

use alloc::string::String;
use alloc::vec::Vec;

use crate::math::Matrix4x4;

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

/// Bone data from granny chunk (0x703).
/// This has the correct inverse world matrix for positioning bones.
#[derive(Debug, Clone, Default)]
pub struct GrannyBone {
    /// Bone name.
    pub name: String,
    /// Parent bone index (-1 for root).
    pub parent_index: i32,
    /// Inverse world matrix (4x4) - read from offset 80 in granny bone struct.
    /// To get world matrix: invert then transpose this matrix.
    pub inverse_world_matrix: Matrix4x4,
}

/// Mesh data from granny chunk (0x703).
/// Each mesh has a name and a list of bone bindings for skinning.
#[derive(Debug, Clone, Default)]
pub struct GrannyMesh {
    /// Mesh name (e.g., "marine_01", "optionalAssaultRifle").
    pub name: String,
    /// Bone names that this mesh is bound to (for skinning).
    /// Each entry is the name of a bone in the skeleton.
    pub bone_bindings: Vec<String>,
}
