//! UGX data types - materials, bones, sections, etc.

use byteorder::{LittleEndian, ReadBytesExt};
use std::io::Read;

use crate::error::Result;
use crate::univert_packer::UnivertPacker;

/// UGX file version magic.
pub const UGX_VERSION: u32 = 0xECDA1015;

/// Maximum string length for names.
#[allow(dead_code)]
const MAX_STRING_LEN: usize = 64;

/// Map types for materials.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MapType {
    Diffuse = 0,
    Specular = 1,
    Bump = 2,
    Env = 3,
    Self_ = 4,
}

impl MapType {
    pub const NUM_TYPES: usize = 5;
}

/// A texture map reference.
#[derive(Debug, Clone, Default)]
pub struct Map {
    /// Texture filename.
    pub name: String,
    /// UV channel index.
    pub channel: i32,
    /// Flags.
    pub flags: i32,
}

impl Map {
    pub fn read<R: Read>(reader: &mut R) -> Result<Self> {
        let name = read_string64(reader)?;
        let channel = reader.read_i32::<LittleEndian>()?;
        let flags = reader.read_i32::<LittleEndian>()?;
        Ok(Self { name, channel, flags })
    }
}

/// Container for maps of a single type (up to 4 per type).
#[derive(Debug, Clone, Default)]
pub struct MapContainer {
    pub maps: Vec<Map>,
}

impl MapContainer {
    pub fn read<R: Read>(reader: &mut R) -> Result<Self> {
        let num_maps = reader.read_i32::<LittleEndian>()? as usize;
        let mut maps = Vec::with_capacity(num_maps);
        for _ in 0..num_maps {
            maps.push(Map::read(reader)?);
        }
        Ok(Self { maps })
    }
}

/// Material definition.
#[derive(Debug, Clone, Default)]
pub struct Material {
    /// Material name.
    pub name: String,
    /// Texture maps by type (diffuse, specular, bump, env, self).
    pub maps: [MapContainer; MapType::NUM_TYPES],
    /// Material flags.
    pub flags: i32,
    /// Bump intensity.
    pub bumpiness: f32,
    /// Is this a skin material?
    pub skin: bool,
    /// Specular level.
    pub spec_level: f32,
    /// Specular power/shininess.
    pub spec_power: f32,
    /// Emissive intensity.
    pub emissive: f32,
    /// Diffuse color [r, g, b].
    pub diff_color: [f32; 3],
    /// Specular color [r, g, b].
    pub spec_color: [f32; 3],
    /// Self-illumination intensity.
    pub self_intensity: f32,
    /// Environment map intensity.
    pub env_intensity: f32,
}

impl Material {
    pub fn read<R: Read>(reader: &mut R) -> Result<Self> {
        let name = read_string64(reader)?;

        let mut maps: [MapContainer; MapType::NUM_TYPES] = Default::default();
        for map in &mut maps {
            *map = MapContainer::read(reader)?;
        }

        let flags = reader.read_i32::<LittleEndian>()?;
        let bumpiness = reader.read_f32::<LittleEndian>()?;
        let skin = reader.read_u8()? != 0;
        let spec_level = reader.read_f32::<LittleEndian>()?;
        let spec_power = reader.read_f32::<LittleEndian>()?;
        let emissive = reader.read_f32::<LittleEndian>()?;

        let diff_color = [
            reader.read_f32::<LittleEndian>()?,
            reader.read_f32::<LittleEndian>()?,
            reader.read_f32::<LittleEndian>()?,
        ];

        let spec_color = [
            reader.read_f32::<LittleEndian>()?,
            reader.read_f32::<LittleEndian>()?,
            reader.read_f32::<LittleEndian>()?,
        ];

        let self_intensity = reader.read_f32::<LittleEndian>()?;
        let env_intensity = reader.read_f32::<LittleEndian>()?;

        Ok(Self {
            name,
            maps,
            flags,
            bumpiness,
            skin,
            spec_level,
            spec_power,
            emissive,
            diff_color,
            spec_color,
            self_intensity,
            env_intensity,
        })
    }
}

