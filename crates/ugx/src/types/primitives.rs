//! Primitive geometric types: AABB, Sphere, Keyframe.

use alloc::string::String;
use alloc::vec::Vec;

use nostdio::{Cursor, ReadLe};

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
    ///
    /// # Errors
    ///
    /// Returns an error if the input is truncated or the cursor position
    /// cannot be represented on the target platform.
    pub fn read(data: &[u8], pos: &mut usize) -> Result<Self> {
        let remaining = data.get(*pos..).ok_or_else(|| Error::UnexpectedEof {
            context: "AABB".into(),
        })?;
        let mut cur = Cursor::new(remaining);
        let min = [cur.read_f32_le()?, cur.read_f32_le()?, cur.read_f32_le()?];
        let max = [cur.read_f32_le()?, cur.read_f32_le()?, cur.read_f32_le()?];
        crate::advance_position(pos, cur.position(), "binary cursor position")?;
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
    ///
    /// # Errors
    ///
    /// Returns an error if the input is truncated or the cursor position
    /// cannot be represented on the target platform.
    pub fn read(data: &[u8], pos: &mut usize) -> Result<Self> {
        let remaining = data.get(*pos..).ok_or_else(|| Error::UnexpectedEof {
            context: "bounding sphere".into(),
        })?;
        let mut cur = Cursor::new(remaining);
        let center = [cur.read_f32_le()?, cur.read_f32_le()?, cur.read_f32_le()?];
        let radius = cur.read_f32_le()?;
        crate::advance_position(pos, cur.position(), "binary cursor position")?;
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
    ///
    /// # Errors
    ///
    /// Returns an error if the input is truncated or a keyframe size cannot be
    /// represented on the target platform.
    pub fn read(data: &[u8], pos: &mut usize) -> Result<Self> {
        let remaining = data.get(*pos..).ok_or_else(|| Error::UnexpectedEof {
            context: "keyframe".into(),
        })?;
        let mut cur = Cursor::new(remaining);
        let time = cur.read_f32_le()?;
        let len = crate::checked_usize(u64::from(cur.read_u32_le()?), "keyframe vertex data")?;
        crate::advance_position(pos, cur.position(), "binary cursor position")?;
        let Some(verts_end) = pos.checked_add(len) else {
            return Err(Error::SizeOverflow("keyframe vertex data"));
        };
        let Some(verts) = data.get(*pos..verts_end) else {
            return Err(Error::UnexpectedEof {
                context: String::from("keyframe verts"),
            });
        };
        let verts = verts.to_vec();
        *pos = verts_end;
        Ok(Self { time, verts })
    }
}
