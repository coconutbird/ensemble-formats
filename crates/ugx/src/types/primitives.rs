//! Primitive geometric types: AABB, Sphere, Keyframe.

use alloc::string::String;
use alloc::vec::Vec;

use nostdio::{ReadLe, SliceCursor};

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
    /// Read an AABB from 24 bytes of little-endian `f32` data (min xyz, max xyz).
    pub fn read(data: &[u8], pos: &mut usize) -> Result<Self> {
        let mut cur = SliceCursor::new(&data[*pos..]);
        let min = [cur.read_f32_le()?, cur.read_f32_le()?, cur.read_f32_le()?];
        let max = [cur.read_f32_le()?, cur.read_f32_le()?, cur.read_f32_le()?];
        *pos += cur.position();
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
    /// Read a bounding sphere from 16 bytes of little-endian `f32` data (center xyz, radius).
    pub fn read(data: &[u8], pos: &mut usize) -> Result<Self> {
        let mut cur = SliceCursor::new(&data[*pos..]);
        let center = [cur.read_f32_le()?, cur.read_f32_le()?, cur.read_f32_le()?];
        let radius = cur.read_f32_le()?;
        *pos += cur.position();
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
    /// Read a morph-target keyframe: 4-byte time (f32le) + 4-byte length (u32le) + vertex blob.
    pub fn read(data: &[u8], pos: &mut usize) -> Result<Self> {
        let mut cur = SliceCursor::new(&data[*pos..]);
        let time = cur.read_f32_le()?;
        let len = cur.read_u32_le()? as usize;
        *pos += cur.position();
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
