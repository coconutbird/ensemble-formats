//! XTT reader implementation.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use ecf::Reader as EcfReader;
use nostdio::{Cursor, ReadBe};
use zerocopy::Ref;

use crate::{
    ActiveDecalInfo, ActiveDecalInstance, ActiveTextureInfo, CHUNK_ATLAS_ALBEDO, CHUNK_ATLAS_LINK,
    CHUNK_FOLIAGE_HEADER, CHUNK_FOLIAGE_QN, CHUNK_ROAD, CHUNK_XTT_HEADER, ChunkMeta, Error,
    FoliageQNChunk, FoliageSetInfo, Result, XTT_VERSION, XttFile, XttHeader, XttHeaderRaw,
    XttLinker, XttLinkerHeaderRaw,
};

/// Size of filename strings in XTT files.
const XTT_FILENAME_SIZE: usize = 256;

/// Helper: read a big-endian i32 from a slice at the given offset.
#[inline]
fn read_i32_be(data: &[u8], offset: usize) -> Result<i32> {
    let mut cursor = Cursor::new(data.get(offset..).ok_or(Error::UnexpectedEof)?);
    Ok(cursor.read_i32_be()?)
}

/// Helper: read a big-endian u32 from a slice at the given offset.
#[inline]
fn read_u32_be(data: &[u8], offset: usize) -> Result<u32> {
    let mut cursor = Cursor::new(data.get(offset..).ok_or(Error::UnexpectedEof)?);
    Ok(cursor.read_u32_be()?)
}

/// Helper: read a big-endian f32 from a slice at the given offset.
#[inline]
fn read_f32_be(data: &[u8], offset: usize) -> Result<f32> {
    let mut cursor = Cursor::new(data.get(offset..).ok_or(Error::UnexpectedEof)?);
    Ok(cursor.read_f32_be()?)
}

/// Helper: read a null-terminated string from a fixed-size byte region.
fn read_filename(data: &[u8], offset: usize, max_len: usize) -> Result<String> {
    let end = offset
        .checked_add(max_len)
        .ok_or(Error::SizeOverflow("filename range"))?;
    let region = data.get(offset..end).ok_or(Error::UnexpectedEof)?;
    let name_end = region
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(region.len());
    Ok(String::from_utf8_lossy(&region[..name_end]).into_owned())
}

fn nonnegative_count(value: i32, field: &'static str) -> Result<usize> {
    usize::try_from(value).map_err(|_| Error::InvalidChunkData(format!("Invalid {field}: {value}")))
}

fn checked_slice(data: &[u8], offset: usize, len: usize) -> Result<&[u8]> {
    let end = offset
        .checked_add(len)
        .ok_or(Error::SizeOverflow("chunk field range"))?;
    data.get(offset..end).ok_or(Error::UnexpectedEof)
}

fn advance_offset(offset: &mut usize, amount: usize) -> Result<()> {
    *offset = offset
        .checked_add(amount)
        .ok_or(Error::SizeOverflow("chunk offset"))?;
    Ok(())
}

fn linker_alpha_size(layer_count: usize) -> Result<usize> {
    let aligned_layers = ((layer_count - 1) >> 2) + 1;
    aligned_layers
        .checked_mul(XttLinker::ALPHA_TEXTURE_WIDTH)
        .and_then(|size| size.checked_mul(XttLinker::ALPHA_TEXTURE_HEIGHT))
        .and_then(|size| size.checked_mul(XttLinker::ALPHA_BPP))
        .map(|bits| bits >> 3)
        .ok_or(Error::SizeOverflow("linker alpha payload"))
}

/// XTT file reader.
pub struct Reader;

impl Reader {
    /// Read an XTT file from a byte slice.
    ///
    /// # Errors
    ///
    /// Returns an error if the ECF container or any recognized XTT chunk is
    /// invalid or truncated.
    pub fn read(data: &[u8]) -> Result<XttFile> {
        XttFile::from_bytes(data)
    }
}

impl XttFile {
    /// Parse an XTT file from a byte slice (ECF container).
    ///
    /// # Errors
    ///
    /// Returns an error if the ECF container or any recognized XTT chunk is
    /// invalid or truncated.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let ecf = EcfReader::new(data)?;

        let mut file = XttFile {
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
                CHUNK_XTT_HEADER => {
                    file.header = read_header(&chunk_data)?;
                    if chunk_data.len() > XttHeader::SIZE {
                        file.header_extra = chunk_data[XttHeader::SIZE..].to_vec();
                    }
                    parse_header_extra(&mut file)?;
                }
                CHUNK_ATLAS_LINK => {
                    file.linkers.push(read_linker(&chunk_data)?);
                }
                CHUNK_ATLAS_ALBEDO => {
                    file.albedo_data = chunk_data;
                }
                CHUNK_ROAD => {
                    file.road_data = chunk_data;
                }
                CHUNK_FOLIAGE_HEADER => {
                    file.foliage.sets = read_foliage_header(&chunk_data)?;
                }
                CHUNK_FOLIAGE_QN => {
                    file.foliage
                        .qn_chunks
                        .push(read_foliage_qn_chunk(&chunk_data)?);
                }
                _ => {}
            }
        }

        Ok(file)
    }
}

