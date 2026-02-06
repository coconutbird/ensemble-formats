//! UGX file reader.
//!
//! UGX files are stored inside ECF (Ensemble Common Format) containers.
//! The geometry data is split across multiple ECF chunks:
//! - Cached data chunk (0x700): Header, sections, bones, accessories
//! - Index buffer chunk (0x701): Triangle indices
//! - Vertex buffer chunk (0x702): Vertex data
//! - Material chunk (0x704): Material definitions
//!
//! The packed format uses 64-bit offsets (Definitive Edition is x64).
//! Packed arrays have: uint32 size, uint32 padding, uint64 offset.
//! Packed strings have: uint64 offset to null-terminated string.

use byteorder::{LittleEndian, ReadBytesExt};
use std::io::{Cursor, Read, Seek};

use crate::error::{Error, Result};
use crate::types::*;
use crate::univert_packer::{UnivertPacker, UnpackedVertex};
use crate::vertex_element::VertexElementType;

/// ECF chunk IDs for UGX.
const ECF_CACHED_DATA_CHUNK_ID: u64 = 0x00000700;
const ECF_IB_CHUNK_ID: u64 = 0x00000701;
const ECF_VB_CHUNK_ID: u64 = 0x00000702;
#[allow(dead_code)]
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

        // Parse cached data (pass the full slice for offset resolution)
        Self::parse_cached_data(&cached_data, vertex_buffer, index_buffer)
    }

    /// Parse the cached data chunk containing header, sections, bones, etc.
    fn parse_cached_data(
        data: &[u8],
        vertex_buffer: Vec<u8>,
        index_buffer: Vec<u16>,
    ) -> Result<Self> {
        let mut cursor = Cursor::new(data);

        // Read and verify header signature
        let signature = cursor.read_u32::<LittleEndian>()?;
        if signature != GEOM_HEADER_SIGNATURE {
            return Err(Error::InvalidSignature {
                expected: GEOM_HEADER_SIGNATURE,
                actual: signature,
            });
        }

        // Header fields (matches BHeader layout - 60 bytes total)
        let rigid_bone_index = cursor.read_i32::<LittleEndian>()?;

        // Bounding sphere center (3 floats) + radius
        let bounding_sphere = Sphere {
            center: [
                cursor.read_f32::<LittleEndian>()?,
                cursor.read_f32::<LittleEndian>()?,
                cursor.read_f32::<LittleEndian>()?,
            ],
            radius: cursor.read_f32::<LittleEndian>()?,
        };

        // AABB bounds (2 Vec3)
        let bounds = AABB {
            min: [
                cursor.read_f32::<LittleEndian>()?,
                cursor.read_f32::<LittleEndian>()?,
                cursor.read_f32::<LittleEndian>()?,
            ],
            max: [
                cursor.read_f32::<LittleEndian>()?,
                cursor.read_f32::<LittleEndian>()?,
                cursor.read_f32::<LittleEndian>()?,
            ],
        };

        // Instance data
        let _max_instances = cursor.read_i16::<LittleEndian>()?;
        let _instance_index_multiplier = cursor.read_i16::<LittleEndian>()?;
        let _large_geom_bone_index = cursor.read_i16::<LittleEndian>()?;

        // Flags (4 bools, 1 byte each)
        let all_sections_rigid = cursor.read_u8()? != 0;
        let global_bones = cursor.read_u8()? != 0;
        let all_sections_skinned = cursor.read_u8()? != 0;
        let rigid_only = cursor.read_u8()? != 0;

        // Padding to align to 8 bytes for 64-bit pointers
        // Header is 58 bytes (0x3A), need 6 bytes padding to reach 0x40
        let _padding = cursor.read_u16::<LittleEndian>()?; // 0x3A-0x3B
        let _padding2 = cursor.read_u32::<LittleEndian>()?; // 0x3C-0x3F

        // Now read packed arrays
        // Format for 64-bit: uint32 size, uint32 padding, uint64 offset

        // Sections array
        let sections = Self::read_packed_sections(data, &mut cursor)?;

        // Bones array
        let bones = Self::read_packed_bones(data, &mut cursor)?;

        // Accessories array (skip for now - complex format)
        let accessories_count = cursor.read_u32::<LittleEndian>()?;
        let _accessories_pad = cursor.read_u32::<LittleEndian>()?;
        let _accessories_offset = cursor.read_u64::<LittleEndian>()?;
        if accessories_count > 0 {
            // Skip accessories - they have a complex structure
        }

        // Valid accessories array (skip)
        let _valid_accessories_count = cursor.read_u32::<LittleEndian>()?;
        let _valid_accessories_pad = cursor.read_u32::<LittleEndian>()?;
        let _valid_accessories_offset = cursor.read_u64::<LittleEndian>()?;

        // Bone bounds low array
        let bone_bounds = Self::read_bone_bounds(data, &mut cursor)?;

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

    /// Read packed sections array from cached data.
    fn read_packed_sections(data: &[u8], cursor: &mut Cursor<&[u8]>) -> Result<Vec<Section>> {
        let count = cursor.read_u32::<LittleEndian>()? as usize;
        let _padding = cursor.read_u32::<LittleEndian>()?;
        let offset = cursor.read_u64::<LittleEndian>()? as usize;

        if count == 0 {
            return Ok(Vec::new());
        }

        // Seek to section data
        let mut section_cursor = Cursor::new(&data[offset..]);
        let mut sections = Vec::with_capacity(count);

        for _ in 0..count {
            sections.push(Self::read_packed_section(data, &mut section_cursor)?);
        }

        Ok(sections)
    }

    /// Read a single packed section.
    ///
    /// DE packed format for BSection is exactly 152 bytes (0x98):
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
    /// - +0x28: BoneRemap packed array (16 bytes: u32 size, u32 pad, u64 offset)
    /// - +0x38: UnivertPacker (84 bytes)
    /// - +0x8C: mRigidOnly (i32)
    /// - +0x90: mGlobalBones (i32) - DE-specific, not in 2008 source
    /// - +0x94: mPadding (i32)
    fn read_packed_section(data: &[u8], cursor: &mut Cursor<&[u8]>) -> Result<Section> {
        // +0x00: mMaterialIndex
        let material_index = cursor.read_i32::<LittleEndian>()?;
        // +0x04: mAccessoryIndex
        let accessory_index = cursor.read_i32::<LittleEndian>()?;
        // +0x08: mMaxBones (always present in DE format)
        let max_bones = cursor.read_i32::<LittleEndian>()?;
        // +0x0C: mRigidBoneIndex
        let rigid_bone_index = cursor.read_i32::<LittleEndian>()?;
        // +0x10: mIBOfs (in indices, not bytes!)
        let ib_offset = cursor.read_i32::<LittleEndian>()?;
        // +0x14: mNumTris
        let num_tris = cursor.read_i32::<LittleEndian>()?;
        // +0x18: mVBOfs
        let vb_offset = cursor.read_i32::<LittleEndian>()?;
        // +0x1C: mVBBytes
        let vb_bytes = cursor.read_i32::<LittleEndian>()?;
        // +0x20: mVertSize
        let vert_size = cursor.read_i32::<LittleEndian>()?;
        // +0x24: mNumVerts
        let num_verts = cursor.read_i32::<LittleEndian>()?;

        // +0x28: LocalToGlobalBoneRemap packed array (16 bytes)
        let _bone_remap_count = cursor.read_u32::<LittleEndian>()?;
        let _bone_remap_pad = cursor.read_u32::<LittleEndian>()?;
        let _bone_remap_offset = cursor.read_u64::<LittleEndian>()?;

        // +0x38: UnivertPacker (84 bytes)
        let base_vert_packer = Self::read_packed_univert_packer(data, cursor)?;

        // +0x8C: mRigidOnly (i32)
        let rigid_only = cursor.read_i32::<LittleEndian>()? != 0;

        // +0x90: mGlobalBones (i32) - DE-specific field
        let global_bones = cursor.read_i32::<LittleEndian>()? != 0;

        // +0x94: mPadding (i32)
        let _padding = cursor.read_i32::<LittleEndian>()?;

        Ok(Section {
            material_index,
            accessory_index,
            max_bones,
            rigid_bone_index,
            ib_offset,
            num_tris,
            vb_offset,
            vb_bytes,
            vert_size,
            num_verts,
            base_vert_packer,
            rigid_only,
            global_bones,
        })
    }

    /// Read packed UnivertPacker.
    fn read_packed_univert_packer(data: &[u8], cursor: &mut Cursor<&[u8]>) -> Result<UnivertPacker> {
        // Packed strings (uint64 offset each)
        let pack_order_offset = cursor.read_u64::<LittleEndian>()? as usize;
        let decl_order_offset = cursor.read_u64::<LittleEndian>()? as usize;

        // Read the pack order string
        let pack_order = if pack_order_offset == 0xFFFFFFFFFFFFFFFF || pack_order_offset >= data.len() {
            String::new()
        } else {
            Self::read_null_terminated_string(&data[pack_order_offset..])?
        };

        let decl_order = if decl_order_offset == 0xFFFFFFFFFFFFFFFF || decl_order_offset >= data.len() {
            String::new()
        } else {
            Self::read_null_terminated_string(&data[decl_order_offset..])?
        };

        // Vertex element types (each is uint32 for VertexElement::EType enum)
        let pos_type = VertexElementType::from_u32(cursor.read_u32::<LittleEndian>()?);
        let basis_type = VertexElementType::from_u32(cursor.read_u32::<LittleEndian>()?);
        let basis_scale_type = VertexElementType::from_u32(cursor.read_u32::<LittleEndian>()?);
        let tangent_type = VertexElementType::from_u32(cursor.read_u32::<LittleEndian>()?);
        let normal_type = VertexElementType::from_u32(cursor.read_u32::<LittleEndian>()?);

        // UV array (8 elements)
        let mut uv_types = [VertexElementType::Ignore; 8];
        for i in 0..8 {
            uv_types[i] = VertexElementType::from_u32(cursor.read_u32::<LittleEndian>()?);
        }

        let indices_type = VertexElementType::from_u32(cursor.read_u32::<LittleEndian>()?);
        let weights_type = VertexElementType::from_u32(cursor.read_u32::<LittleEndian>()?);
        let diffuse_type = VertexElementType::from_u32(cursor.read_u32::<LittleEndian>()?);
        let index_type = VertexElementType::from_u32(cursor.read_u32::<LittleEndian>()?);

        Ok(UnivertPacker {
            pack_order,
            decl_order,
            pos_type,
            basis_type,
            basis_scale_type,
            tangent_type,
            normal_type,
            uv_types,
            indices_type,
            weights_type,
            diffuse_type,
            index_type,
        })
    }

    /// Read null-terminated string from data.
    fn read_null_terminated_string(data: &[u8]) -> Result<String> {
        let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
        Ok(String::from_utf8(data[..end].to_vec())?)
    }

    /// Read packed bones array from cached data.
    fn read_packed_bones(data: &[u8], cursor: &mut Cursor<&[u8]>) -> Result<Vec<Bone>> {
        let count = cursor.read_u32::<LittleEndian>()? as usize;
        let _padding = cursor.read_u32::<LittleEndian>()?;
        let offset = cursor.read_u64::<LittleEndian>()? as usize;

        if count == 0 {
            return Ok(Vec::new());
        }

        // Seek to bone data
        let mut bone_cursor = Cursor::new(&data[offset..]);
        let mut bones = Vec::with_capacity(count);

        for _ in 0..count {
            bones.push(Self::read_packed_bone(data, &mut bone_cursor)?);
        }

        Ok(bones)
    }

    /// Read a single packed bone.
    fn read_packed_bone(data: &[u8], cursor: &mut Cursor<&[u8]>) -> Result<Bone> {
        // Packed string for name (uint64 offset)
        let name_offset = cursor.read_u64::<LittleEndian>()? as usize;
        let name = if name_offset == 0xFFFFFFFFFFFFFFFF || name_offset >= data.len() {
            String::new()
        } else {
            Self::read_null_terminated_string(&data[name_offset..])?
        };

        let parent_index = cursor.read_i32::<LittleEndian>()?;
        let _padding = cursor.read_u32::<LittleEndian>()?; // Alignment padding

        // QForm: quaternion (4 floats) + translation (3 floats)
        let model_to_bone = QForm {
            rotation: [
                cursor.read_f32::<LittleEndian>()?,
                cursor.read_f32::<LittleEndian>()?,
                cursor.read_f32::<LittleEndian>()?,
                cursor.read_f32::<LittleEndian>()?,
            ],
            translation: [
                cursor.read_f32::<LittleEndian>()?,
                cursor.read_f32::<LittleEndian>()?,
                cursor.read_f32::<LittleEndian>()?,
            ],
        };

        Ok(Bone {
            name,
            parent_index,
            model_to_bone,
        })
    }

    /// Read bone bounds (low and high arrays).
    fn read_bone_bounds(data: &[u8], cursor: &mut Cursor<&[u8]>) -> Result<Vec<AABB>> {
        // Bone bounds low array
        let low_count = cursor.read_u32::<LittleEndian>()? as usize;
        let _low_pad = cursor.read_u32::<LittleEndian>()?;
        let low_offset = cursor.read_u64::<LittleEndian>()? as usize;

        // Bone bounds high array
        let high_count = cursor.read_u32::<LittleEndian>()? as usize;
        let _high_pad = cursor.read_u32::<LittleEndian>()?;
        let high_offset = cursor.read_u64::<LittleEndian>()? as usize;

        if low_count == 0 || low_count != high_count {
            return Ok(Vec::new());
        }

        let mut bounds = Vec::with_capacity(low_count);

        for i in 0..low_count {
            let low_idx = low_offset + i * 12;
            let high_idx = high_offset + i * 12;

            if low_idx + 12 > data.len() || high_idx + 12 > data.len() {
                break;
            }

            let mut low_cursor = Cursor::new(&data[low_idx..]);
            let mut high_cursor = Cursor::new(&data[high_idx..]);

            bounds.push(AABB {
                min: [
                    low_cursor.read_f32::<LittleEndian>()?,
                    low_cursor.read_f32::<LittleEndian>()?,
                    low_cursor.read_f32::<LittleEndian>()?,
                ],
                max: [
                    high_cursor.read_f32::<LittleEndian>()?,
                    high_cursor.read_f32::<LittleEndian>()?,
                    high_cursor.read_f32::<LittleEndian>()?,
                ],
            });
        }

        Ok(bounds)
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
