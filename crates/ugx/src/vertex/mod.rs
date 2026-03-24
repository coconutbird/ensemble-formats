//! Vertex format types and packing/unpacking logic.
//!
//! This module groups the vertex element type definitions and the UnivertPacker
//! which together describe how vertex attributes are packed in UGX vertex buffers.

pub mod element;
pub mod packer;

// Re-export primary types for convenient access.
pub use element::VertexElementType;
pub use packer::{MAX_UV, UnivertPacker, UnpackedVertex};
