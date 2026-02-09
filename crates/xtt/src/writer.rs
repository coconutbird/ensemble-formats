//! XTT writer implementation.

use crate::{
    Result, XttFile, XttHeader, XttLinker,
    CHUNK_XTT_HEADER, CHUNK_ATLAS_LINK, CHUNK_ATLAS_ALBEDO, CHUNK_ROAD,
    CHUNK_FOLIAGE_HEADER, CHUNK_FOLIAGE_QN,
};
use byteorder::{BigEndian, WriteBytesExt};
use ecf::EcfWriter;
use std::io::{Cursor, Seek, Write};

/// XTT file writer.
pub struct XttWriter;

impl XttWriter {
    /// Write an XTT file to a byte vector.
    pub fn write(file: &XttFile) -> Result<Vec<u8>> {
        let mut buffer = Cursor::new(Vec::new());
        Self::write_to(file, &mut buffer)?;
        Ok(buffer.into_inner())
    }

    /// Write an XTT file to a writer.
    pub fn write_to<W: Write + Seek>(file: &XttFile, writer: &mut W) -> Result<()> {
        let mut ecf = EcfWriter::new(writer, file.ecf_file_id);

        // Track indices for multi-instance chunk types
        let mut linker_idx = 0;
        let mut foliage_qn_idx = 0;

        // Write chunks in original order using stored metadata
        for meta in &file.chunk_order {
            let data = match meta.id {
                CHUNK_XTT_HEADER => {
                    let mut header_data = Self::write_header(&file.header)?;
                    header_data.extend_from_slice(&file.header_extra);
                    header_data
                }
                CHUNK_ATLAS_LINK => {
                    let linker = &file.linkers[linker_idx];
                    linker_idx += 1;
                    Self::write_linker(linker)?
                }
                CHUNK_ATLAS_ALBEDO => file.albedo_data.clone(),
                CHUNK_ROAD => file.road_data.clone(),
                CHUNK_FOLIAGE_HEADER => file.foliage.header_data.clone(),
                CHUNK_FOLIAGE_QN => {
                    let qn = &file.foliage.qn_chunks[foliage_qn_idx];
                    foliage_qn_idx += 1;
                    qn.clone()
                }
                _ => continue, // Skip unknown chunks
            };

            ecf.add_chunk_with_alignment(meta.id, data, meta.alignment_log2);
        }

        ecf.finalize()?;
        Ok(())
    }

    fn write_header(header: &XttHeader) -> Result<Vec<u8>> {
        let mut buffer = Vec::with_capacity(XttHeader::SIZE);
        let mut cursor = Cursor::new(&mut buffer);

        cursor.write_i32::<BigEndian>(header.version)?;
        cursor.write_i32::<BigEndian>(header.num_active_textures)?;
        cursor.write_i32::<BigEndian>(header.num_active_decals)?;
        cursor.write_i32::<BigEndian>(header.num_active_decal_instances)?;

        Ok(buffer)
    }

    fn write_linker(linker: &XttLinker) -> Result<Vec<u8>> {
        let mut buffer = Vec::with_capacity(XttLinker::HEADER_SIZE + linker.splat_data.len());
        let mut cursor = Cursor::new(&mut buffer);

        cursor.write_i32::<BigEndian>(linker.grid_x)?;
        cursor.write_i32::<BigEndian>(linker.grid_z)?;
        cursor.write_i32::<BigEndian>(linker.spec_pass_needed)?;
        cursor.write_i32::<BigEndian>(linker.self_pass_needed)?;
        cursor.write_i32::<BigEndian>(linker.env_mask_pass_needed)?;
        cursor.write_i32::<BigEndian>(linker.alpha_pass_needed)?;
        cursor.write_i32::<BigEndian>(linker.is_fully_opaque)?;
        cursor.write_i32::<BigEndian>(linker.num_splat_layers)?;
        cursor.write_i32::<BigEndian>(linker.num_decal_layers)?;

        // Write splat data
        cursor.get_mut().extend_from_slice(&linker.splat_data);

        Ok(buffer)
    }
}

