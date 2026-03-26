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
use crate::chunk_ids::*;
use crate::error::{Error, Result};
use crate::raw::GeomHeaderRaw;
use crate::types::*;

use cached_data::{
    UgxVersion, read_bone_bounds, read_packed_accessories, read_packed_bones, read_packed_sections,
    read_valid_accessory_indices,
};
use granny::{parse_granny_bones, parse_granny_meshes};
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

        let granny_bones = if let Some(ref granny) = granny_data {
            parse_granny_bones(granny)?
        } else {
            Vec::new()
        };

        let granny_meshes = if let Some(ref granny) = granny_data {
            parse_granny_meshes(granny)?
        } else {
            Vec::new()
        };

        let accessories = read_packed_accessories(data, pos)?;
        let valid_accessories = match version {
            UgxVersion::Hw1 => read_packed_accessories(data, pos)?,
            UgxVersion::Hw2 => read_valid_accessory_indices(data, pos, &accessories)?,
        };

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
mod tests {
    use super::*;
    use std::{eprint, eprintln};

    /// Helper: read a test file, skipping if not present on disk.
    fn read_test_file(path: &str) -> Option<Vec<u8>> {
        std::fs::read(path).ok()
    }

    #[test]
    fn read_full_ugx_pipeline() {
        // Exercise the full parse pipeline on every available test file.
        let paths = [
            "../../foxcannon01/mesh_turret_0.ugx",
            "../../foxcannon01/mesh_barrel_0.ugx",
            "../../foxcannon01/mesh_chassis_front_0.ugx",
            "../../foxcannon01/mesh_foxcannon01.ugx",
            "../../test_ugx/art/covenant/air/banshee_01/banshee_damage_01.ugx",
            "../../test_ugx/art/covenant/air/banshee_01/upgrade_01.ugx",
        ];

        let mut parsed = 0usize;
        for path in paths {
            let data = match read_test_file(path) {
                Some(d) => d,
                None => continue,
            };

            let geom = Reader::read(&data).expect(path);

            // Basic structural invariants
            assert!(!geom.sections.is_empty(), "{path}: no sections");
            assert!(!geom.index_buffer.is_empty(), "{path}: empty IB");
            assert!(!geom.vertex_buffer.is_empty(), "{path}: empty VB");
            assert!(geom.bounding_sphere.radius > 0.0, "{path}: zero radius");

            for (i, sec) in geom.sections.iter().enumerate() {
                assert!(sec.num_verts > 0, "{path} sec[{i}]: zero verts");
                assert!(sec.num_tris > 0, "{path} sec[{i}]: zero tris");
                assert!(sec.vert_size > 0, "{path} sec[{i}]: zero vert_size");
                if let Some(ref packer) = sec.base_vert_packer {
                    assert!(
                        !packer.pack_order.is_empty(),
                        "{path} sec[{i}]: empty pack_order"
                    );
                }
            }

            parsed += 1;
        }

        if parsed == 0 {
            eprintln!("No test UGX files found on disk — skipping");
        }
    }

