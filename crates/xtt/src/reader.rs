//! XTT reader implementation.

use crate::{
    ActiveDecalInfo, ActiveDecalInstance, ActiveTextureInfo, CHUNK_ATLAS_ALBEDO, CHUNK_ATLAS_LINK,
    CHUNK_FOLIAGE_HEADER, CHUNK_FOLIAGE_QN, CHUNK_ROAD, CHUNK_XTT_HEADER, ChunkMeta, Error,
    FoliageQNChunk, FoliageSetInfo, Result, XTT_VERSION, XttFile, XttHeader, XttLinker,
};
use byteorder::{BigEndian, ReadBytesExt};
use ecf::EcfReader;
use std::io::{Cursor, Read};

/// Size of filename strings in XTT files.
const XTT_FILENAME_SIZE: usize = 256;

/// XTT file reader.
pub struct XttReader;

impl XttReader {
    /// Read an XTT file from a byte slice.
    pub fn read(data: &[u8]) -> Result<XttFile> {
        let ecf = EcfReader::new(data)?;

        let mut file = XttFile {
            ecf_file_id: ecf.header().id,
            ecf_flags: ecf.header().flags,
            ..Default::default()
        };

        // Read all chunks
        for i in 0..ecf.chunks().len() {
            // Clone chunk header to avoid borrow issues
            let chunk_header = ecf.chunks()[i].clone();

            // Store chunk metadata for round-trip
            file.chunk_order.push(ChunkMeta {
                id: chunk_header.id,
                alignment_log2: chunk_header.alignment_log2,
                flags: chunk_header.flags,
                resource_flags: chunk_header.resource_flags,
            });

            let chunk_data = ecf.chunk_data(i)?;

            match chunk_header.id {
                CHUNK_XTT_HEADER => {
                    file.header = Self::read_header(&chunk_data)?;
                    // Store extra data beyond the header
                    if chunk_data.len() > XttHeader::SIZE {
                        file.header_extra = chunk_data[XttHeader::SIZE..].to_vec();
                    }
                    // Parse active textures, decals, and decal instances
                    Self::parse_header_extra(&mut file)?;
                }
                CHUNK_ATLAS_LINK => {
                    let linker = Self::read_linker(&chunk_data)?;
                    file.linkers.push(linker);
                }
                CHUNK_ATLAS_ALBEDO => {
                    file.albedo_data = chunk_data;
                }
                CHUNK_ROAD => {
                    file.road_data = chunk_data;
                }
                CHUNK_FOLIAGE_HEADER => {
                    file.foliage.sets = Self::read_foliage_header(&chunk_data)?;
                }
                CHUNK_FOLIAGE_QN => {
                    let qn_chunk = Self::read_foliage_qn_chunk(&chunk_data)?;
                    file.foliage.qn_chunks.push(qn_chunk);
                }
                _ => {
                    // Unknown chunk, skip
                }
            }
        }

        Ok(file)
    }

    fn read_header(data: &[u8]) -> Result<XttHeader> {
        if data.len() < XttHeader::SIZE {
            return Err(Error::InvalidHeaderSize {
                expected: XttHeader::SIZE,
                actual: data.len(),
            });
        }

        let mut cursor = Cursor::new(data);

        let version = cursor.read_i32::<BigEndian>()?;
        if version != XTT_VERSION {
            return Err(Error::InvalidVersion {
                expected: XTT_VERSION,
                actual: version,
            });
        }

        Ok(XttHeader {
            version,
            num_active_textures: cursor.read_i32::<BigEndian>()?,
            num_active_decals: cursor.read_i32::<BigEndian>()?,
            num_active_decal_instances: cursor.read_i32::<BigEndian>()?,
        })
    }

