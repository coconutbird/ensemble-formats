//! XTD writer implementation.

use alloc::vec::Vec;

use crate::{
    CHUNK_ALPHA, CHUNK_AO, CHUNK_ATLAS, CHUNK_LIGHTING, CHUNK_TERRAIN, CHUNK_TESS,
    CHUNK_XTD_HEADER, Result, XtdFile, XtdHeader, XtdVisualChunk,
};

/// XTD file writer.
pub struct Writer;

impl Writer {
    /// Write an XTD file to a byte vector.
    pub fn write(file: &XtdFile) -> Result<Vec<u8>> {
        let mut ecf = ecf::Writer::new(file.ecf_file_id);

        let mut visual_chunk_idx = 0;

        for meta in &file.chunk_order {
            let data = match meta.id {
                CHUNK_XTD_HEADER => Self::write_header(&file.header),
                CHUNK_TERRAIN => {
                    let chunk = &file.visual_chunks[visual_chunk_idx];
                    visual_chunk_idx += 1;
                    Self::write_visual_chunk(chunk)
                }
                CHUNK_ATLAS => file.atlas_data.clone(),
                CHUNK_TESS => file.tess_data.clone(),
                CHUNK_LIGHTING => file.lighting_data.clone(),
                CHUNK_AO => file.ao_data.clone(),
                CHUNK_ALPHA => file.alpha_data.clone(),
                _ => continue,
            };

            ecf.add_chunk_with_alignment(meta.id, data, meta.alignment_log2);
        }

        Ok(ecf.finalize()?)
    }

    fn write_header(header: &XtdHeader) -> Vec<u8> {
        let mut buf = Vec::with_capacity(XtdHeader::SIZE);
        buf.extend_from_slice(&header.version.to_be_bytes());
        buf.extend_from_slice(&header.num_x_verts.to_be_bytes());
        buf.extend_from_slice(&header.num_x_chunks.to_be_bytes());
        buf.extend_from_slice(&header.tile_scale.to_be_bytes());
        for v in &header.world_min {
            buf.extend_from_slice(&v.to_be_bytes());
        }
        for v in &header.world_max {
            buf.extend_from_slice(&v.to_be_bytes());
        }
        buf
    }

    fn write_visual_chunk(chunk: &XtdVisualChunk) -> Vec<u8> {
        let mut buf = Vec::with_capacity(XtdVisualChunk::SIZE);
        buf.extend_from_slice(&chunk.grid_x.to_be_bytes());
        buf.extend_from_slice(&chunk.grid_z.to_be_bytes());
        buf.extend_from_slice(&chunk.max_v_stride.to_be_bytes());
        for v in &chunk.min {
            buf.extend_from_slice(&v.to_be_bytes());
        }
        for v in &chunk.max {
            buf.extend_from_slice(&v.to_be_bytes());
        }
        buf.push(if chunk.can_cast_shadows { 1 } else { 0 });
        buf
    }
}