fn read_header(data: &[u8]) -> Result<XttHeader> {
    let (raw, _): (Ref<_, XttHeaderRaw>, _) =
        Ref::from_prefix(data).map_err(|_| Error::InvalidHeaderSize {
            expected: XttHeader::SIZE,
            actual: data.len(),
        })?;

    let version = i32::from_be_bytes(raw.version);
    if version != XTT_VERSION {
        return Err(Error::InvalidVersion {
            expected: XTT_VERSION,
            actual: version,
        });
    }

    Ok(XttHeader {
        version,
        num_active_textures: i32::from_be_bytes(raw.num_active_textures),
        num_active_decals: i32::from_be_bytes(raw.num_active_decals),
        num_active_decal_instances: i32::from_be_bytes(raw.num_active_decal_instances),
    })
}

fn read_linker(data: &[u8]) -> Result<XttLinker> {
    let (raw, _): (Ref<_, XttLinkerHeaderRaw>, _) = Ref::from_prefix(data).map_err(|_| {
        Error::InvalidChunkData(format!(
            "Linker chunk too small: {} < {}",
            data.len(),
            XttLinker::HEADER_SIZE
        ))
    })?;

    let grid_x = i32::from_be_bytes(raw.grid_x);
    let grid_z = i32::from_be_bytes(raw.grid_z);
    let spec_pass_needed = i32::from_be_bytes(raw.spec_pass_needed);
    let self_pass_needed = i32::from_be_bytes(raw.self_pass_needed);
    let env_mask_pass_needed = i32::from_be_bytes(raw.env_mask_pass_needed);
    let alpha_pass_needed = i32::from_be_bytes(raw.alpha_pass_needed);
    let is_fully_opaque = i32::from_be_bytes(raw.is_fully_opaque);
    let num_splat_layers = i32::from_be_bytes(raw.num_splat_layers);
    let num_decal_layers = i32::from_be_bytes(raw.num_decal_layers);
    let splat_layer_count = nonnegative_count(num_splat_layers, "splat layer count")?;
    let decal_layer_count = nonnegative_count(num_decal_layers, "decal layer count")?;
    let mut off = XttLinker::HEADER_SIZE;

    // Parse splat layer IDs
    let mut splat_layer_ids = Vec::with_capacity(splat_layer_count);
    for _ in 0..splat_layer_count {
        splat_layer_ids.push(read_i32_be(data, off)?);
        advance_offset(&mut off, 4)?;
    }

    // Parse splat alpha data (if more than 1 layer)
    let splat_alpha_data = if splat_layer_count > 1 {
        let mem_size = linker_alpha_size(splat_layer_count)?;
        let alpha_data = checked_slice(data, off, mem_size)?.to_vec();
        advance_offset(&mut off, mem_size)?;
        alpha_data
    } else {
        Vec::new()
    };

    // Parse decal layer IDs (if any decal layers)
    let mut decal_layer_ids = Vec::new();
    let mut decal_alpha_data = Vec::new();

    if decal_layer_count > 0 {
        decal_layer_ids.reserve(decal_layer_count);
        for _ in 0..decal_layer_count {
            decal_layer_ids.push(read_i32_be(data, off)?);
            advance_offset(&mut off, 4)?;
        }

        let mem_size = linker_alpha_size(decal_layer_count)?;
        decal_alpha_data = checked_slice(data, off, mem_size)?.to_vec();
    }

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
        splat_layer_ids,
        splat_alpha_data,
        decal_layer_ids,
        decal_alpha_data,
    })
}

