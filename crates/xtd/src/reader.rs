//! XTD reader implementation.

use alloc::format;

use zerocopy::{FromBytes, Immutable, KnownLayout, Ref};

use crate::{
    CHUNK_ALPHA, CHUNK_AO, CHUNK_ATLAS, CHUNK_LIGHTING, CHUNK_TERRAIN, CHUNK_TESS,
    CHUNK_XTD_HEADER, ChunkMeta, Error, Result, XTD_VERSION, XtdFile, XtdHeader, XtdVisualChunk,
};

/// Raw on-disk XTD header (40 bytes, big-endian).
#[derive(FromBytes, KnownLayout, Immutable, Debug)]
#[repr(C)]
struct XtdHeaderRaw {
    version: [u8; 4],
    num_x_verts: [u8; 4],
    num_x_chunks: [u8; 4],
    tile_scale: [u8; 4],
    world_min: [[u8; 4]; 3],
    world_max: [[u8; 4]; 3],
}

/// Raw on-disk XTD visual chunk header (37 bytes, big-endian).
#[derive(FromBytes, KnownLayout, Immutable, Debug)]
#[repr(C)]
struct XtdVisualChunkRaw {
    grid_x: [u8; 4],
    grid_z: [u8; 4],
    max_v_stride: [u8; 4],
    min: [[u8; 4]; 3],
    max: [[u8; 4]; 3],
    can_cast_shadows: u8,
}

/// XTD file reader.
pub struct Reader;

impl Reader {
    /// Read an XTD file from a byte slice.
    pub fn read(data: &[u8]) -> Result<XtdFile> {
        let ecf = ecf::Reader::new(data)?;

        let mut file = XtdFile {
            ecf_file_id: ecf.header().id,
            ecf_flags: ecf.header().flags,
            ..Default::default()
        };

        for i in 0..ecf.chunks().len() {
            let chunk_header = ecf.chunks()[i].clone();

            file.chunk_order.push(ChunkMeta {
                id: chunk_header.id,
                alignment_log2: chunk_header.alignment_log2,
                flags: chunk_header.flags,
                resource_flags: chunk_header.resource_flags,
            });

            let chunk_data = ecf.chunk_data(i)?;

            match chunk_header.id {
                CHUNK_XTD_HEADER => {
                    file.header = Self::read_header(&chunk_data)?;
                }
                CHUNK_TERRAIN => {
                    file.visual_chunks
                        .push(Self::read_visual_chunk(&chunk_data)?);
                }
                CHUNK_ATLAS => file.atlas_data = chunk_data,
                CHUNK_TESS => file.tess_data = chunk_data,
                CHUNK_LIGHTING => file.lighting_data = chunk_data,
                CHUNK_AO => file.ao_data = chunk_data,
                CHUNK_ALPHA => file.alpha_data = chunk_data,
                _ => {}
            }
        }

        Ok(file)
    }

    fn read_header(data: &[u8]) -> Result<XtdHeader> {
        let (raw, _): (Ref<_, XtdHeaderRaw>, _) =
            Ref::from_prefix(data).map_err(|_| Error::InvalidHeaderSize {
                expected: XtdHeader::SIZE,
                actual: data.len(),
            })?;

        let version = i32::from_be_bytes(raw.version);
        if version != XTD_VERSION {
            return Err(Error::InvalidVersion {
                expected: XTD_VERSION,
                actual: version,
            });
        }

        Ok(XtdHeader {
            version,
            num_x_verts: i32::from_be_bytes(raw.num_x_verts),
            num_x_chunks: i32::from_be_bytes(raw.num_x_chunks),
            tile_scale: f32::from_be_bytes(raw.tile_scale),
            world_min: [
                f32::from_be_bytes(raw.world_min[0]),
                f32::from_be_bytes(raw.world_min[1]),
                f32::from_be_bytes(raw.world_min[2]),
            ],
            world_max: [
                f32::from_be_bytes(raw.world_max[0]),
                f32::from_be_bytes(raw.world_max[1]),
                f32::from_be_bytes(raw.world_max[2]),
            ],
        })
    }

    fn read_visual_chunk(data: &[u8]) -> Result<XtdVisualChunk> {
        let (raw, _): (Ref<_, XtdVisualChunkRaw>, _) = Ref::from_prefix(data).map_err(|_| {
            Error::InvalidChunkData(format!(
                "Visual chunk too small: {} < {}",
                data.len(),
                XtdVisualChunk::SIZE
            ))
        })?;

        Ok(XtdVisualChunk {
            grid_x: i32::from_be_bytes(raw.grid_x),
            grid_z: i32::from_be_bytes(raw.grid_z),
            max_v_stride: i32::from_be_bytes(raw.max_v_stride),
            min: [
                f32::from_be_bytes(raw.min[0]),
                f32::from_be_bytes(raw.min[1]),
                f32::from_be_bytes(raw.min[2]),
            ],
            max: [
                f32::from_be_bytes(raw.max[0]),
                f32::from_be_bytes(raw.max[1]),
                f32::from_be_bytes(raw.max[2]),
            ],
            can_cast_shadows: raw.can_cast_shadows != 0,
        })
    }
}
