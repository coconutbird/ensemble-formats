//! Accessory type for UGX models.

use alloc::vec::Vec;

/// A model accessory (C++ `Unigeom::BAccessory`).
///
/// Accessories group bones and reference object (section) indices.
/// Layout verified from IDA `BPackedArray_Accessories__unpack` at `0x1406d8660`.
#[derive(Debug, Clone, PartialEq)]
pub struct Accessory {
    /// First bone index in this accessory group.
    pub first_bone: i32,
    /// Number of bones in this accessory group.
    pub num_bones: i32,
    /// Section/object indices that belong to this accessory.
    pub object_indices: Vec<i32>,
}