    fn read_linker(data: &[u8]) -> Result<XttLinker> {
        if data.len() < XttLinker::HEADER_SIZE {
            return Err(Error::InvalidChunkData(format!(
                "Linker chunk too small: {} < {}",
                data.len(),
                XttLinker::HEADER_SIZE
            )));
        }

        let mut cursor = Cursor::new(data);

        let grid_x = cursor.read_i32::<BigEndian>()?;
        let grid_z = cursor.read_i32::<BigEndian>()?;
        let spec_pass_needed = cursor.read_i32::<BigEndian>()?;
        let self_pass_needed = cursor.read_i32::<BigEndian>()?;
        let env_mask_pass_needed = cursor.read_i32::<BigEndian>()?;
        let alpha_pass_needed = cursor.read_i32::<BigEndian>()?;
        let is_fully_opaque = cursor.read_i32::<BigEndian>()?;
        let num_splat_layers = cursor.read_i32::<BigEndian>()?;
        let num_decal_layers = cursor.read_i32::<BigEndian>()?;

        // Parse splat layer IDs
        let mut splat_layer_ids = Vec::with_capacity(num_splat_layers as usize);
        for _ in 0..num_splat_layers {
            splat_layer_ids.push(cursor.read_i32::<BigEndian>()?);
        }

        // Parse splat alpha data (if more than 1 layer)
        let splat_alpha_data = if num_splat_layers > 1 {
            // Calculate size: numAlignedLayers * 64 * 64 * 16bpp / 8
            let num_aligned_layers = ((num_splat_layers - 1) >> 2) + 1;
            let mem_size = (num_aligned_layers as usize
                * XttLinker::ALPHA_TEXTURE_WIDTH
                * XttLinker::ALPHA_TEXTURE_HEIGHT
                * XttLinker::ALPHA_BPP)
                >> 3;
            let mut alpha_data = vec![0u8; mem_size];
            cursor.read_exact(&mut alpha_data)?;
            alpha_data
        } else {
            Vec::new()
        };

        // Parse decal layer IDs (if any decal layers)
        let mut decal_layer_ids = Vec::new();
        let mut decal_alpha_data = Vec::new();

        if num_decal_layers > 0 {
            for _ in 0..num_decal_layers {
                decal_layer_ids.push(cursor.read_i32::<BigEndian>()?);
            }

            // Parse decal alpha data
            let num_aligned_layers = ((num_decal_layers - 1) >> 2) + 1;
            let mem_size = (num_aligned_layers as usize
                * XttLinker::ALPHA_TEXTURE_WIDTH
                * XttLinker::ALPHA_TEXTURE_HEIGHT
                * XttLinker::ALPHA_BPP)
                >> 3;
            decal_alpha_data = vec![0u8; mem_size];
            cursor.read_exact(&mut decal_alpha_data)?;
        }

        Ok(XttLinker {
            grid_x,
            grid_z,
            spec_pass_needed,
            self_pass_needed,
            env_mask_pass_needed,
            alpha_pass_needed,
            is_fully_opaque,
            num_splat_layers,
            num_decal_layers,
            splat_layer_ids,
            splat_alpha_data,
            decal_layer_ids,
            decal_alpha_data,
        })
    }

    /// Parse header_extra data to extract active textures, decals, and decal instances.
    fn parse_header_extra(file: &mut XttFile) -> Result<()> {
        if file.header_extra.is_empty() {
            return Ok(());
        }

        let mut cursor = Cursor::new(&file.header_extra);

        // Parse active textures
        for _ in 0..file.header.num_active_textures {
            let texture = Self::read_active_texture(&mut cursor)?;
            file.active_textures.push(texture);
        }

        // Parse active decals (if any)
        for _ in 0..file.header.num_active_decals {
            let decal = Self::read_active_decal(&mut cursor)?;
            file.active_decals.push(decal);
        }

        // Parse decal instances (if any)
        for _ in 0..file.header.num_active_decal_instances {
            let instance = Self::read_decal_instance(&mut cursor)?;
            file.decal_instances.push(instance);
        }

        Ok(())
    }

    /// Read a single active texture from the cursor.
    fn read_active_texture<R: Read>(reader: &mut R) -> Result<ActiveTextureInfo> {
        // Read 256-byte filename (null-terminated string)
        let mut filename_bytes = [0u8; 256];
        reader.read_exact(&mut filename_bytes)?;

        // Find null terminator and convert to string
        let filename = {
            let end = filename_bytes
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(filename_bytes.len());
            String::from_utf8_lossy(&filename_bytes[..end]).to_string()
        };

        // Read scale and blend op (BigEndian for Xbox 360)
        let u_scale = reader.read_i32::<BigEndian>()?;
        let v_scale = reader.read_i32::<BigEndian>()?;
        let blend_op = reader.read_i32::<BigEndian>()?;

        Ok(ActiveTextureInfo {
            filename,
            u_scale,
            v_scale,
            blend_op,
        })
    }

    /// Read a single active decal from the cursor.
    fn read_active_decal<R: Read>(reader: &mut R) -> Result<ActiveDecalInfo> {
        // Read 256-byte filename (null-terminated string)
        let mut filename_bytes = [0u8; 256];
        reader.read_exact(&mut filename_bytes)?;

        let filename = {
            let end = filename_bytes
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(filename_bytes.len());
            String::from_utf8_lossy(&filename_bytes[..end]).to_string()
        };

        Ok(ActiveDecalInfo { filename })
    }

