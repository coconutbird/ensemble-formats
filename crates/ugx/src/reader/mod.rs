//! UGX file reader.
//!
//! # File Format Overview
//!
//! UGX (Unit Graphics) files contain 3D model data for Halo Wars. They are stored
//! inside ECF (Ensemble Common Format) containers with multiple chunks.
//!
//! ## ECF Container Structure
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │ ECF Header (32 bytes, big-endian)                                   │
//! │   Magic: 0xDABA7737                                                 │
//! ├─────────────────────────────────────────────────────────────────────┤
//! │ Chunk Headers (24 bytes each, big-endian)                           │
//! ├─────────────────────────────────────────────────────────────────────┤
//! │ Chunk 0x700: BCachedData - Header, sections, bones, accessories     │
//! │ Chunk 0x701: Index Buffer - Triangle indices (u16 array)            │
//! │ Chunk 0x702: Vertex Buffer - Packed vertex data                     │
//! │ Chunk 0x703: Granny Data - Bone inverse world matrices (optional)   │
//! │ Chunk 0x704: Materials - BBinaryDataTree document (optional)        │
//! │ Chunk 0x705: AABB Tree - Spatial acceleration structure (optional)  │
//!
//! The AABB tree is a streamed format (not flat binary) with variable-length
//! triangle index arrays per node. See `reader/aabb_tree.rs` for details.
//! └─────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Packed Data Format (Definitive Edition x64)
//!
//! The BCachedData chunk uses a "packed" format where pointers are stored as
//! offsets relative to the chunk start. This makes the data position-independent.
//!
//! ### Packed Array Layout (16 bytes)
//!
//! Corresponds to C++ `BPackedArray<T>`:
//! ```text
//! +0x00: uint32 size      - Number of elements
//! +0x04: uint32 padding   - Alignment padding (always 0)
//! +0x08: uint64 offset    - Offset from chunk start (0xFFFFFFFFFFFFFFFF = NULL)
//! ```
//!
//! ### Packed String Layout (8 bytes)
//!
//! Corresponds to C++ `BPackedString`:
//! ```text
//! +0x00: uint64 offset    - Offset to null-terminated string (0xFFFFFFFFFFFFFFFF = NULL)
//! ```
//!
//! ## BCachedData Layout (Chunk 0x700)
//!
//! Corresponds to C++ `BUGXGeom::BCachedData`:
//! ```text
//! +0x00: BHeader (60 bytes)
//!        +0x00: uint32 signature (0xC2340004)
//!        +0x04: int32 rigidBoneIndex
//!        +0x08: Sphere boundingSphere (16 bytes)
//!        +0x18: AABB bounds (24 bytes)
//!        +0x30: int16 maxInstances
//!        +0x32: int16 instanceIndexMultiplier
//!        +0x34: int16 largeGeomBoneIndex
//!        +0x36: bool allSectionsRigid, globalBones, allSectionsSkinned, rigidOnly
//!        +0x3A: padding to 0x40
//! +0x40: BPackedArray<BSection> sections (16 bytes)
//! +0x50: BPackedArray<BBone> bones (16 bytes)
//! +0x60: BPackedArray<BAccessory> accessories (16 bytes)
//! +0x70: BPackedArray<BAccessory> validAccessories (16 bytes)
//! +0x80: BPackedArray<BVector3> boneBoundsLow (16 bytes)
//! +0x90: BPackedArray<BVector3> boneBoundsHigh (16 bytes)
//! ```
//!
//! ## BSection Layout (152 bytes on-disk)
//!
//! Corresponds to C++ `BUGXGeom::BSection`:
//! ```text
//! +0x00: int32 materialIndex
//! +0x04: int32 accessoryIndex
//! +0x08: int32 maxBones
//! +0x0C: int32 rigidBoneIndex
//! +0x10: int32 ibOffset (in indices, NOT bytes!)
//! +0x14: int32 numTris
//! +0x18: int32 vbOffset
//! +0x1C: int32 vbBytes
//! +0x20: int32 vertSize
//! +0x24: int32 numVerts
//! +0x28: BPackedArray<uint8> localToGlobalBoneRemap (16 bytes)
//! +0x38: UnivertPacker baseVertPacker (84 bytes)
//! +0x8C: int32 rigidOnly (bool as int)
//! +0x90: int32 globalBones (DE-specific, not in 2008 source!)
//! +0x94: int32 padding
//! ```
//!
//! ## UnivertPacker Layout (84 bytes on-disk)
//!
//! Corresponds to C++ `Unigeom::BUnpacker`:
//! ```text
//! +0x00: BPackedString packOrder (8 bytes)
//! +0x08: BPackedString declOrder (8 bytes)
//! +0x10: VertexElementType[14] types (56 bytes) - pos, basis, basisScale, tangent,
//!        normal, uv[8], indices, weights, diffuse, index
//! +0x48: end (total 84 bytes, differs from x64 in-memory which is 104 bytes)
//! ```

