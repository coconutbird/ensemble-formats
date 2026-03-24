//! XTT writer implementation.

use alloc::vec::Vec;

use ecf::Writer as EcfWriter;
use ecf::io::WriteBe;

use crate::{
    CHUNK_ATLAS_ALBEDO, CHUNK_ATLAS_LINK, CHUNK_FOLIAGE_HEADER, CHUNK_FOLIAGE_QN, CHUNK_ROAD,
    CHUNK_XTT_HEADER, FILENAME_SIZE, FoliageQNChunk, Result, XttFile, XttFoliage, XttHeader,
    XttLinker,
};

/// XTT file writer.
pub struct Writer;

impl Writer {
    /// Write an XTT file to a byte vector.
    pub fn write(file: &XttFile) -> Result<Vec<u8>> {
        file.to_bytes()
    }
}

impl XttFile {
    /// Serialize this XTT file to bytes (ECF container).
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        Writer::write_inner(self)
    }
}

impl Writer {
    fn write_inner(file: &XttFile) -> Result<Vec<u8>> {
        let mut ecf = EcfWriter::new(file.ecf_file_id);

        let mut linker_idx = 0;
        let mut foliage_qn_idx = 0;

        for meta in &file.chunk_order {
            let data = match meta.id {
                CHUNK_XTT_HEADER => {
                    let mut header_data = Self::write_header(&file.header);
                    header_data.extend_from_slice(&file.header_extra);
                    header_data
                }
                CHUNK_ATLAS_LINK => {
                    let linker = &file.linkers[linker_idx];
                    linker_idx += 1;
                    Self::write_linker(linker)
                }
                CHUNK_ATLAS_ALBEDO => file.albedo_data.clone(),
                CHUNK_ROAD => file.road_data.clone(),
                CHUNK_FOLIAGE_HEADER => Self::write_foliage_header(&file.foliage),
                CHUNK_FOLIAGE_QN => {
                    let qn = &file.foliage.qn_chunks[foliage_qn_idx];
                    foliage_qn_idx += 1;
                    Self::write_foliage_qn_chunk(qn)
                }
                _ => continue,
            };

            ecf.add_chunk_with_alignment(meta.id, data, meta.alignment_log2);
        }

        Ok(ecf.finalize()?)
    }

    fn write_header(header: &XttHeader) -> Vec<u8> {
        let mut buf = Vec::with_capacity(XttHeader::SIZE);
        buf.write_i32_be(header.version).unwrap();
        buf.write_i32_be(header.num_active_textures).unwrap();
        buf.write_i32_be(header.num_active_decals).unwrap();
        buf.write_i32_be(header.num_active_decal_instances).unwrap();
        buf
    }

    fn write_linker(linker: &XttLinker) -> Vec<u8> {
        let splat_ids_size = linker.splat_layer_ids.len() * 4;
        let decal_ids_size = linker.decal_layer_ids.len() * 4;
        let total_size = XttLinker::HEADER_SIZE
            + splat_ids_size
            + linker.splat_alpha_data.len()
            + decal_ids_size
            + linker.decal_alpha_data.len();

        let mut buf = Vec::with_capacity(total_size);
        buf.write_i32_be(linker.grid_x).unwrap();
        buf.write_i32_be(linker.grid_z).unwrap();
        buf.write_i32_be(linker.spec_pass_needed).unwrap();
        buf.write_i32_be(linker.self_pass_needed).unwrap();
        buf.write_i32_be(linker.env_mask_pass_needed).unwrap();
        buf.write_i32_be(linker.alpha_pass_needed).unwrap();
        buf.write_i32_be(linker.is_fully_opaque).unwrap();
        buf.write_i32_be(linker.num_splat_layers).unwrap();
        buf.write_i32_be(linker.num_decal_layers).unwrap();

        for &id in &linker.splat_layer_ids {
            buf.write_i32_be(id).unwrap();
        }
        buf.extend_from_slice(&linker.splat_alpha_data);

        for &id in &linker.decal_layer_ids {
            buf.write_i32_be(id).unwrap();
        }
        buf.extend_from_slice(&linker.decal_alpha_data);

        buf
    }

    fn write_foliage_header(foliage: &XttFoliage) -> Vec<u8> {
        let num_sets = foliage.sets.len();
        let mut buf = Vec::with_capacity(4 + num_sets * FILENAME_SIZE);
        buf.write_u32_be(num_sets as u32).unwrap();

        for set in &foliage.sets {
            let mut filename_bytes = [0u8; FILENAME_SIZE];
            let bytes = set.filename.as_bytes();
            let len = bytes.len().min(FILENAME_SIZE - 1);
            filename_bytes[..len].copy_from_slice(&bytes[..len]);
            buf.extend_from_slice(&filename_bytes);
        }

        buf
    }

    fn write_foliage_qn_chunk(qn: &FoliageQNChunk) -> Vec<u8> {
        let num_sets = qn.num_sets as usize;
        let total_index_buffer_size: usize = qn.index_buffers.iter().map(|b| b.len()).sum();
        let total_size = 8 + num_sets * 4 * 3 + 4 + total_index_buffer_size;

        let mut buf = Vec::with_capacity(total_size);
        buf.write_u32_be(qn.qn_parent_index).unwrap();
        buf.write_u32_be(qn.num_sets).unwrap();

        for &idx in &qn.set_indices {
            buf.write_i32_be(idx).unwrap();
        }
        for &count in &qn.set_poly_counts {
            buf.write_i32_be(count).unwrap();
        }

        let total_physical_memory: i32 = qn.index_buffers.iter().map(|b| b.len() as i32).sum();
        buf.write_i32_be(total_physical_memory).unwrap();

        for b in &qn.index_buffers {
            buf.write_i32_be(b.len() as i32).unwrap();
        }
        for b in &qn.index_buffers {
            buf.extend_from_slice(b);
        }

        buf
    }
}
