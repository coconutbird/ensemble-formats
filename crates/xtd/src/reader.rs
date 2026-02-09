//! XTD reader implementation.

use crate::{
    ChunkMeta, Error, Result, XtdFile, XtdHeader, XtdVisualChunk, CHUNK_ALPHA, CHUNK_AO,
    CHUNK_ATLAS, CHUNK_LIGHTING, CHUNK_TERRAIN, CHUNK_TESS, CHUNK_XTD_HEADER, XTD_VERSION,
};
use byteorder::{BigEndian, ReadBytesExt};
use ecf::EcfReader;
use std::io::{Cursor, Read, Seek};

/// XTD file reader.
pub struct XtdReader;

impl XtdReader {
    /// Read an XTD file from a byte slice.
    pub fn read(data: &[u8]) -> Result<XtdFile> {
        let mut cursor = Cursor::new(data);
        Self::read_from(&mut cursor)
    }

    /// Read an XTD file from a reader.
    pub fn read_from<R: Read + Seek>(reader: &mut R) -> Result<XtdFile> {
        let mut ecf = EcfReader::new(reader)?;

        let mut file = XtdFile::default();

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
                CHUNK_XTD_HEADER => {
                    file.header = Self::read_header(&chunk_data)?;
                }
                CHUNK_TERRAIN => {
                    let chunk = Self::read_visual_chunk(&chunk_data)?;
                    file.visual_chunks.push(chunk);
                }
                CHUNK_ATLAS => {
                    file.atlas_data = chunk_data;
                }
                CHUNK_TESS => {
                    file.tess_data = chunk_data;
                }
                CHUNK_LIGHTING => {
                    file.lighting_data = chunk_data;
                }
                CHUNK_AO => {
                    file.ao_data = chunk_data;
                }
                CHUNK_ALPHA => {
                    file.alpha_data = chunk_data;
                }
                _ => {
                    // Unknown chunk type - we'll lose this on round-trip
                }
            }
        }

        Ok(file)
    }

    fn read_header(data: &[u8]) -> Result<XtdHeader> {
        if data.len() < XtdHeader::SIZE {
            return Err(Error::InvalidHeaderSize {
                expected: XtdHeader::SIZE,
                actual: data.len(),
            });
        }

        let mut cursor = Cursor::new(data);

        let version = cursor.read_i32::<BigEndian>()?;
        if version != XTD_VERSION {
            return Err(Error::InvalidVersion {
                expected: XTD_VERSION,
                actual: version,
            });
        }

        Ok(XtdHeader {
            version,
            num_x_verts: cursor.read_i32::<BigEndian>()?,
            num_x_chunks: cursor.read_i32::<BigEndian>()?,
            tile_scale: cursor.read_f32::<BigEndian>()?,
            world_min: [
                cursor.read_f32::<BigEndian>()?,
                cursor.read_f32::<BigEndian>()?,
                cursor.read_f32::<BigEndian>()?,
            ],
            world_max: [
                cursor.read_f32::<BigEndian>()?,
                cursor.read_f32::<BigEndian>()?,
                cursor.read_f32::<BigEndian>()?,
            ],
        })
    }

    fn read_visual_chunk(data: &[u8]) -> Result<XtdVisualChunk> {
        if data.len() < XtdVisualChunk::SIZE {
            return Err(Error::InvalidChunkData(format!(
                "Visual chunk too small: {} < {}",
                data.len(),
                XtdVisualChunk::SIZE
            )));
        }

        let mut cursor = Cursor::new(data);

        Ok(XtdVisualChunk {
            grid_x: cursor.read_i32::<BigEndian>()?,
            grid_z: cursor.read_i32::<BigEndian>()?,
            max_v_stride: cursor.read_i32::<BigEndian>()?,
            min: [
                cursor.read_f32::<BigEndian>()?,
                cursor.read_f32::<BigEndian>()?,
                cursor.read_f32::<BigEndian>()?,
            ],
            max: [
                cursor.read_f32::<BigEndian>()?,
                cursor.read_f32::<BigEndian>()?,
                cursor.read_f32::<BigEndian>()?,
            ],
            can_cast_shadows: cursor.read_u8()? != 0,
        })
    }
}
