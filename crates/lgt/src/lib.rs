//! LGT animated light-scene format parser for Halo Wars.
//!
//! An `.lgt` file is the little-endian `LightData::BScene` stream that the
//! original 3ds Max light exporter writes. The file has this structure:
//!
//! 1. A version word. The high 24 bits are the family [`VERSION_FAMILY`].
//! 2. A signed frame count, then the frames.
//! 3. A closing version word that must be equal to the opening word.
//!
//! Each frame has a time, then counted lists of lights, objects and cameras.
//! Strings are a `u32` byte length followed by the bytes. The reader replaces
//! invalid UTF-8 sequences with U+FFFD.

#![no_std]
extern crate alloc;

mod error;
mod reader;
mod records;

pub use error::{Error, Result};
pub use records::{
    Camera, CameraClip, CameraRange, Frame, Light, LightFlags, LightKind, Object, Scene,
};

/// High 24 bits of each supported version word.
pub const VERSION_FAMILY: u32 = 0x00CC_DD01;

/// First version that stores the light-buffer switch of a light.
///
/// Older versions do not store the switch. The reader then sets it.
pub const LIGHT_BUFFERED_VERSION: u32 = 0xCCDD_0109;

/// First version revision (low byte) that stores the camera ranges.
pub const CAMERA_RANGE_REVISION: u32 = 8;

/// First version revision (low byte) that stores the camera clip planes.
pub const CAMERA_CLIP_REVISION: u32 = 10;

/// Maximum value of a serialized record or byte count.
pub const MAX_RECORDS: usize = 1 << 20;
