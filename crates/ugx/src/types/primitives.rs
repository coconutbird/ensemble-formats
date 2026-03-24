//! Primitive geometric types: AABB, Sphere, Keyframe.

use alloc::string::String;
use alloc::vec::Vec;

use crate::bytes::read_f32_le;
use crate::error::{Error, Result};

/// Axis-aligned bounding box.
#[derive(Debug, Clone, Default)]
pub struct AABB {
    /// Minimum corner [x, y, z].
    pub min: [f32; 3],
    /// Maximum corner [x, y, z].
    pub max: [f32; 3],
}

impl AABB {
    pub fn read(data: &[u8], pos: &mut usize) -> Result<Self> {
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
        Ok(Self { min, max })
    }
}

/// Bounding sphere.
#[derive(Debug, Clone, Default)]
pub struct Sphere {
    /// Center [x, y, z].
    pub center: [f32; 3],
    /// Radius.
    pub radius: f32,
}

impl Sphere {
    pub fn read(data: &[u8], pos: &mut usize) -> Result<Self> {
        let center = [
            read_f32_le(data, pos)?,
            read_f32_le(data, pos)?,
            read_f32_le(data, pos)?,
        ];
        let radius = read_f32_le(data, pos)?;
        Ok(Self { center, radius })
    }
}

/// Morph target keyframe.
#[derive(Debug, Clone, Default)]
pub struct Keyframe {
    /// Time in seconds.
    pub time: f32,
    /// Vertex data.
    pub verts: Vec<u8>,
}

impl Keyframe {
    pub fn read(data: &[u8], pos: &mut usize) -> Result<Self> {
        let time = read_f32_le(data, pos)?;
        let end4 = *pos + 4;
        if end4 > data.len() {
            return Err(Error::UnexpectedEof {
                context: String::from("keyframe length"),
            });
        }
        let len = u32::from_le_bytes([data[*pos], data[*pos + 1], data[*pos + 2], data[*pos + 3]])
            as usize;
        *pos = end4;
        let verts_end = *pos + len;
        if verts_end > data.len() {
            return Err(Error::UnexpectedEof {
                context: String::from("keyframe verts"),
            });
        }
        let verts = data[*pos..verts_end].to_vec();
        *pos = verts_end;
        Ok(Self { time, verts })
    }
}
