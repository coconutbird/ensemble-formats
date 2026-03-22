//! XTT writer implementation.

use alloc::vec::Vec;

use ecf::Writer as EcfWriter;

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
        buf.extend_from_slice(&header.version.to_be_bytes());
        buf.extend_from_slice(&header.num_active_textures.to_be_bytes());
        buf.extend_from_slice(&header.num_active_decals.to_be_bytes());
        buf.extend_from_slice(&header.num_active_decal_instances.to_be_bytes());
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
        buf.extend_from_slice(&linker.grid_x.to_be_bytes());
        buf.extend_from_slice(&linker.grid_z.to_be_bytes());
        buf.extend_from_slice(&linker.spec_pass_needed.to_be_bytes());
        buf.extend_from_slice(&linker.self_pass_needed.to_be_bytes());
        buf.extend_from_slice(&linker.env_mask_pass_needed.to_be_bytes());
        buf.extend_from_slice(&linker.alpha_pass_needed.to_be_bytes());
        buf.extend_from_slice(&linker.is_fully_opaque.to_be_bytes());
        buf.extend_from_slice(&linker.num_splat_layers.to_be_bytes());
        buf.extend_from_slice(&linker.num_decal_layers.to_be_bytes());

        for &id in &linker.splat_layer_ids {
            buf.extend_from_slice(&id.to_be_bytes());
        }
        buf.extend_from_slice(&linker.splat_alpha_data);

        for &id in &linker.decal_layer_ids {
            buf.extend_from_slice(&id.to_be_bytes());
        }
        buf.extend_from_slice(&linker.decal_alpha_data);

        buf
    }

    fn write_foliage_header(foliage: &XttFoliage) -> Vec<u8> {
        let num_sets = foliage.sets.len();
        let mut buf = Vec::with_capacity(4 + num_sets * FILENAME_SIZE);
        buf.extend_from_slice(&(num_sets as u32).to_be_bytes());

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
        buf.extend_from_slice(&qn.qn_parent_index.to_be_bytes());
        buf.extend_from_slice(&qn.num_sets.to_be_bytes());

        for &idx in &qn.set_indices {
            buf.extend_from_slice(&idx.to_be_bytes());
        }
        for &count in &qn.set_poly_counts {
            buf.extend_from_slice(&count.to_be_bytes());
        }

        let total_physical_memory: i32 = qn.index_buffers.iter().map(|b| b.len() as i32).sum();
        buf.extend_from_slice(&total_physical_memory.to_be_bytes());

        for b in &qn.index_buffers {
            buf.extend_from_slice(&(b.len() as i32).to_be_bytes());
        }
        for b in &qn.index_buffers {
            buf.extend_from_slice(b);
        }

        buf
    }
}
