//! UGX file reader.
//!
//! UGX files are stored inside ECF (Ensemble Common Format) containers.
//! The geometry data is split across multiple ECF chunks:
//! - Cached data chunk (0x700): Header, sections, bones, accessories
//! - Index buffer chunk (0x701): Triangle indices
//! - Vertex buffer chunk (0x702): Vertex data
//! - Material chunk (0x704): Material definitions

use byteorder::{LittleEndian, ReadBytesExt};
use std::io::{Cursor, Read, Seek, SeekFrom};

use crate::error::{Error, Result};
use crate::types::*;
use crate::univert_packer::UnpackedVertex;

/// ECF chunk IDs for UGX.
const ECF_CACHED_DATA_CHUNK_ID: u64 = 0x00000700;
const ECF_IB_CHUNK_ID: u64 = 0x00000701;
const ECF_VB_CHUNK_ID: u64 = 0x00000702;
const ECF_MATERIAL_CHUNK_ID: u64 = 0x00000704;

/// Geometry header signature.
const GEOM_HEADER_SIGNATURE: u32 = 0xC2340004;

/// Parsed UGX geometry data.
#[derive(Debug, Clone)]
pub struct UgxGeom {
    /// Bounding sphere.
    pub bounding_sphere: Sphere,
    /// Axis-aligned bounding box.
    pub bounds: AABB,
    /// Materials.
    pub materials: Vec<Material>,
    /// Bones.
    pub bones: Vec<Bone>,
    /// Per-bone bounding boxes.
    pub bone_bounds: Vec<AABB>,
    /// Mesh sections.
    pub sections: Vec<Section>,
    /// Raw vertex buffer.
    pub vertex_buffer: Vec<u8>,
    /// Raw index buffer (u16 indices).
    pub index_buffer: Vec<u16>,
    /// Is the entire mesh rigid (single bone)?
    pub rigid_only: bool,
    /// Rigid bone index (if rigid_only).
    pub rigid_bone_index: i32,
    /// Are all sections rigid (multi-bone rigid)?
    pub all_sections_rigid: bool,
    /// Are all sections skinned?
    pub all_sections_skinned: bool,
    /// Use global bones?
    pub global_bones: bool,
}

impl UgxGeom {
    /// Read UGX geometry from raw bytes (ECF container).
    pub fn read(data: &[u8]) -> Result<Self> {
        let mut cursor = Cursor::new(data);
        Self::read_from(&mut cursor)
    }

    /// Read UGX geometry from a reader (ECF container).
    pub fn read_from<R: Read + Seek>(reader: &mut R) -> Result<Self> {
        // Parse ECF container
        let mut ecf = ecf::EcfReader::new(reader)?;

        // Read cached data chunk (header, sections, bones, etc.)
        let cached_data = ecf
            .read_chunk_data_by_id(ECF_CACHED_DATA_CHUNK_ID)
            .map_err(|_| Error::MissingChunk("cached_data (0x700)"))?;

        // Read vertex buffer chunk
        let vertex_buffer = ecf
            .read_chunk_data_by_id(ECF_VB_CHUNK_ID)
            .map_err(|_| Error::MissingChunk("vertex_buffer (0x702)"))?;

        // Read index buffer chunk
        let ib_data = ecf
            .read_chunk_data_by_id(ECF_IB_CHUNK_ID)
            .map_err(|_| Error::MissingChunk("index_buffer (0x701)"))?;

        // Convert index buffer from bytes to u16
        let mut ib_cursor = Cursor::new(&ib_data);
        let num_indices = ib_data.len() / 2;
        let mut index_buffer = Vec::with_capacity(num_indices);
        for _ in 0..num_indices {
            index_buffer.push(ib_cursor.read_u16::<LittleEndian>()?);
        }

        // Parse cached data
        let mut cursor = Cursor::new(&cached_data);
        Self::parse_cached_data(&mut cursor, vertex_buffer, index_buffer)
    }