/// Parse `header_extra` data to extract active textures, decals, and decal instances.
fn parse_header_extra(file: &mut XttFile) -> Result<()> {
    if file.header_extra.is_empty() {
        return Ok(());
    }

    let data = &file.header_extra;
    let mut off = 0;

    // Parse active textures
    let active_texture_count =
        nonnegative_count(file.header.num_active_textures, "active texture count")?;
    for _ in 0..active_texture_count {
        let (tex, consumed) = read_active_texture(data, off)?;
        file.active_textures.push(tex);
        advance_offset(&mut off, consumed)?;
    }

    // Parse active decals
    let active_decal_count =
        nonnegative_count(file.header.num_active_decals, "active decal count")?;
    for _ in 0..active_decal_count {
        let filename = read_filename(data, off, XTT_FILENAME_SIZE)?;
        advance_offset(&mut off, XTT_FILENAME_SIZE)?;
        file.active_decals.push(ActiveDecalInfo { filename });
    }

    // Parse decal instances
    let decal_instance_count = nonnegative_count(
        file.header.num_active_decal_instances,
        "active decal instance count",
    )?;
    for _ in 0..decal_instance_count {
        let active_decal_index = read_i32_be(data, off)?;
        advance_offset(&mut off, 4)?;
        let rotation = read_f32_be(data, off)?;
        advance_offset(&mut off, 4)?;
        let tile_center_x = read_f32_be(data, off)?;
        advance_offset(&mut off, 4)?;
        let tile_center_y = read_f32_be(data, off)?;
        advance_offset(&mut off, 4)?;
        let u_scale = read_f32_be(data, off)?;
        advance_offset(&mut off, 4)?;
        let v_scale = read_f32_be(data, off)?;
        advance_offset(&mut off, 4)?;
        file.decal_instances.push(ActiveDecalInstance {
            active_decal_index,
            rotation,
            tile_center_x,
            tile_center_y,
            u_scale,
            v_scale,
        });
    }

    Ok(())
}

/// Read a single active texture from the data at the given offset.
/// Returns (texture, `bytes_consumed`).
fn read_active_texture(data: &[u8], off: usize) -> Result<(ActiveTextureInfo, usize)> {
    let filename = read_filename(data, off, XTT_FILENAME_SIZE)?;
    let mut pos = off
        .checked_add(XTT_FILENAME_SIZE)
        .ok_or(Error::SizeOverflow("active texture offset"))?;
    let u_scale = read_i32_be(data, pos)?;
    advance_offset(&mut pos, 4)?;
    let v_scale = read_i32_be(data, pos)?;
    advance_offset(&mut pos, 4)?;
    let blend_op = read_i32_be(data, pos)?;
    advance_offset(&mut pos, 4)?;

    Ok((
        ActiveTextureInfo {
            filename,
            u_scale,
            v_scale,
            blend_op,
        },
        pos - off,
    ))
}

/// Parse the foliage header chunk.
fn read_foliage_header(data: &[u8]) -> Result<Vec<FoliageSetInfo>> {
    if data.len() < 4 {
        return Ok(Vec::new());
    }

    let num_sets = usize::try_from(read_u32_be(data, 0)?)
        .map_err(|_| Error::SizeOverflow("foliage set count"))?;
    let mut off = 4;
    let mut sets = Vec::with_capacity(num_sets);

    for _ in 0..num_sets {
        let filename = read_filename(data, off, XTT_FILENAME_SIZE)?;
        advance_offset(&mut off, XTT_FILENAME_SIZE)?;
        sets.push(FoliageSetInfo { filename });
    }

    Ok(sets)
}

/// Parse a foliage QN (quad-node) chunk.
fn read_foliage_qn_chunk(data: &[u8]) -> Result<FoliageQNChunk> {
    if data.len() < 8 {
        return Err(Error::InvalidChunkData("Foliage QN chunk too small".into()));
    }

    let mut off = 0;
    let qn_parent_index = read_u32_be(data, off)?;
    advance_offset(&mut off, 4)?;
    let num_sets = read_u32_be(data, off)?;
    advance_offset(&mut off, 4)?;
    let set_count =
        usize::try_from(num_sets).map_err(|_| Error::SizeOverflow("foliage set count"))?;

    let mut set_indices = Vec::with_capacity(set_count);
    for _ in 0..set_count {
        set_indices.push(read_i32_be(data, off)?);
        advance_offset(&mut off, 4)?;
    }

    let mut set_poly_counts = Vec::with_capacity(set_count);
    for _ in 0..set_count {
        set_poly_counts.push(read_i32_be(data, off)?);
        advance_offset(&mut off, 4)?;
    }

    let _total_physical_memory = read_i32_be(data, off)?;
    advance_offset(&mut off, 4)?;

    let mut index_buffer_sizes = Vec::with_capacity(set_count);
    for _ in 0..set_count {
        let size = nonnegative_count(read_i32_be(data, off)?, "foliage index buffer size")?;
        index_buffer_sizes.push(size);
        advance_offset(&mut off, 4)?;
    }

    let mut index_buffers = Vec::with_capacity(set_count);
    for size in index_buffer_sizes {
        index_buffers.push(checked_slice(data, off, size)?.to_vec());
        advance_offset(&mut off, size)?;
    }

    Ok(FoliageQNChunk {
        qn_parent_index,
        num_sets,
        set_indices,
        set_poly_counts,
        index_buffers,
    })
}
