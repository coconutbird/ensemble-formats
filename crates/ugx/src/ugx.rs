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
const ECF_GRANNY_CHUNK_ID: u64 = 0x00000703;
const ECF_MATERIAL_CHUNK_ID: u64 = 0x00000704;

/// Geometry header signatures (version 4 = original, version 6 = Definitive Edition).
const GEOM_HEADER_SIGNATURE_V4: u32 = 0xC2340004;
const GEOM_HEADER_SIGNATURE_V6: u32 = 0xC2340006;

/// Parsed UGX geometry data.
#[derive(Debug, Clone)]
pub struct UgxGeom {
    /// Bounding sphere.
    pub bounding_sphere: Sphere,
    /// Axis-aligned bounding box.
    pub bounds: AABB,
    /// Materials.
    pub materials: Vec<Material>,
    /// Bones (from cached data chunk 0x700).
    pub bones: Vec<Bone>,
    /// Granny bone data (from granny chunk 0x703) - contains inverse world matrices.
    pub granny_bones: Vec<GrannyBone>,
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

/// Bone data from granny chunk (0x703).
/// This has the correct inverse world matrix for positioning bones.
#[derive(Debug, Clone, Default)]
pub struct GrannyBone {
    /// Bone name.
    pub name: String,
    /// Parent bone index (-1 for root).
    pub parent_index: i32,
    /// Inverse world matrix (4x4) - read from offset 80 in granny bone struct.
    /// To get world matrix: invert then transpose this matrix.
    pub inverse_world_matrix: Matrix4x4,
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

        // Read granny chunk (optional - contains bone inverse world matrices)
        let granny_data = ecf.read_chunk_data_by_id(ECF_GRANNY_CHUNK_ID).ok();

        // Read material chunk (optional - BBinaryDataTree packed document)
        let material_data = ecf.read_chunk_data_by_id(ECF_MATERIAL_CHUNK_ID).ok();

        // Convert index buffer from bytes to u16
        let mut ib_cursor = Cursor::new(&ib_data);
        let num_indices = ib_data.len() / 2;
        let mut index_buffer = Vec::with_capacity(num_indices);
        for _ in 0..num_indices {
            index_buffer.push(ib_cursor.read_u16::<LittleEndian>()?);
        }

        // Parse cached data (pass the full slice for offset resolution)
        Self::parse_cached_data(&cached_data, granny_data, material_data, vertex_buffer, index_buffer)
    }