    /// Parse the cached data chunk containing header, sections, bones, etc.
    fn parse_cached_data<R: Read>(
        reader: &mut R,
        vertex_buffer: Vec<u8>,
        index_buffer: Vec<u16>,
    ) -> Result<Self> {
        // Read and verify header signature
        let signature = reader.read_u32::<LittleEndian>()?;
        if signature != GEOM_HEADER_SIGNATURE {
            return Err(Error::InvalidSignature {
                expected: GEOM_HEADER_SIGNATURE,
                actual: signature,
            });
        }

        // Header fields (matches BHeader layout)
        let rigid_bone_index = reader.read_i32::<LittleEndian>()?;

        // Bounding sphere center (3 floats)
        let sphere_x = reader.read_f32::<LittleEndian>()?;
        let sphere_y = reader.read_f32::<LittleEndian>()?;
        let sphere_z = reader.read_f32::<LittleEndian>()?;
        let sphere_radius = reader.read_f32::<LittleEndian>()?;
        let bounding_sphere = Sphere {
            center: [sphere_x, sphere_y, sphere_z],
            radius: sphere_radius,
        };

        // AABB bounds (2 Vec3)
        let min_x = reader.read_f32::<LittleEndian>()?;
        let min_y = reader.read_f32::<LittleEndian>()?;
        let min_z = reader.read_f32::<LittleEndian>()?;
        let max_x = reader.read_f32::<LittleEndian>()?;
        let max_y = reader.read_f32::<LittleEndian>()?;
        let max_z = reader.read_f32::<LittleEndian>()?;
        let bounds = AABB {
            min: [min_x, min_y, min_z],
            max: [max_x, max_y, max_z],
        };

        // Instance data
        let _max_instances = reader.read_i16::<LittleEndian>()?;
        let _instance_index_multiplier = reader.read_i16::<LittleEndian>()?;
        let _large_geom_bone_index = reader.read_i16::<LittleEndian>()?;

        // Flags (4 bools)
        let all_sections_rigid = reader.read_u8()? != 0;
        let global_bones = reader.read_u8()? != 0;
        let all_sections_skinned = reader.read_u8()? != 0;
        let rigid_only = reader.read_u8()? != 0;

        // Read packed arrays using the packed array format
        // Sections array
        let sections = Self::read_packed_section_array(reader)?;

        // Bones array
        let bones = Self::read_packed_bone_array(reader)?;

        // Accessories array (skip for now)
        let _num_accessories = reader.read_u32::<LittleEndian>()?;
        // Accessories have complex structure, skip reading their data

        // Valid accessories array (skip)
        let _num_valid_accessories = reader.read_u32::<LittleEndian>()?;

        // Bone bounds low array
        let num_bone_bounds = reader.read_u32::<LittleEndian>()? as usize;
        let mut bone_bounds_low = Vec::with_capacity(num_bone_bounds);
        for _ in 0..num_bone_bounds {
            let x = reader.read_f32::<LittleEndian>()?;
            let y = reader.read_f32::<LittleEndian>()?;
            let z = reader.read_f32::<LittleEndian>()?;
            bone_bounds_low.push([x, y, z]);
        }

        // Bone bounds high array
        let num_bone_bounds_high = reader.read_u32::<LittleEndian>()? as usize;
        let mut bone_bounds_high = Vec::with_capacity(num_bone_bounds_high);
        for _ in 0..num_bone_bounds_high {
            let x = reader.read_f32::<LittleEndian>()?;
            let y = reader.read_f32::<LittleEndian>()?;
            let z = reader.read_f32::<LittleEndian>()?;
            bone_bounds_high.push([x, y, z]);
        }

        // Combine low/high into AABB
        let bone_bounds: Vec<AABB> = bone_bounds_low
            .iter()
            .zip(bone_bounds_high.iter())
            .map(|(low, high)| AABB {
                min: *low,
                max: *high,
            })
            .collect();

        // Materials are in a separate chunk - leave empty for now
        let materials = Vec::new();

        Ok(Self {
            bounding_sphere,
            bounds,
            materials,
            bones,
            bone_bounds,
            sections,
            vertex_buffer,
            index_buffer,
            rigid_only,
            rigid_bone_index,
            all_sections_rigid,
            all_sections_skinned,
            global_bones,
        })
    }

    /// Read packed section array.
    fn read_packed_section_array<R: Read>(reader: &mut R) -> Result<Vec<Section>> {
        let count = reader.read_u32::<LittleEndian>()? as usize;
        let mut sections = Vec::with_capacity(count);

        for _ in 0..count {
            sections.push(Section::read_packed(reader)?);
        }

        Ok(sections)
    }

    /// Read packed bone array.
    fn read_packed_bone_array<R: Read>(reader: &mut R) -> Result<Vec<Bone>> {
        let count = reader.read_u32::<LittleEndian>()? as usize;
        let mut bones = Vec::with_capacity(count);

        for _ in 0..count {
            bones.push(Bone::read_packed(reader)?);
        }

        Ok(bones)
    }

    /// Get unpacked vertices for a section.
    pub fn unpack_section_vertices(&self, section_idx: usize) -> Result<Vec<UnpackedVertex>> {
        let section = &self.sections[section_idx];
        let packer = &section.base_vert_packer;

        let vb_start = section.vb_offset as usize;
        let vb_end = vb_start + section.vb_bytes as usize;
        let vb_slice = &self.vertex_buffer[vb_start..vb_end];

        let mut cursor = Cursor::new(vb_slice);
        let mut vertices = Vec::with_capacity(section.num_verts as usize);

        for _ in 0..section.num_verts {
            vertices.push(packer.unpack_vertex(&mut cursor)?);
        }

        Ok(vertices)
    }

    /// Get indices for a section.
    pub fn get_section_indices(&self, section_idx: usize) -> Vec<u16> {
        let section = &self.sections[section_idx];
        let start = section.ib_offset as usize;
        let count = section.num_tris as usize * 3;
        self.index_buffer[start..start + count].to_vec()
    }

    /// Get total vertex count across all sections.
    pub fn total_vertices(&self) -> usize {
        self.sections.iter().map(|s| s.num_verts as usize).sum()
    }

    /// Get total triangle count across all sections.
    pub fn total_triangles(&self) -> usize {
        self.sections.iter().map(|s| s.num_tris as usize).sum()
    }
}
