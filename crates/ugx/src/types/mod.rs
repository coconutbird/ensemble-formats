//! UGX data types - materials, bones, sections, etc.
//!
//! # C++ Equivalents
//!
//! These Rust types correspond to the following C++ types from the original source:
//!
//! | Rust Type       | C++ Type (xgeom/ugxGeom.h)       |
//! |-----------------|----------------------------------|
//! | `Section`       | `BUGXGeom::BSection`             |
//! | `Bone`          | `BUGXGeom::BBone`                |
//! | `Material`      | `Unigeom::BMaterial`             |
//! | `Map`           | `Unigeom::BMap`                  |
//! | `MapType`       | `Unigeom::eMapType`              |
//! | `UnivertPacker` | `Unigeom::BUnpacker`             |
//! | `Matrix4x4`     | `BMatrix` (row-major 4x4)        |
//! | `AABB`          | `AABB` (xcore/math/vectorTypes.h)|
//! | `Sphere`        | `BSphere`                        |
//! | `AabbTree`      | `BAABBTree` (xgeom/aabbTree.h)   |
//! | `AabbTreeNode`  | `BAABBTreeNode`                  |
//!
//! Note: The HW1 (Definitive Edition) format differs from the original Xbox 360
//! source due to x64 pointer sizes and some additional fields.

pub mod aabb_tree;
pub mod accessory;
pub mod bone;
pub mod convert;
pub mod geom;
pub mod granny;
pub mod material;
pub mod math;
pub mod primitives;
pub(crate) mod raw;
pub mod section;
pub mod version;

// Re-export all public types for convenient access.
pub use aabb_tree::{AabbTree, AabbTreeNode};
pub use accessory::Accessory;
pub use bone::{Bone, GrannyBone, GrannyBoneBinding, GrannyLocalTransform, GrannyMesh};
pub use geom::{GeometryFlags, UgxGeom};
pub use granny::{GrannyMemberType, GrannyTypeMember, GrannyVariant};
pub use material::{
    HoganMaterialData, LegacyMaterialData, Map, MapType, Material, MaterialData, ShaderPermutation,
};
// Re-export HoganFlag from the `ufx` crate where it's defined.
pub use primitives::{AABB, Keyframe, Sphere};
pub use section::Section;
pub use ufx::HoganFlag;
pub use version::UgxVersion;

// Re-export math types for convenient access.
pub use math::{Matrix4x4, QForm};

// Re-export from constants for backward compatibility.
pub use crate::constants::{UGX_FILE_ID, UGX_VERSION};
