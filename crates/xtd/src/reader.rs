//! XTD reader implementation.

use alloc::format;

use zerocopy::{FromBytes, Immutable, KnownLayout, Ref};

use crate::{
    CHUNK_ALPHA, CHUNK_AO, CHUNK_ATLAS, CHUNK_LIGHTING, CHUNK_TERRAIN, CHUNK_TESS,
    CHUNK_XTD_HEADER, ChunkMeta, Error, Result, XTD_FILE_ID, XTD_VERSION, XtdFile, XtdHeader,
    XtdVisualChunk,
};

mod options;
pub use options::ReadOptions;

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
    ///
    /// # Errors
    ///
    /// Returns an error if the ECF container or a recognized XTD chunk is
    /// invalid or truncated.
    pub fn read(data: &[u8]) -> Result<XtdFile> {
        XtdFile::from_bytes(data)
    }

    /// Read an XTD file with explicit validation controls.
    ///
    /// # Errors
    ///
    /// Returns an error if an enabled validation fails or a recognized chunk
    /// is malformed or truncated.
    pub fn read_with_options(data: &[u8], options: ReadOptions) -> Result<XtdFile> {
        XtdFile::from_bytes_with_options(data, options)
    }
}

impl XtdFile {
    /// Parse an XTD file from a byte slice (ECF container).
    ///
    /// # Errors
    ///
    /// Returns an error if the ECF container or a recognized XTD chunk is
    /// invalid or truncated.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        Self::from_bytes_with_options(data, ReadOptions::strict())
    }

    /// Parse an XTD file while skipping ECF checksum validation.
    ///
    /// # Errors
    ///
    /// Returns an error if the signatures, required chunks, or recognized
    /// chunk structures are invalid.
    pub fn from_bytes_unchecked(data: &[u8]) -> Result<Self> {
        Self::from_bytes_with_options(data, ReadOptions::unchecked_checksums())
    }

    /// Parse an XTD file with explicit validation controls.
    ///
    /// # Errors
    ///
    /// Returns an error if an enabled validation fails or a recognized chunk
    /// is malformed or truncated.
    pub fn from_bytes_with_options(data: &[u8], options: ReadOptions) -> Result<Self> {
        let ecf = ecf::Reader::new_with_options(
            data,
            ecf::ReadOptions {
                validate_magic: options.validate_signatures,
                validate_checksums: options.validate_checksums,
            },
        )?;

        if options.validate_signatures && ecf.header().id != XTD_FILE_ID {
            return Err(Error::InvalidFileId {
                expected: XTD_FILE_ID,
                actual: ecf.header().id,
            });
        }

        let mut file = XtdFile {
            ecf_file_id: ecf.header().id,
            ecf_flags: ecf.header().flags,
            ..Default::default()
        };
        let mut singleton_counts = [0usize; 6];
        let mut header_count = 0usize;

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
                    header_count += 1;
                    file.header = read_header(
                        &chunk_data,
                        options.validate_signatures,
                        options.validate_engine_requirements,
                    )?;
                }
                CHUNK_TERRAIN => {
                    file.visual_chunks.push(read_visual_chunk(
                        &chunk_data,
                        options.validate_engine_requirements,
                    )?);
                }
                CHUNK_ATLAS => {
                    singleton_counts[0] += 1;
                    file.atlas_data = chunk_data;
                }
                CHUNK_TESS => {
                    singleton_counts[1] += 1;
                    file.tess_data = chunk_data;
                }
                CHUNK_LIGHTING => {
                    singleton_counts[2] += 1;
                    file.lighting_data = chunk_data;
                }
                CHUNK_AO => {
                    singleton_counts[3] += 1;
                    file.ao_data = chunk_data;
                }
                CHUNK_ALPHA => {
                    singleton_counts[4] += 1;
                    file.alpha_data = chunk_data;
                }
                _ => {}
            }
        }

        if options.validate_engine_requirements {
            if header_count == 0 {
                return Err(Error::MissingChunk(CHUNK_XTD_HEADER));
            }
            if header_count != 1 || singleton_counts.iter().any(|count| *count > 1) {
                return Err(Error::InvalidChunkData(
                    "Duplicate singleton XTD chunk".into(),
                ));
            }
        }

        Ok(file)
    }
}

fn read_header(data: &[u8], validate_version: bool, require_exact_size: bool) -> Result<XtdHeader> {
    if require_exact_size && data.len() != XtdHeader::SIZE {
        return Err(Error::InvalidHeaderSize {
            expected: XtdHeader::SIZE,
            actual: data.len(),
        });
    }
    let (raw, _): (Ref<_, XtdHeaderRaw>, _) =
        Ref::from_prefix(data).map_err(|_| Error::InvalidHeaderSize {
            expected: XtdHeader::SIZE,
            actual: data.len(),
        })?;

    let version = i32::from_be_bytes(raw.version);
    if validate_version && version != XTD_VERSION {
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

fn read_visual_chunk(data: &[u8], require_exact_size: bool) -> Result<XtdVisualChunk> {
    if require_exact_size && data.len() != XtdVisualChunk::SIZE {
        return Err(Error::InvalidChunkData(format!(
            "Invalid visual chunk size: expected {}, got {}",
            XtdVisualChunk::SIZE,
            data.len()
        )));
    }
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

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use super::*;

    fn test_container(file_id: u32, version: i32) -> Vec<u8> {
        let mut header = Vec::new();
        header.extend_from_slice(&version.to_be_bytes());
        header.extend_from_slice(&64i32.to_be_bytes());
        header.extend_from_slice(&1i32.to_be_bytes());
        header.extend_from_slice(&1.0f32.to_be_bytes());
        header.extend_from_slice(&[0; 24]);
        let mut writer = ecf::Writer::new(file_id);
        writer.add_chunk(CHUNK_XTD_HEADER, header);
        writer.finalize().unwrap()
    }

    #[test]
    fn strict_reader_rejects_bad_signatures() {
        assert!(matches!(
            Reader::read(&test_container(0xDEAD_BEEF, XTD_VERSION)),
            Err(Error::InvalidFileId { .. })
        ));
        assert!(matches!(
            Reader::read(&test_container(XTD_FILE_ID, 99)),
            Err(Error::InvalidVersion { .. })
        ));
    }

    #[test]
    fn permissive_reader_accepts_bad_signatures() {
        let options = ReadOptions::accepting_bad_signatures();
        let bad_id =
            Reader::read_with_options(&test_container(0xDEAD_BEEF, XTD_VERSION), options).unwrap();
        assert_eq!(bad_id.ecf_file_id, 0xDEAD_BEEF);
        let bad_version =
            Reader::read_with_options(&test_container(XTD_FILE_ID, 99), options).unwrap();
        assert_eq!(bad_version.header.version, 99);

        let mut bad_magic = test_container(XTD_FILE_ID, XTD_VERSION);
        bad_magic[..4].copy_from_slice(&0xDEAD_BEEFu32.to_be_bytes());
        Reader::read_with_options(&bad_magic, options).unwrap();
    }
}
