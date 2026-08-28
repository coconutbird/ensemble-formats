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
//! │ Chunk 0x703: Granny Data - Bone inverse world matrices (required)   │
//! │ Chunk 0x704: Materials - BBinaryDataTree document (required)        │
//! │ Chunk 0x705: AABB Tree - Spatial acceleration structure (optional)  │
//!
//! The AABB tree is a streamed format (not flat binary) with variable-length
//! triangle index arrays per node. See `reader/aabb_tree.rs` for details.
//! └─────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Packed Data Format (Definitive Edition x64)
//!
//! The `BCachedData` chunk uses a "packed" format where pointers are stored as
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
//! ## `BCachedData` Layout (Chunk 0x700)
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
//! ## `BSection` Layout (152 bytes on-disk)
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
//! +0x90: int32 globalBones (HW1-specific, not in 2008 source!)
//! +0x94: int32 padding
//! ```
//!
//! ## `UnivertPacker` Layout (84 bytes on-disk)
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
mod options;

pub use options::ReadOptions;

use alloc::string::String;
use alloc::vec::Vec;
use zerocopy::Ref;

use nostdio::{Cursor, ReadLe};

use crate::constants::{
    ECF_AABB_TREE_CHUNK_ID, ECF_CACHED_DATA_CHUNK_ID, ECF_GRANNY_CHUNK_ID, ECF_IB_CHUNK_ID,
    ECF_MATERIAL_CHUNK_ID, ECF_VB_CHUNK_ID, GEOM_HEADER_SIGNATURE_HW1, GEOM_HEADER_SIGNATURE_HW2,
    UGX_FILE_ID,
};
use crate::error::{Error, Result};
use crate::types::raw::GeomHeaderRaw;
use crate::types::{AABB, Material, Sphere, UgxGeom};

use crate::types::UgxVersion;
use cached_data::{
    read_bone_bounds, read_packed_accessories, read_packed_bones, read_packed_sections,
    read_valid_accessory_indices,
};
use granny::{parse_granny_bones, parse_granny_meshes, validate_granny_chunk};
use material::read_materials as parse_materials;

impl UgxGeom {
    /// Parse UGX geometry from a byte slice (ECF container).
    ///
    /// # Errors
    ///
    /// Returns an error if the ECF file ID, cached-data signature, checksums,
    /// engine-required chunks, or any encoded UGX structure is invalid.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        Self::from_bytes_with_options(data, ReadOptions::strict())
    }

    /// Parse UGX geometry from a byte slice, skipping ECF checksum validation.
    ///
    /// # Errors
    ///
    /// Returns an error if the ECF container or any required UGX chunk is
    /// invalid, missing, or truncated.
    pub fn from_bytes_unchecked(data: &[u8]) -> Result<Self> {
        Self::from_bytes_with_options(data, ReadOptions::unchecked_checksums())
    }

    /// Parse UGX geometry with explicit validation controls.
    ///
    /// # Errors
    ///
    /// Returns an error if enabled validation fails, a required structure is
    /// missing, or any encoded count, offset, or value is malformed.
    pub fn from_bytes_with_options(data: &[u8], options: ReadOptions) -> Result<Self> {
        let ecf = if options.validate_checksums {
            ecf::Reader::new(data)?
        } else {
            ecf::Reader::new_unchecked(data)?
        };

        if options.validate_signatures && ecf.header().id != UGX_FILE_ID {
            return Err(Error::InvalidFileId {
                expected: UGX_FILE_ID,
                actual: ecf.header().id,
            });
        }

        let cached_data = ecf
            .chunk_data_by_id(ECF_CACHED_DATA_CHUNK_ID)
            .map_err(|_| Error::MissingChunk("cached_data (0x700)"))?;

        let vertex_buffer = ecf
            .chunk_data_by_id(ECF_VB_CHUNK_ID)
            .map_err(|_| Error::MissingChunk("vertex_buffer (0x702)"))?;

        let ib_data = ecf
            .chunk_data_by_id(ECF_IB_CHUNK_ID)
            .map_err(|_| Error::MissingChunk("index_buffer (0x701)"))?;

        let granny_data = read_optional_chunk(&ecf, ECF_GRANNY_CHUNK_ID)?;
        let material_data = read_optional_chunk(&ecf, ECF_MATERIAL_CHUNK_ID)?;
        let aabb_tree_raw = read_optional_chunk(&ecf, ECF_AABB_TREE_CHUNK_ID)?;

        if options.validate_engine_requirements {
            if granny_data.is_none() {
                return Err(Error::MissingChunk("granny_data (0x703)"));
            }
            if material_data.is_none() {
                return Err(Error::MissingChunk("materials (0x704)"));
            }
        }

        if ib_data.len() % core::mem::size_of::<u16>() != 0 {
            return Err(Error::UnsupportedFormat(String::from(
                "index-buffer chunk has a trailing partial index",
            )));
        }

        let num_indices = ib_data.len() / 2;
        let mut index_buffer = Vec::with_capacity(num_indices);
        let mut ib_cur = Cursor::new(&ib_data);
        for _ in 0..num_indices {
            index_buffer.push(ib_cur.read_u16_le()?);
        }

        Self::parse_cached_data(
            &cached_data,
            granny_data.as_deref(),
            material_data.as_deref(),
            aabb_tree_raw.as_deref(),
            vertex_buffer,
            index_buffer,
            options,
        )
    }

    /// Parse the cached data chunk (0x700) containing header, sections, bones, etc.
    fn parse_cached_data(
        data: &[u8],
        granny_data: Option<&[u8]>,
        material_data: Option<&[u8]>,
        aabb_tree_raw: Option<&[u8]>,
        vertex_buffer: Vec<u8>,
        index_buffer: Vec<u16>,
        options: ReadOptions,
    ) -> Result<Self> {
        let (hdr, rest): (Ref<_, GeomHeaderRaw>, _) =
            Ref::from_prefix(data).map_err(|_| Error::UnexpectedEof {
                context: String::from("GeomHeaderRaw"),
            })?;

        let signature = u32::from_le_bytes(hdr.signature);
        let version = read_version(signature, options)?;

        let rigid_bone_index = i32::from_le_bytes(hdr.rigid_bone_index);

        let bounding_sphere = Sphere::from(&*hdr);
        let bounds = AABB::from(&*hdr);

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
        if options.validate_engine_requirements
            && let Some(granny) = granny_data
        {
            validate_granny_chunk(granny)?;
        }

        let (granny_bones, skeleton_lod_type) = if let Some(granny) = granny_data {
            parse_granny_bones(granny)?
        } else {
            (Vec::new(), 0)
        };

        let granny_meshes = if let Some(granny) = granny_data {
            parse_granny_meshes(granny)?
        } else {
            Vec::new()
        };

        let accessories = read_packed_accessories(data, pos)?;
        // IDA: BUGXGeomData::readCachedData uses BPackedArray_Simple__unpack for
        // validAccessories (v6+28) in BOTH HW1 and HW2 — they are i32 indices into
        // the accessories array, not full 24-byte AccessoryRaw structs.
        let valid_accessories = read_valid_accessory_indices(data, pos)?;

        let bone_bounds = read_bone_bounds(data, pos)?;

        let materials = if let Some(mat_data) = material_data {
            parse_materials(mat_data)?
        } else {
            Vec::new()
        };

        let aabb_tree = if let Some(tree_data) = aabb_tree_raw {
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
            vertex_buffer,
            index_buffer,
            accessories,
            valid_accessories,
            rigid_only,
            rigid_bone_index,
            max_instances,
            instance_index_multiplier,
            large_geom_bone_index,
            flags: crate::GeometryFlags {
                all_sections_rigid,
                all_sections_skinned,
                global_bones,
            },
            aabb_tree,
        })
    }
}