    #[test]
    fn inspect_hw2_ugx() {
        let paths = [
            "/Users/dev/gamedepot/wstore/DUMP/data/maps/rostermode/evenflow_desert/evenflow_desert_water_01/mesh_water.ugx",
            "/Users/dev/gamedepot/wstore/DUMP/data/maps/rostermode/evenflow_desert/evenflow_desert_water_01/childmesh_child_asset003.ugx",
        ];

        for path in paths {
            let data = match read_test_file(path) {
                Some(d) => d,
                None => {
                    eprintln!("HW2 test file not found, skipping");
                    continue;
                }
            };

            let ecf = ecf::Reader::new(&data).unwrap();

            eprintln!("\n=== {} ===", path.rsplit('/').next().unwrap());
            eprintln!("ECF file ID: 0x{:08X}", ecf.header().id);
            eprintln!("ECF chunks: {}", ecf.chunks().len());
            for (i, chunk) in ecf.chunks().iter().enumerate() {
                eprintln!("  chunk[{}]: id=0x{:X}, size={}", i, chunk.id, chunk.size);
            }

            let cached = ecf.chunk_data_by_id(0x700).unwrap();
            eprintln!("Chunk 0x700 size: {} bytes", cached.len());

            let sig = u32::from_le_bytes([cached[0], cached[1], cached[2], cached[3]]);
            eprintln!("Signature: 0x{:08X}", sig);

            // Dump first 160 bytes
            for (i, byte) in cached.iter().enumerate().take(160) {
                if i % 16 == 0 {
                    eprint!("\n  {:04x}: ", i);
                }
                eprint!("{:02x} ", byte);
            }
            eprintln!();

            // After 64-byte header, read packed arrays
            eprintln!("\n--- Packed Arrays (after 64-byte header) ---");
            for arr_idx in 0..8 {
                let base = 64 + arr_idx * 16;
                if base + 16 > cached.len() {
                    break;
                }
                let count = u32::from_le_bytes([
                    cached[base],
                    cached[base + 1],
                    cached[base + 2],
                    cached[base + 3],
                ]);
                let offset = u64::from_le_bytes([
                    cached[base + 8],
                    cached[base + 9],
                    cached[base + 10],
                    cached[base + 11],
                    cached[base + 12],
                    cached[base + 13],
                    cached[base + 14],
                    cached[base + 15],
                ]);
                eprintln!(
                    "  Array[{}]: count={}, offset=0x{:X}",
                    arr_idx, count, offset
                );

                if count > 1 && (offset as usize) < cached.len() {
                    let next_base = 64 + (arr_idx + 1) * 16;
                    if next_base + 16 <= cached.len() {
                        let next_offset = u64::from_le_bytes([
                            cached[next_base + 8],
                            cached[next_base + 9],
                            cached[next_base + 10],
                            cached[next_base + 11],
                            cached[next_base + 12],
                            cached[next_base + 13],
                            cached[next_base + 14],
                            cached[next_base + 15],
                        ]);
                        if next_offset > offset && next_offset != 0xFFFFFFFFFFFFFFFF {
                            let span = next_offset - offset;
                            eprintln!(
                                "    -> span to next: {} bytes, per-element: {}",
                                span,
                                span / count as u64
                            );
                        }
                    }
                }
            }

            // Check other chunks
            for chunk_id in [0x701u64, 0x702, 0x703, 0x704, 0x705] {
                match ecf.chunk_data_by_id(chunk_id) {
                    Ok(d) => {
                        eprintln!("\nChunk 0x{:03X}: {} bytes", chunk_id, d.len());
                        if chunk_id == 0x705 {
                            let ver = u32::from_le_bytes([d[0], d[1], d[2], d[3]]);
                            eprintln!("  AABB tree version: 0x{:08X}", ver);
                            let nc = u32::from_le_bytes([d[4], d[5], d[6], d[7]]);
                            eprintln!("  AABB tree node count: {}", nc);
                        }
                    }
                    Err(_) => eprintln!("\nChunk 0x{:03X}: NOT FOUND", chunk_id),
                }
            }

            // Dump section data (Array[0]) for the childmesh file
            if cached.len() > 0xA0 {
                let arr0_count =
                    u32::from_le_bytes([cached[64], cached[65], cached[66], cached[67]]) as usize;
                let arr0_offset = u64::from_le_bytes([
                    cached[72], cached[73], cached[74], cached[75], cached[76], cached[77],
                    cached[78], cached[79],
                ]) as usize;

                eprintln!(
                    "\n--- Section data (Array[0]: count={}, offset=0x{:X}) ---",
                    arr0_count, arr0_offset
                );
                for sec_idx in 0..arr0_count {
                    let sec_start = arr0_offset + sec_idx * 72;
                    eprintln!("  Section[{}] at 0x{:X}:", sec_idx, sec_start);
                    for row in 0..5 {
                        let row_start = sec_start + row * 16;
                        if row_start + 16 <= cached.len() {
                            eprint!("    {:04x}: ", row_start);
                            for b in 0..16 {
                                if row_start + b < cached.len() {
                                    eprint!("{:02x} ", cached[row_start + b]);
                                }
                            }
                            eprintln!();
                        }
                    }
                    // Remaining 8 bytes
                    let rem_start = sec_start + 64;
                    if rem_start + 8 <= cached.len() {
                        eprint!("    {:04x}: ", rem_start);
                        for b in 0..8 {
                            eprint!("{:02x} ", cached[rem_start + b]);
                        }
                        eprintln!();
                    }

                    // Parse known fields (assuming same first 40 bytes as DE)
                    if sec_start + 40 <= cached.len() {
                        let mat_idx = i32::from_le_bytes([
                            cached[sec_start],
                            cached[sec_start + 1],
                            cached[sec_start + 2],
                            cached[sec_start + 3],
                        ]);
                        let acc_idx = i32::from_le_bytes([
                            cached[sec_start + 4],
                            cached[sec_start + 5],
                            cached[sec_start + 6],
                            cached[sec_start + 7],
                        ]);
                        let max_bones = i32::from_le_bytes([
                            cached[sec_start + 8],
                            cached[sec_start + 9],
                            cached[sec_start + 10],
                            cached[sec_start + 11],
                        ]);
                        let rigid_bone = i32::from_le_bytes([
                            cached[sec_start + 12],
                            cached[sec_start + 13],
                            cached[sec_start + 14],
                            cached[sec_start + 15],
                        ]);
                        let ib_ofs = i32::from_le_bytes([
                            cached[sec_start + 16],
                            cached[sec_start + 17],
                            cached[sec_start + 18],
                            cached[sec_start + 19],
                        ]);
                        let num_tris = i32::from_le_bytes([
                            cached[sec_start + 20],
                            cached[sec_start + 21],
                            cached[sec_start + 22],
                            cached[sec_start + 23],
                        ]);
                        let vb_ofs = i32::from_le_bytes([
                            cached[sec_start + 24],
                            cached[sec_start + 25],
                            cached[sec_start + 26],
                            cached[sec_start + 27],
                        ]);
                        let vb_bytes = i32::from_le_bytes([
                            cached[sec_start + 28],
                            cached[sec_start + 29],
                            cached[sec_start + 30],
                            cached[sec_start + 31],
                        ]);
                        let vert_size = i32::from_le_bytes([
                            cached[sec_start + 32],
                            cached[sec_start + 33],
                            cached[sec_start + 34],
                            cached[sec_start + 35],
                        ]);
                        let num_verts = i32::from_le_bytes([
                            cached[sec_start + 36],
                            cached[sec_start + 37],
                            cached[sec_start + 38],
                            cached[sec_start + 39],
                        ]);
                        eprintln!(
                            "    mat={} acc={} maxBones={} rigidBone={}",
                            mat_idx, acc_idx, max_bones, rigid_bone
                        );
                        eprintln!(
                            "    ibOfs={} numTris={} vbOfs={} vbBytes={}",
                            ib_ofs, num_tris, vb_ofs, vb_bytes
                        );
                        eprintln!("    vertSize={} numVerts={}", vert_size, num_verts);
                    }
                }
            }

            // Now try the actual parser
            eprintln!("\n--- Attempting UGX parse ---");
            match UgxGeom::from_bytes(&data) {
                Ok(geom) => {
                    eprintln!("SUCCESS!");
                    eprintln!("  Sections: {}", geom.sections.len());
                    eprintln!("  Materials: {}", geom.materials.len());
                    eprintln!("  Bones: {}", geom.bones.len());
                }
                Err(e) => {
                    eprintln!("FAILED: {:?}", e);
                }
            }
        }
    }
}
