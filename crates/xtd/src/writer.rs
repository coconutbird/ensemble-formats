//! XTD writer implementation.

use crate::{
    Result, XtdFile, XtdHeader, XtdVisualChunk,
    CHUNK_XTD_HEADER, CHUNK_TERRAIN, CHUNK_ATLAS, CHUNK_TESS,
    CHUNK_LIGHTING, CHUNK_AO, CHUNK_ALPHA,
};
use byteorder::{BigEndian, WriteBytesExt};
use ecf::EcfWriter;
use std::io::{Cursor, Seek, Write};

/// XTD file writer.
pub struct XtdWriter;

impl XtdWriter {
    /// Write an XTD file to a byte vector.
    pub fn write(file: &XtdFile) -> Result<Vec<u8>> {
        let mut buffer = Cursor::new(Vec::new());
        Self::write_to(file, &mut buffer)?;
        Ok(buffer.into_inner())
    }

    /// Write an XTD file to a writer.
    pub fn write_to<W: Write + Seek>(file: &XtdFile, writer: &mut W) -> Result<()> {
        let mut ecf = EcfWriter::new(writer, file.ecf_file_id);

        // Track which visual chunk we're on
        let mut visual_chunk_idx = 0;

        // Write chunks in original order using stored metadata
        for meta in &file.chunk_order {
            let data = match meta.id {
                CHUNK_XTD_HEADER => Self::write_header(&file.header)?,
                CHUNK_TERRAIN => {
                    let chunk = &file.visual_chunks[visual_chunk_idx];
                    visual_chunk_idx += 1;
                    Self::write_visual_chunk(chunk)?
                }
                CHUNK_ATLAS => file.atlas_data.clone(),
                CHUNK_TESS => file.tess_data.clone(),
                CHUNK_LIGHTING => file.lighting_data.clone(),
                CHUNK_AO => file.ao_data.clone(),
                CHUNK_ALPHA => file.alpha_data.clone(),
                _ => continue, // Skip unknown chunks
            };

            ecf.add_chunk_with_alignment(meta.id, data, meta.alignment_log2);
        }

        ecf.finalize()?;
        Ok(())
    }

    fn write_header(header: &XtdHeader) -> Result<Vec<u8>> {
        let mut buffer = Vec::with_capacity(XtdHeader::SIZE);
        let mut cursor = Cursor::new(&mut buffer);

        cursor.write_i32::<BigEndian>(header.version)?;
        cursor.write_i32::<BigEndian>(header.num_x_verts)?;
        cursor.write_i32::<BigEndian>(header.num_x_chunks)?;
        cursor.write_f32::<BigEndian>(header.tile_scale)?;
        cursor.write_f32::<BigEndian>(header.world_min[0])?;
        cursor.write_f32::<BigEndian>(header.world_min[1])?;
        cursor.write_f32::<BigEndian>(header.world_min[2])?;
        cursor.write_f32::<BigEndian>(header.world_max[0])?;
        cursor.write_f32::<BigEndian>(header.world_max[1])?;
        cursor.write_f32::<BigEndian>(header.world_max[2])?;

        Ok(buffer)
    }

    fn write_visual_chunk(chunk: &XtdVisualChunk) -> Result<Vec<u8>> {
        let mut buffer = Vec::with_capacity(XtdVisualChunk::SIZE);
        let mut cursor = Cursor::new(&mut buffer);

        cursor.write_i32::<BigEndian>(chunk.grid_x)?;
        cursor.write_i32::<BigEndian>(chunk.grid_z)?;
        cursor.write_i32::<BigEndian>(chunk.max_v_stride)?;
        cursor.write_f32::<BigEndian>(chunk.min[0])?;
        cursor.write_f32::<BigEndian>(chunk.min[1])?;
        cursor.write_f32::<BigEndian>(chunk.min[2])?;
        cursor.write_f32::<BigEndian>(chunk.max[0])?;
        cursor.write_f32::<BigEndian>(chunk.max[1])?;
        cursor.write_f32::<BigEndian>(chunk.max[2])?;
        cursor.write_u8(if chunk.can_cast_shadows { 1 } else { 0 })?;

        Ok(buffer)
    }
}