/// Quaternion + translation transform.
#[derive(Debug, Clone, Default)]
pub struct QForm {
    /// Quaternion [x, y, z, w].
    pub rotation: [f32; 4],
    /// Translation [x, y, z].
    pub translation: [f32; 3],
}

impl QForm {
    pub fn read<R: Read>(reader: &mut R) -> Result<Self> {
        let rotation = [
            reader.read_f32::<LittleEndian>()?,
            reader.read_f32::<LittleEndian>()?,
            reader.read_f32::<LittleEndian>()?,
            reader.read_f32::<LittleEndian>()?,
        ];
        let translation = [
            reader.read_f32::<LittleEndian>()?,
            reader.read_f32::<LittleEndian>()?,
            reader.read_f32::<LittleEndian>()?,
        ];
        Ok(Self { rotation, translation })
    }
}

/// Bone definition.
#[derive(Debug, Clone, Default)]
pub struct Bone {
    /// Bone name.
    pub name: String,
    /// Parent bone index (-1 for root).
    pub parent_index: i32,
    /// Model-to-bone transform.
    pub model_to_bone: QForm,
}

impl Bone {
    pub fn read<R: Read>(reader: &mut R) -> Result<Self> {
        let name = read_string64(reader)?;
        let parent_index = reader.read_i32::<LittleEndian>()?;
        let model_to_bone = QForm::read(reader)?;
        Ok(Self { name, parent_index, model_to_bone })
    }
}

/// Axis-aligned bounding box.
#[derive(Debug, Clone, Default)]
pub struct AABB {
    /// Minimum corner [x, y, z].
    pub min: [f32; 3],
    /// Maximum corner [x, y, z].
    pub max: [f32; 3],
}

impl AABB {
    pub fn read<R: Read>(reader: &mut R) -> Result<Self> {
        let min = [
            reader.read_f32::<LittleEndian>()?,
            reader.read_f32::<LittleEndian>()?,
            reader.read_f32::<LittleEndian>()?,
        ];
        let max = [
            reader.read_f32::<LittleEndian>()?,
            reader.read_f32::<LittleEndian>()?,
            reader.read_f32::<LittleEndian>()?,
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
    pub fn read<R: Read>(reader: &mut R) -> Result<Self> {
        let center = [
            reader.read_f32::<LittleEndian>()?,
            reader.read_f32::<LittleEndian>()?,
            reader.read_f32::<LittleEndian>()?,
        ];
        let radius = reader.read_f32::<LittleEndian>()?;
        Ok(Self { center, radius })
    }
}

/// Mesh section - a submesh with its own material and vertex format.
///
/// DE packed format is 152 bytes (0x98):
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
    /// Base vertex packer.
    pub base_vert_packer: UnivertPacker,
    /// Is this section rigid (no skinning)?
    pub rigid_only: bool,
    /// Uses global bone indices (DE-specific field).
    pub global_bones: bool,
}

// Note: Section is read via UgxGeom::read_packed_section() in ugx.rs
// The packed DE format (152 bytes) is different from the original Xbox 360 format.

/// Morph target keyframe.
#[derive(Debug, Clone, Default)]
pub struct Keyframe {
    /// Time in seconds.
    pub time: f32,
    /// Vertex data.
    pub verts: Vec<u8>,
}

impl Keyframe {
    pub fn read<R: Read>(reader: &mut R) -> Result<Self> {
        let time = reader.read_f32::<LittleEndian>()?;
        let len = reader.read_u32::<LittleEndian>()? as usize;
        let mut verts = vec![0u8; len];
        reader.read_exact(&mut verts)?;
        Ok(Self { time, verts })
    }
}

/// Read a fixed-size string (64 bytes max, null-terminated).
fn read_string64<R: Read>(reader: &mut R) -> Result<String> {
    let len = reader.read_u32::<LittleEndian>()? as usize;
    if len == 0 {
        return Ok(String::new());
    }

    let mut bytes = vec![0u8; len];
    reader.read_exact(&mut bytes)?;

    // Remove null terminator if present
    while bytes.last() == Some(&0) {
        bytes.pop();
    }

    Ok(String::from_utf8(bytes)?)
}
