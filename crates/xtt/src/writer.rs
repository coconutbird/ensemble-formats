//! XTT writer implementation.

use crate::{
    CHUNK_ATLAS_ALBEDO, CHUNK_ATLAS_LINK, CHUNK_FOLIAGE_HEADER, CHUNK_FOLIAGE_QN, CHUNK_ROAD,
    CHUNK_XTT_HEADER, FILENAME_SIZE, FoliageQNChunk, Result, XttFile, XttFoliage, XttHeader,
    XttLinker,
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
                CHUNK_FOLIAGE_HEADER => Self::write_foliage_header(&file.foliage)?,
                CHUNK_FOLIAGE_QN => {
                    let qn = &file.foliage.qn_chunks[foliage_qn_idx];
                    foliage_qn_idx += 1;
                    Self::write_foliage_qn_chunk(qn)?
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
        // Calculate total size
        let splat_ids_size = linker.splat_layer_ids.len() * 4;
        let decal_ids_size = linker.decal_layer_ids.len() * 4;
        let total_size = XttLinker::HEADER_SIZE
            + splat_ids_size
            + linker.splat_alpha_data.len()
            + decal_ids_size
            + linker.decal_alpha_data.len();

        let mut buffer = Vec::with_capacity(total_size);
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

        // Write splat layer IDs
        for &id in &linker.splat_layer_ids {
            cursor.write_i32::<BigEndian>(id)?;
        }

        // Write splat alpha data
        cursor.get_mut().extend_from_slice(&linker.splat_alpha_data);

        // Write decal layer IDs
        for &id in &linker.decal_layer_ids {
            cursor.write_i32::<BigEndian>(id)?;
        }

        // Write decal alpha data
        cursor.get_mut().extend_from_slice(&linker.decal_alpha_data);

        Ok(buffer)
    }

    /// Write foliage header chunk data.
    fn write_foliage_header(foliage: &XttFoliage) -> Result<Vec<u8>> {
        let num_sets = foliage.sets.len();
        let mut buffer = Vec::with_capacity(4 + num_sets * FILENAME_SIZE);
        let mut cursor = Cursor::new(&mut buffer);

        cursor.write_u32::<BigEndian>(num_sets as u32)?;

        for set in &foliage.sets {
            // Write 256-byte filename (null-padded)
            let mut filename_bytes = [0u8; FILENAME_SIZE];
            let bytes = set.filename.as_bytes();
            let len = bytes.len().min(FILENAME_SIZE - 1);
            filename_bytes[..len].copy_from_slice(&bytes[..len]);
            cursor.get_mut().extend_from_slice(&filename_bytes);
        }

        Ok(buffer)
    }

    /// Write foliage QN chunk data.
    fn write_foliage_qn_chunk(qn: &FoliageQNChunk) -> Result<Vec<u8>> {
        // Calculate total size
        let num_sets = qn.num_sets as usize;
        let total_index_buffer_size: usize = qn.index_buffers.iter().map(|b| b.len()).sum();
        let total_size = 8 + num_sets * 4 * 3 + 4 + total_index_buffer_size; // header + sets*3*4 + totalMem + data

        let mut buffer = Vec::with_capacity(total_size);
        let mut cursor = Cursor::new(&mut buffer);

        cursor.write_u32::<BigEndian>(qn.qn_parent_index)?;
        cursor.write_u32::<BigEndian>(qn.num_sets)?;

        // Write set indices
        for &idx in &qn.set_indices {
            cursor.write_i32::<BigEndian>(idx)?;
        }

        // Write poly counts
        for &count in &qn.set_poly_counts {
            cursor.write_i32::<BigEndian>(count)?;
        }

        // Calculate total physical memory
        let total_physical_memory: i32 = qn.index_buffers.iter().map(|b| b.len() as i32).sum();
        cursor.write_i32::<BigEndian>(total_physical_memory)?;

        // Write individual memory sizes
        for buf in &qn.index_buffers {
            cursor.write_i32::<BigEndian>(buf.len() as i32)?;
        }

        // Write raw index buffer data
        for buf in &qn.index_buffers {
            cursor.get_mut().extend_from_slice(buf);
        }

        Ok(buffer)
    }
}