/// Read only the materials from a UGX file, skipping geometry, bones, etc.
///
/// Opens the ECF container, extracts chunk 0x704 (materials), and parses
/// the `BBinaryDataTree` document. This is much cheaper than a full
/// [`UgxGeom::from_bytes`] parse when you only need texture/material info.
///
/// # Errors
///
/// Returns an error if the ECF container, UGX file ID, or material chunk is
/// invalid or if the material chunk is missing.
pub fn read_materials(data: &[u8]) -> Result<Vec<Material>> {
    read_materials_with_options(data, ReadOptions::strict())
}

/// Read only the materials with explicit UGX validation controls.
///
/// # Errors
///
/// Returns an error if enabled ECF validation fails or the material document
/// is malformed. Strict mode also rejects a missing material chunk.
pub fn read_materials_with_options(data: &[u8], options: ReadOptions) -> Result<Vec<Material>> {
    let ecf = if options.validate_checksums {
        ecf::Reader::new(data)?
    } else {
        ecf::Reader::new_unchecked(data)?
    };
    if options.validate_signatures && ecf.header().id != UGX_FILE_ID {
        return Err(Error::InvalidFileId {
            expected: UGX_FILE_ID,
            actual: ecf.header().id,
        });
    }
    match read_optional_chunk(&ecf, ECF_MATERIAL_CHUNK_ID)? {
        Some(mat_data) => parse_materials(&mat_data),
        None if options.validate_engine_requirements => {
            Err(Error::MissingChunk("materials (0x704)"))
        }
        None => Ok(Vec::new()),
    }
}

/// UGX file reader.
pub struct Reader;

impl Reader {
    /// Read a UGX file from a byte slice.
    ///
    /// # Errors
    ///
    /// Returns an error if the ECF container or any required UGX chunk is
    /// invalid, missing, truncated, or fails checksum validation.
    pub fn read(data: &[u8]) -> Result<UgxGeom> {
        UgxGeom::from_bytes(data)
    }

    /// Read a UGX file with explicit validation controls.
    ///
    /// # Errors
    ///
    /// Returns an error if enabled validation fails or the file is malformed.
    pub fn read_with_options(data: &[u8], options: ReadOptions) -> Result<UgxGeom> {
        UgxGeom::from_bytes_with_options(data, options)
    }
}

fn read_optional_chunk(ecf: &ecf::Reader<'_>, id: u64) -> Result<Option<Vec<u8>>> {
    if ecf.find_chunk(id).is_some() {
        ecf.chunk_data_by_id(id).map(Some).map_err(Error::from)
    } else {
        Ok(None)
    }
}

fn read_version(signature: u32, options: ReadOptions) -> Result<UgxVersion> {
    if !options.validate_signatures
        && let Some(version) = options.version_hint
    {
        return Ok(version);
    }

    match signature {
        GEOM_HEADER_SIGNATURE_HW1 => Ok(UgxVersion::Hw1),
        GEOM_HEADER_SIGNATURE_HW2 => Ok(UgxVersion::Hw2),
        _ if options.validate_signatures => Err(Error::InvalidSignature { actual: signature }),
        _ => Err(Error::MissingVersionHint { actual: signature }),
    }
}

#[cfg(test)]
mod tests;