    /// Read a single decal instance from the cursor.
    fn read_decal_instance<R: Read>(reader: &mut R) -> Result<ActiveDecalInstance> {
        let active_decal_index = reader.read_i32::<BigEndian>()?;
        let rotation = f32::from_bits(reader.read_u32::<BigEndian>()?);
        let tile_center_x = f32::from_bits(reader.read_u32::<BigEndian>()?);
        let tile_center_y = f32::from_bits(reader.read_u32::<BigEndian>()?);
        let u_scale = f32::from_bits(reader.read_u32::<BigEndian>()?);
        let v_scale = f32::from_bits(reader.read_u32::<BigEndian>()?);

        Ok(ActiveDecalInstance {
            active_decal_index,
            rotation,
            tile_center_x,
            tile_center_y,
            u_scale,
            v_scale,
        })
    }

    /// Parse the foliage header chunk.
    ///
    /// From TerrainIO.cpp:
    /// ```cpp
    /// case cXTT_FoliageHeaderChunk:
    ///    ecfReader.getStream()->readObj(numSetsUsed);
    ///    for(uint i=0;i<numSetsUsed;i++) {
    ///       ecfReader.getStream()->readBytes(&mFilename, cXTT_FilenameSize);
    ///       gFoliageManager.newSet(mFilename);
    ///    }
    /// ```
    fn read_foliage_header(data: &[u8]) -> Result<Vec<FoliageSetInfo>> {
        if data.len() < 4 {
            return Ok(Vec::new());
        }

        let mut cursor = Cursor::new(data);
        let num_sets = cursor.read_u32::<BigEndian>()?;

        let mut sets = Vec::with_capacity(num_sets as usize);
        for _ in 0..num_sets {
            let mut filename_bytes = [0u8; XTT_FILENAME_SIZE];
            cursor.read_exact(&mut filename_bytes)?;

            let filename = {
                let end = filename_bytes
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(filename_bytes.len());
                String::from_utf8_lossy(&filename_bytes[..end]).to_string()
            };

            sets.push(FoliageSetInfo { filename });
        }

        Ok(sets)
    }

    /// Parse a foliage QN (quad-node) chunk.
    ///
    /// From TerrainIO.cpp:
    /// ```cpp
    /// case cXTT_FoliageQNChunk:
    ///    ecfReader.getStream()->readObj(qnc->mQNParentIndex);
    ///    ecfReader.getStream()->readObj(qnc->mNumSets);
    ///    ecfReader.getStream()->readBytes(qnc->mSetIndexes, qnc->mNumSets * sizeof(uint));
    ///    ecfReader.getStream()->readBytes(qnc->mSetPolyCount, qnc->mNumSets * sizeof(uint));
    ///    ecfReader.getStream()->readObj(totalPhysicalMemory);
    ///    ecfReader.getStream()->readBytes(indMemSizes, qnc->mNumSets * sizeof(uint));
    ///    // Raw index buffer data follows
    /// ```
    fn read_foliage_qn_chunk(data: &[u8]) -> Result<FoliageQNChunk> {
        if data.len() < 8 {
            return Err(Error::InvalidChunkData(
                "Foliage QN chunk too small".to_string(),
            ));
        }

        let mut cursor = Cursor::new(data);

        let qn_parent_index = cursor.read_u32::<BigEndian>()?;
        let num_sets = cursor.read_u32::<BigEndian>()?;

        // Read set indices
        let mut set_indices = Vec::with_capacity(num_sets as usize);
        for _ in 0..num_sets {
            set_indices.push(cursor.read_i32::<BigEndian>()?);
        }

        // Read poly counts
        let mut set_poly_counts = Vec::with_capacity(num_sets as usize);
        for _ in 0..num_sets {
            set_poly_counts.push(cursor.read_i32::<BigEndian>()?);
        }

        // Read total physical memory size
        let _total_physical_memory = cursor.read_i32::<BigEndian>()?;

        // Read individual memory sizes per set
        let mut ind_mem_sizes = Vec::with_capacity(num_sets as usize);
        for _ in 0..num_sets {
            ind_mem_sizes.push(cursor.read_i32::<BigEndian>()? as usize);
        }

        // Read raw index buffer data for each set
        let mut index_buffers = Vec::with_capacity(num_sets as usize);
        for &size in &ind_mem_sizes {
            let mut buf = vec![0u8; size];
            cursor.read_exact(&mut buf)?;
            index_buffers.push(buf);
        }

        Ok(FoliageQNChunk {
            qn_parent_index,
            num_sets,
            set_indices,
            set_poly_counts,
            index_buffers,
        })
    }
}