mod aabb_tree;
mod cached_data;
mod granny;
mod material;

use alloc::string::String;
use alloc::vec::Vec;
use zerocopy::Ref;

use crate::bytes::read_u16_le;
use crate::constants::*;
use crate::error::{Error, Result};
use crate::types::raw::GeomHeaderRaw;
use crate::types::*;

use crate::types::UgxVersion;
use cached_data::{
    read_bone_bounds, read_packed_accessories, read_packed_bones, read_packed_sections,
    read_valid_accessory_indices,
};
use granny::{parse_granny_bones, parse_granny_meshes, validate_granny_chunk};
use material::read_materials as parse_materials;

impl UgxGeom {
    /// Parse UGX geometry from a byte slice (ECF container).
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let ecf = ecf::Reader::new(data)?;

        let cached_data = ecf
            .chunk_data_by_id(ECF_CACHED_DATA_CHUNK_ID)
            .map_err(|_| Error::MissingChunk("cached_data (0x700)"))?;

        let vertex_buffer = ecf
            .chunk_data_by_id(ECF_VB_CHUNK_ID)
            .map_err(|_| Error::MissingChunk("vertex_buffer (0x702)"))?;

        let ib_data = ecf
            .chunk_data_by_id(ECF_IB_CHUNK_ID)
            .map_err(|_| Error::MissingChunk("index_buffer (0x701)"))?;

        let granny_data = ecf.chunk_data_by_id(ECF_GRANNY_CHUNK_ID).ok();
        let material_data = ecf.chunk_data_by_id(ECF_MATERIAL_CHUNK_ID).ok();
        let aabb_tree_raw = ecf.chunk_data_by_id(ECF_AABB_TREE_CHUNK_ID).ok();

        let num_indices = ib_data.len() / 2;
        let mut index_buffer = Vec::with_capacity(num_indices);
        let mut ib_pos = 0usize;
        for _ in 0..num_indices {
            index_buffer.push(read_u16_le(&ib_data, &mut ib_pos)?);
        }

