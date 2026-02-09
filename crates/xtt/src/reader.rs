//! XTT reader implementation.

use crate::{
    ChunkMeta, Error, Result, XttFile, XttHeader, XttLinker, CHUNK_ATLAS_ALBEDO, CHUNK_ATLAS_LINK,
    CHUNK_FOLIAGE_HEADER, CHUNK_FOLIAGE_QN, CHUNK_ROAD, CHUNK_XTT_HEADER, XTT_VERSION,
};
use byteorder::{BigEndian, ReadBytesExt};
use ecf::EcfReader;
use std::io::{Cursor, Read, Seek};

/// XTT file reader.
pub struct XttReader;

impl XttReader {
    /// Read an XTT file from a byte slice.
    pub fn read(data: &[u8]) -> Result<XttFile> {
        let mut cursor = Cursor::new(data);
        Self::read_from(&mut cursor)
    }

    /// Read an XTT file from a reader.
    pub fn read_from<R: Read + Seek>(reader: &mut R) -> Result<XttFile> {
        let mut ecf = EcfReader::new(reader)?;

        let mut file = XttFile::default();

        // Store ECF metadata for round-trip fidelity
        file.ecf_file_id = ecf.header().id;
        file.ecf_flags = ecf.header().flags;

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

            let chunk_data = ecf.read_chunk_data(i)?;

            match chunk_header.id {
                CHUNK_XTT_HEADER => {
                    file.header = Self::read_header(&chunk_data)?;
                    // Store extra data beyond the header
                    if chunk_data.len() > XttHeader::SIZE {
                        file.header_extra = chunk_data[XttHeader::SIZE..].to_vec();
                    }
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
                    file.foliage.header_data = chunk_data;
                }
                CHUNK_FOLIAGE_QN => {
                    file.foliage.qn_chunks.push(chunk_data);
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

        // Rest is splat + decal data
        let remaining = data[XttLinker::HEADER_SIZE..].to_vec();

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
            splat_data: remaining.clone(), // For now, store all as splat
            decal_data: Vec::new(),
        })
    }
}