    /// Parse the cached data chunk containing header, sections, bones, etc.
    fn parse_cached_data(
        data: &[u8],
        granny_data: Option<Vec<u8>>,
        material_data: Option<Vec<u8>>,
        vertex_buffer: Vec<u8>,
        index_buffer: Vec<u16>,
    ) -> Result<Self> {
        let mut cursor = Cursor::new(data);

        // Read and verify header signature (accept both v4 and v6)
        let signature = cursor.read_u32::<LittleEndian>()?;
        if signature != GEOM_HEADER_SIGNATURE_V4 && signature != GEOM_HEADER_SIGNATURE_V6 {
            return Err(Error::InvalidSignature {
                expected: GEOM_HEADER_SIGNATURE_V4,
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

        // Bones array (from cached data)
        let bones = Self::read_packed_bones(data, &mut cursor)?;

        // Parse granny bones if available (these have the correct inverse world matrices)
        let granny_bones = if let Some(ref granny) = granny_data {
            Self::parse_granny_bones(granny)?
        } else {
            Vec::new()
        };

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

        // Read materials from BBinaryDataTree packed document (chunk 0x704)
        // Gracefully handle parse failures - some UGX files may have invalid/empty material chunks
        let materials = if let Some(ref mat_data) = material_data {
            Self::read_materials(mat_data).unwrap_or_default()
        } else {
            Vec::new()
        };

        Ok(Self {
            bounding_sphere,
            bounds,
            materials,
            bones,
            granny_bones,
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

    /// Read a single packed bone (80 bytes).
    ///
    /// Bone layout:
    /// - +0x00: mName (uint64 offset to null-terminated string)
    /// - +0x08: mModelToBone (4x4 matrix = 64 bytes)
    /// - +0x48: mParentIndex (int32)
    /// - +0x4C: padding (4 bytes)
    fn read_packed_bone(data: &[u8], cursor: &mut Cursor<&[u8]>) -> Result<Bone> {
        // +0x00: Packed string for name (uint64 offset)
        let name_offset = cursor.read_u64::<LittleEndian>()? as usize;
        let name = if name_offset == 0xFFFFFFFFFFFFFFFF || name_offset >= data.len() {
            String::new()
        } else {
            Self::read_null_terminated_string(&data[name_offset..])?
        };

        // +0x08: Transform matrix (4x4 floats = 64 bytes)
        let mut rows = [[0.0f32; 4]; 4];
        for row in &mut rows {
            for col in row {
                *col = cursor.read_f32::<LittleEndian>()?;
            }
        }
        let model_to_bone = Matrix4x4 { rows };

        // +0x48: Parent index (int32)
        let parent_index = cursor.read_i32::<LittleEndian>()?;

        // +0x4C: Padding (4 bytes)
        let _padding = cursor.read_u32::<LittleEndian>()?;

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

    /// Read materials from BBinaryDataTree packed document (chunk 0x704).
    ///
    /// The root node's children are individual material nodes. Each material
    /// has a "Name" attribute, map type children (Diffuse, Normal, etc.),
    /// UVW velocity children, and a Properties child (BNameValueMap).
    fn read_materials(data: &[u8]) -> Result<Vec<Material>> {
        let root = match bdt::PackedReader::read_le(data)? {
            Some(root) => root,
            None => return Ok(Vec::new()),
        };

        let mut materials = Vec::with_capacity(root.children.len());
        for child in &root.children {
            materials.push(Self::read_material(child));
        }

        Ok(materials)
    }

    /// Read a single material from a BBinaryDataTree node.
    ///
    /// Tree structure:
    /// ```text
    /// <Material>
    ///   @Name="material_name"
    ///   @Ver=4
    ///   <NameValues>
    ///     <SpecPower> text=Float(...)
    ///     <Flags> text=UInt(...)
    ///     <BlendType> text=UInt(...)
    ///     <Opacity> text=UInt(0-255)
    ///     ...
    ///   <Maps>
    ///     <diffuse>
    ///       @UVWVel=Float(0.0)
    ///       <Map> @Name="texture_path" @Channel=Int(0) @Flags=UInt(7)
    ///     <normal>
    ///       @UVWVel=Float(0.0)
    ///     ...
    /// ```
    fn read_material(node: &bdt::Node) -> Material {
        let mut mat = Material::default();

        // Name from attribute
        if let Some(attr) = node.get_attribute("Name") {
            mat.name = attr.value.to_string_value();
        }

        // Read properties from "NameValues" child
        if let Some(nv_node) = node.children.iter().find(|c| c.name == "NameValues") {
            for prop in &nv_node.children {
                match prop.name.as_str() {
                    "SpecPower" => mat.spec_power = variant_to_f32(&prop.text),
                    "Flags" => mat.flags = variant_to_u32(&prop.text),
                    "BlendType" => mat.blend_type = variant_to_u8(&prop.text),
                    "Opacity" => {
                        // Opacity is stored as UInt 0-255, convert to 0.0-1.0
                        let raw = variant_to_u32(&prop.text);
                        mat.opacity = raw as f32 / 255.0;
                    }
                    _ => {}
                }
            }
        }

        // Read maps from "Maps" child
        if let Some(maps_node) = node.children.iter().find(|c| c.name == "Maps") {
            for map_type in MapType::ALL {
                if let Some(type_node) = maps_node.children.iter().find(|c| c.name == map_type.name()) {
                    // UVWVel is an attribute on the map type node
                    if let Some(uvw_attr) = type_node.get_attribute("UVWVel") {
                        mat.uvw_velocity[map_type as usize][0] = variant_to_f32(&uvw_attr.value);
                    }

                    // Each <Map> child is a texture reference
                    for map_child in &type_node.children {
                        if map_child.name == "Map" {
                            let mut map = Map::default();
                            if let Some(a) = map_child.get_attribute("Name") {
                                map.name = a.value.to_string_value();
                            }
                            if let Some(a) = map_child.get_attribute("Channel") {
                                map.channel = variant_to_i16(&a.value);
                            }
                            if let Some(a) = map_child.get_attribute("Flags") {
                                map.flags = variant_to_u8(&a.value);
                            }
                            mat.maps[map_type as usize].push(map);
                        }
                    }
                }
            }
        }

        mat
    }

    /// Parse granny bones from granny chunk (0x703).
    ///
    /// Granny skeleton format (from Python reference):
    /// - Skeleton offset at granny[52:60] (uint64)
    /// - At skelOffs + 24: bonesLen (uint32), then at skelOffs + 28: bonesOffs (uint64)
    /// - Each bone is 164 bytes:
    ///   - +0x00: nameOffs (uint64)
    ///   - +0x08: parent (int32)
    ///   - +0x50 (80): InverseWorld4x4 matrix (16 floats = 64 bytes)
    fn parse_granny_bones(granny: &[u8]) -> Result<Vec<GrannyBone>> {
        if granny.len() < 60 {
            return Ok(Vec::new());
        }

        // Read skeleton offset from granny[52:60]
        let mut cursor = Cursor::new(&granny[52..60]);
        let skel_offs = cursor.read_u64::<LittleEndian>()? as usize;

        if skel_offs + 36 > granny.len() {
            return Ok(Vec::new());
        }

        // Read bonesLen and bonesOffs from skelOffs + 24
        let mut cursor = Cursor::new(&granny[skel_offs + 24..skel_offs + 36]);
        let bones_len = cursor.read_u32::<LittleEndian>()? as usize;
        let bones_offs = cursor.read_u64::<LittleEndian>()? as usize;

        if bones_len == 0 {
            return Ok(Vec::new());
        }

        const GRANNY_BONE_SIZE: usize = 164;
        let mut bones = Vec::with_capacity(bones_len);

        for i in 0..bones_len {
            let bone_start = bones_offs + (i * GRANNY_BONE_SIZE);
            if bone_start + GRANNY_BONE_SIZE > granny.len() {
                break;
            }

            // +0x00: nameOffs (uint64)
            let mut cursor = Cursor::new(&granny[bone_start..bone_start + 12]);
            let name_offs = cursor.read_u64::<LittleEndian>()? as usize;
            // +0x08: parent (int32)
            let parent_index = cursor.read_i32::<LittleEndian>()?;

            // Read name from offset
            let name = if name_offs < granny.len() {
                Self::read_null_terminated_string(&granny[name_offs..])?
            } else {
                String::new()
            };

            // +0x50 (80): InverseWorld4x4 matrix (16 floats)
            let matrix_start = bone_start + 80;
            if matrix_start + 64 > granny.len() {
                break;
            }
            let mut cursor = Cursor::new(&granny[matrix_start..matrix_start + 64]);

            // Read 16 floats as row-major matrix (matches Python: matUnpack[0:4], [4:8], [8:12], [12:16])
            let mut rows = [[0.0f32; 4]; 4];
            for row in &mut rows {
                for col in row {
                    *col = cursor.read_f32::<LittleEndian>()?;
                }
            }
            let inverse_world_matrix = Matrix4x4 { rows };

            bones.push(GrannyBone {
                name,
                parent_index,
                inverse_world_matrix,
            });
        }

        Ok(bones)
    }
}

// ============================================================================
// Variant conversion helpers
// ============================================================================

fn variant_to_f32(v: &bdt::Variant) -> f32 {
    match v {
        bdt::Variant::Float(f) => *f,
        bdt::Variant::Double(d) => *d as f32,
        bdt::Variant::Int(i) => *i as f32,
        bdt::Variant::UInt(u) => *u as f32,
        _ => 0.0,
    }
}

fn variant_to_u32(v: &bdt::Variant) -> u32 {
    match v {
        bdt::Variant::UInt(u) => *u,
        bdt::Variant::Int(i) => *i as u32,
        _ => 0,
    }
}

fn variant_to_i16(v: &bdt::Variant) -> i16 {
    match v {
        bdt::Variant::Int(i) => *i as i16,
        bdt::Variant::UInt(u) => *u as i16,
        _ => 0,
    }
}

fn variant_to_u8(v: &bdt::Variant) -> u8 {
    match v {
        bdt::Variant::UInt(u) => *u as u8,
        bdt::Variant::Int(i) => *i as u8,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f16_to_f32(bits: u16) -> f32 {
        half::f16::from_bits(bits).to_f32()
    }

    #[test]
    fn dump_v6_granny_chunk() {
        let path = "../../foxcannon01/mesh_turret_0.ugx";
        let data = match std::fs::read(path) {
            Ok(d) => d,
            Err(_) => { eprintln!("File not found, skipping"); return; }
        };

        let mut cursor = Cursor::new(&data);
        let mut ecf = ecf::EcfReader::new(&mut cursor).unwrap();
        let granny = ecf.read_chunk_data_by_id(ECF_GRANNY_CHUNK_ID).unwrap();

        // IB analysis
        let ib = ecf.read_chunk_data_by_id(ECF_IB_CHUNK_ID).unwrap();
        let ib_u16: Vec<u16> = ib.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        let num_unique = *ib_u16.iter().max().unwrap_or(&0) as usize + 1;
        eprintln!("IB: {} indices, {} tris, {} unique verts", ib_u16.len(), ib_u16.len() / 3, num_unique);
        eprintln!("Granny: {} bytes\n", granny.len());

        // Dump the ENTIRE granny chunk in annotated hex, looking for data regions
        // Key known offsets:
        // 0x00-0x5F: File info header
        // 0xA0-0x28C: Skeleton (3 bones × 164 bytes)
        // 0x2D0-0x2DF: Mesh ptr array (1 mesh)
        // 0x2E0-0x3xx: Mesh struct
        // Strings near end (0x2510+)

        // Find big gaps of non-pointer, non-zero data
        // Dump everything from 0x380 to 0x2500 as hex in 24-byte rows (candidate vertex stride)
        eprintln!("Data from 0x380 to 0x600 (hex, 24-byte rows):");
        let start = 0x380usize;
        let end = 0x600usize.min(granny.len());
        for i in (start..end).step_by(24) {
            let row_end = (i + 24).min(end);
            let row = &granny[i..row_end];
            let hex: Vec<String> = row.iter().map(|b| format!("{:02X}", b)).collect();

            // Try reading as f16×4 + f32×3 + f16×2 (=24 bytes: pos + normal + uv)
            let mut annotation = String::new();
            if row.len() >= 24 {
                let hx = f16_to_f32(u16::from_le_bytes([row[0], row[1]]));
                let hy = f16_to_f32(u16::from_le_bytes([row[2], row[3]]));
                let hz = f16_to_f32(u16::from_le_bytes([row[4], row[5]]));
                let hw = f16_to_f32(u16::from_le_bytes([row[6], row[7]]));
                if hx.is_finite() && hy.is_finite() && hz.is_finite() && hx.abs() < 100.0 && hy.abs() < 100.0 && hz.abs() < 100.0 {
                    annotation += &format!("f16: ({:.3},{:.3},{:.3},{:.3}) ", hx, hy, hz, hw);
                }
                let fx = f32::from_le_bytes([row[0], row[1], row[2], row[3]]);
                let fy = f32::from_le_bytes([row[4], row[5], row[6], row[7]]);
                let fz = f32::from_le_bytes([row[8], row[9], row[10], row[11]]);
                if fx.is_finite() && fy.is_finite() && fz.is_finite() && fx.abs() < 100.0 && fy.abs() < 100.0 && fz.abs() < 100.0 {
                    annotation += &format!("f32: ({:.3},{:.3},{:.3}) ", fx, fy, fz);
                }
            }
            eprintln!("  {:04X}: {} {}", i, hex.join(" "), annotation);
        }

        // Also scan with NO bounding box constraint - just find runs of "reasonable" f32s
        eprintln!("\nScanning granny for runs of reasonable f32 triples (any range < 100)...");
        for stride in [12usize, 24, 32, 36, 44, 48] {
            let mut offset = 0x100;
            while offset + 12 <= granny.len() {
                let x = f32::from_le_bytes(granny[offset..offset+4].try_into().unwrap());
                let y = f32::from_le_bytes(granny[offset+4..offset+8].try_into().unwrap());
                let z = f32::from_le_bytes(granny[offset+8..offset+12].try_into().unwrap());

                if x.is_finite() && y.is_finite() && z.is_finite()
                    && x.abs() < 100.0 && y.abs() < 100.0 && z.abs() < 100.0
                    && (x.abs() > 0.01 || y.abs() > 0.01 || z.abs() > 0.01) {
                    let mut count = 1;
                    while offset + count * stride + 12 <= granny.len() {
                        let nx = f32::from_le_bytes(granny[offset+count*stride..offset+count*stride+4].try_into().unwrap());
                        let ny = f32::from_le_bytes(granny[offset+count*stride+4..offset+count*stride+8].try_into().unwrap());
                        let nz = f32::from_le_bytes(granny[offset+count*stride+8..offset+count*stride+12].try_into().unwrap());
                        if nx.is_finite() && ny.is_finite() && nz.is_finite()
                            && nx.abs() < 100.0 && ny.abs() < 100.0 && nz.abs() < 100.0
                            && (nx.abs() > 0.01 || ny.abs() > 0.01 || nz.abs() > 0.01) {
                            count += 1;
                        } else {
                            break;
                        }
                    }
                    if count >= 100 {
                        eprintln!("  stride={}: {} positions at granny+0x{:X}", stride, count, offset);
                        for vi in 0..3 {
                            let vx = f32::from_le_bytes(granny[offset+vi*stride..offset+vi*stride+4].try_into().unwrap());
                            let vy = f32::from_le_bytes(granny[offset+vi*stride+4..offset+vi*stride+8].try_into().unwrap());
                            let vz = f32::from_le_bytes(granny[offset+vi*stride+8..offset+vi*stride+12].try_into().unwrap());
                            eprintln!("    v[{}]: ({:.4}, {:.4}, {:.4})", vi, vx, vy, vz);
                        }
                    }
                }
                offset += 4;
            }
        }

        // Same for f16 triples (any range < 100)
        eprintln!("\nScanning granny for runs of reasonable f16 triples (any range < 100)...");
        for stride in [8usize, 12, 16, 20, 24, 28, 32, 36, 44] {
            let mut offset = 0x100;
            while offset + 6 <= granny.len() {
                let x = f16_to_f32(u16::from_le_bytes([granny[offset], granny[offset+1]]));
                let y = f16_to_f32(u16::from_le_bytes([granny[offset+2], granny[offset+3]]));
                let z = f16_to_f32(u16::from_le_bytes([granny[offset+4], granny[offset+5]]));

                if x.is_finite() && y.is_finite() && z.is_finite()
                    && x.abs() < 100.0 && y.abs() < 100.0 && z.abs() < 100.0
                    && (x.abs() > 0.01 || y.abs() > 0.01 || z.abs() > 0.01) {
                    let mut count = 1;
                    while offset + count * stride + 6 <= granny.len() {
                        let nx = f16_to_f32(u16::from_le_bytes([granny[offset+count*stride], granny[offset+count*stride+1]]));
                        let ny = f16_to_f32(u16::from_le_bytes([granny[offset+count*stride+2], granny[offset+count*stride+3]]));
                        let nz = f16_to_f32(u16::from_le_bytes([granny[offset+count*stride+4], granny[offset+count*stride+5]]));
                        if nx.is_finite() && ny.is_finite() && nz.is_finite()
                            && nx.abs() < 100.0 && ny.abs() < 100.0 && nz.abs() < 100.0
                            && (nx.abs() > 0.01 || ny.abs() > 0.01 || nz.abs() > 0.01) {
                            count += 1;
                        } else {
                            break;
                        }
                    }
                    if count >= 100 {
                        eprintln!("  stride={}: {} half-positions at granny+0x{:X}", stride, count, offset);
                        for vi in 0..3 {
                            let vx = f16_to_f32(u16::from_le_bytes([granny[offset+vi*stride], granny[offset+vi*stride+1]]));
                            let vy = f16_to_f32(u16::from_le_bytes([granny[offset+vi*stride+2], granny[offset+vi*stride+3]]));
                            let vz = f16_to_f32(u16::from_le_bytes([granny[offset+vi*stride+4], granny[offset+vi*stride+5]]));
                            eprintln!("    v[{}]: ({:.4}, {:.4}, {:.4})", vi, vx, vy, vz);
                        }
                    }
                }
                offset += 2;
            }
        }
    }

    fn dump_node(node: &bdt::Node, indent: usize) {
        let pad = "  ".repeat(indent);
        let text_str = match &node.text {
            bdt::Variant::Null => String::new(),
            other => format!(" text={:?}", other),
        };
        eprintln!("{}<{}>{}", pad, node.name, text_str);
        for attr in &node.attributes {
            eprintln!("{}  @{}={:?}", pad, attr.name, attr.value);
        }
        for child in &node.children {
            dump_node(child, indent + 1);
        }
    }

    #[test]
    fn test_material_parsing() {
        let paths = [
            ("foxcannon turret", "../../foxcannon01/mesh_turret_0.ugx"),
            ("foxcannon barrel", "../../foxcannon01/mesh_barrel_0.ugx"),
            ("foxcannon chassis", "../../foxcannon01/mesh_chassis_front_0.ugx"),
            ("foxcannon main", "../../foxcannon01/mesh_foxcannon01.ugx"),
            ("banshee damage", "../../test_ugx/art/covenant/air/banshee_01/banshee_damage_01.ugx"),
            ("banshee upgrade", "../../test_ugx/art/covenant/air/banshee_01/upgrade_01.ugx"),
        ];

        for (label, path) in paths {
            let data = match std::fs::read(path) {
                Ok(d) => d,
                Err(_) => { eprintln!("  {} - file not found, skipping", label); continue; }
            };

            // Check if 0x704 chunk exists
            let mut cursor = Cursor::new(&data);
            let mut ecf = ecf::EcfReader::new(&mut cursor).unwrap();
            let has_mat_chunk = ecf.read_chunk_data_by_id(ECF_MATERIAL_CHUNK_ID).is_ok();

            // Try direct BDT parse on the material chunk
            if let Ok(mat_data) = ecf.read_chunk_data_by_id(ECF_MATERIAL_CHUNK_ID) {
                eprintln!("\n=== {} ===", label);
                eprintln!("  Material chunk (0x704): {} bytes", mat_data.len());
                // Hexdump first 64 bytes
                let dump = mat_data.len().min(64);
                for row in mat_data[..dump].chunks(16) {
                    let hex: Vec<String> = row.iter().map(|b| format!("{:02X}", b)).collect();
                    eprintln!("    {}", hex.join(" "));
                }

                match bdt::PackedReader::read_le(&mat_data) {
                    Ok(Some(root)) => {
                        eprintln!("  BDT root: '{}' children={}", root.name, root.children.len());
                        // Dump full tree for first material
                        if let Some(mat_node) = root.children.first() {
                            dump_node(mat_node, 2);
                        }
                    }
                    Ok(None) => eprintln!("  BDT parse: None"),
                    Err(e) => eprintln!("  BDT parse FAILED: {:?}", e),
                }
            } else {
                eprintln!("\n=== {} === NO 0x704 chunk", label);
            }

            // Now test through UgxGeom::read
            let geom = UgxGeom::read(&data).unwrap();
            eprintln!("  UgxGeom materials: {}", geom.materials.len());
            for (i, mat) in geom.materials.iter().enumerate() {
                eprintln!("    [{}] '{}' opacity={:.3} spec_power={} blend_type={} flags={}",
                    i, mat.name, mat.opacity, mat.spec_power, mat.blend_type, mat.flags);
                for (mi, maps) in mat.maps.iter().enumerate() {
                    if !maps.is_empty() {
                        let type_name = MapType::ALL[mi].name();
                        for map in maps {
                            eprintln!("        {}: '{}' ch={} fl={}",
                                type_name, map.name, map.channel, map.flags);
                        }
                    }
                }
            }
            if !has_mat_chunk {
                eprintln!("  (no 0x704 chunk - materials empty as expected)");
            }
        }
    }

    #[test]
    fn dump_foxcannon_vs_v4() {
        let v6_path = "../../foxcannon01/mesh_turret_0.ugx";
        let v4_path = "../../test_ugx/art/covenant/air/banshee_01/banshee_damage_01.ugx";

        for (label, path) in [("V6 foxcannon", v6_path), ("V4 banshee", v4_path)] {
            let data = match std::fs::read(path) {
                Ok(d) => d,
                Err(_) => { eprintln!("  {} - file not found, skipping", label); continue; }
            };

            eprintln!("\n=== {} ({} bytes total) ===", label, data.len());

            // Dump ECF chunk info
            let mut cursor = Cursor::new(&data);
            if let Ok(mut ecf) = ecf::EcfReader::new(&mut cursor) {
                eprintln!("  ECF chunks ({} total):", ecf.chunks().len());
                for (i, chunk) in ecf.chunks().iter().enumerate() {
                    eprintln!("    [{}] id=0x{:03X} offset={} size={} flags=0x{:02X} res_flags=0x{:04X}",
                        i, chunk.id, chunk.offset, chunk.size, chunk.flags, chunk.resource_flags);
                }

                // Dump first 256 bytes of cached data (0x700)
                if let Ok(cached) = ecf.read_chunk_data_by_id(ECF_CACHED_DATA_CHUNK_ID) {
                    let dump_len = 320.min(cached.len());
                    eprintln!("  Cached data (0x700) first {} bytes:", dump_len);
                    let hex: Vec<String> = cached[..dump_len].iter().map(|b| format!("{:02X}", b)).collect();
                    for (i, row) in hex.chunks(16).enumerate() {
                        eprintln!("    {:04X}: {}", i * 16, row.join(" "));
                    }

                    // Parse header manually for comparison
                    let sig = u32::from_le_bytes([cached[0], cached[1], cached[2], cached[3]]);
                    eprintln!("  Header signature: 0x{:08X}", sig);

                    // Section array at 0x40
                    let sec_count = u32::from_le_bytes([cached[0x40], cached[0x41], cached[0x42], cached[0x43]]);
                    let sec_offset = u64::from_le_bytes([
                        cached[0x48], cached[0x49], cached[0x4A], cached[0x4B],
                        cached[0x4C], cached[0x4D], cached[0x4E], cached[0x4F],
                    ]);
                    eprintln!("  Sections: count={}, offset=0x{:X}", sec_count, sec_offset);

                    // Dump section data at offset
                    if sec_count > 0 && (sec_offset as usize) < cached.len() {
                        let sec_start = sec_offset as usize;
                        let sec_dump = 160.min(cached.len() - sec_start);
                        eprintln!("  Section 0 raw data at 0x{:X} ({} bytes):", sec_start, sec_dump);
                        let sec_hex: Vec<String> = cached[sec_start..sec_start+sec_dump].iter().map(|b| format!("{:02X}", b)).collect();
                        for (i, row) in sec_hex.chunks(16).enumerate() {
                            eprintln!("    +{:04X}: {}", i * 16, row.join(" "));
                        }
                    }
                }
            }
        }
    }
}