        Self::parse_cached_data(
            &cached_data,
            granny_data,
            material_data,
            aabb_tree_raw,
            vertex_buffer,
            index_buffer,
        )
    }

    /// Parse the cached data chunk (0x700) containing header, sections, bones, etc.
    fn parse_cached_data(
        data: &[u8],
        granny_data: Option<Vec<u8>>,
        material_data: Option<Vec<u8>>,
        aabb_tree_raw: Option<Vec<u8>>,
        vertex_buffer: Vec<u8>,
        index_buffer: Vec<u16>,
    ) -> Result<Self> {
        let (hdr, rest): (Ref<_, GeomHeaderRaw>, _) =
            Ref::from_prefix(data).map_err(|_| Error::UnexpectedEof {
                context: String::from("GeomHeaderRaw"),
            })?;

        let signature = u32::from_le_bytes(hdr.signature);
        let version = match signature {
            GEOM_HEADER_SIGNATURE_HW1 => UgxVersion::Hw1,
            GEOM_HEADER_SIGNATURE_HW2 => UgxVersion::Hw2,
            _ => return Err(Error::InvalidSignature { actual: signature }),
        };

        let rigid_bone_index = i32::from_le_bytes(hdr.rigid_bone_index);

        let bounding_sphere = Sphere {
            center: [
                f32::from_le_bytes(hdr.sphere_center[0]),
                f32::from_le_bytes(hdr.sphere_center[1]),
                f32::from_le_bytes(hdr.sphere_center[2]),
            ],
            radius: f32::from_le_bytes(hdr.sphere_radius),
        };

        let bounds = AABB {
            min: [
                f32::from_le_bytes(hdr.aabb_min[0]),
                f32::from_le_bytes(hdr.aabb_min[1]),
                f32::from_le_bytes(hdr.aabb_min[2]),
            ],
            max: [
                f32::from_le_bytes(hdr.aabb_max[0]),
                f32::from_le_bytes(hdr.aabb_max[1]),
                f32::from_le_bytes(hdr.aabb_max[2]),
            ],
        };

        let max_instances = i16::from_le_bytes(hdr.max_instances);
        let instance_index_multiplier = i16::from_le_bytes(hdr.instance_index_multiplier);
        let large_geom_bone_index = i16::from_le_bytes(hdr.large_geom_bone_index);
        let all_sections_rigid = hdr.all_sections_rigid != 0;
        let global_bones = hdr.global_bones != 0;
        let all_sections_skinned = hdr.all_sections_skinned != 0;
        let rigid_only = hdr.rigid_only != 0;

        let pos = &mut (data.len() - rest.len());

        let sections = read_packed_sections(data, pos, version)?;
        let bones = read_packed_bones(data, pos)?;

        // Validate the Granny chunk: the engine checks FromFileName == "gr2ugx"
        // at +0x10 before parsing. If the chunk exists but is invalid, error out.
        if let Some(ref granny) = granny_data {
            validate_granny_chunk(granny)?;
        }

        let (granny_bones, skeleton_lod_type) = if let Some(ref granny) = granny_data {
            parse_granny_bones(granny)?
        } else {
            (Vec::new(), 0)
        };

        let granny_meshes = if let Some(ref granny) = granny_data {
            parse_granny_meshes(granny)?
        } else {
            Vec::new()
        };

        let accessories = read_packed_accessories(data, pos)?;
        // IDA: BUGXGeomData::readCachedData uses BPackedArray_Simple__unpack for
        // validAccessories (v6+28) in BOTH HW1 and HW2 — they are i32 indices into
        // the accessories array, not full 24-byte AccessoryRaw structs.
        let valid_accessories = read_valid_accessory_indices(data, pos, &accessories)?;

        let bone_bounds = read_bone_bounds(data, pos)?;

        let materials = if let Some(ref mat_data) = material_data {
            parse_materials(mat_data).unwrap_or_default()
        } else {
            Vec::new()
        };

        let aabb_tree = if let Some(ref tree_data) = aabb_tree_raw {
            Some(aabb_tree::read_aabb_tree(tree_data)?)
        } else {
            None
        };

        Ok(Self {
            bounding_sphere,
            bounds,
            materials,
            bones,
            granny_bones,
            granny_meshes,
            skeleton_lod_type,
            bone_bounds,
            sections,
            accessories,
            valid_accessories,
            vertex_buffer,
            index_buffer,
            rigid_only,
            rigid_bone_index,
            max_instances,
            instance_index_multiplier,
            large_geom_bone_index,
            all_sections_rigid,
            all_sections_skinned,
            global_bones,
            aabb_tree,
        })
    }
}

/// Read only the materials from a UGX file, skipping geometry, bones, etc.
///
/// Opens the ECF container, extracts chunk 0x704 (materials), and parses
/// the BBinaryDataTree document. This is much cheaper than a full
/// [`UgxGeom::from_bytes`] parse when you only need texture/material info.
pub fn read_materials(data: &[u8]) -> Result<Vec<Material>> {
    let ecf = ecf::Reader::new(data)?;
    match ecf.chunk_data_by_id(ECF_MATERIAL_CHUNK_ID) {
        Ok(mat_data) => parse_materials(&mat_data),
        Err(_) => Ok(Vec::new()),
    }
}

/// UGX file reader.
pub struct Reader;

impl Reader {
    /// Read a UGX file from a byte slice.
    pub fn read(data: &[u8]) -> Result<UgxGeom> {
        UgxGeom::from_bytes(data)
    }
}

#[cfg(test)]
mod tests;
