//! Game-compatible XTD writer implementation.

use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

use nostdio::WriteBe;

use crate::{
    CHUNK_ALPHA, CHUNK_AO, CHUNK_ATLAS, CHUNK_LIGHTING, CHUNK_TERRAIN, CHUNK_TESS,
    CHUNK_XTD_HEADER, Error, Result, XTD_FILE_ID, XTD_VERSION, XtdFile, XtdHeader, XtdVisualChunk,
};

const VERTICES_PER_CHUNK_AXIS: usize = 64;
const VERTICES_PER_PATCH_AXIS: usize = 16;

/// XTD file writer.
pub struct Writer;

impl Writer {
    /// Write an XTD file to a byte vector.
    ///
    /// # Errors
    ///
    /// Returns an error if the terrain dimensions, chunk inventory, or raw
    /// texture/tessellation payloads are inconsistent with the retail layout.
    pub fn write(file: &XtdFile) -> Result<Vec<u8>> {
        file.to_bytes()
    }
}

impl XtdFile {
    /// Serialize this XTD file to a game-compatible ECF container.
    ///
    /// # Errors
    ///
    /// Returns an error if the representation is inconsistent, contains an
    /// unsupported chunk, or cannot fit the on-disk integer fields.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        Writer::write_inner(self)
    }
}

impl Writer {
    fn write_inner(file: &XtdFile) -> Result<Vec<u8>> {
        validate_file_layout(file)?;
        let mut ecf = ecf::Writer::new(file.ecf_file_id);
        ecf.set_header_flags(file.ecf_flags);
        let mut visual_index = 0usize;
        let mut counts = [0usize; 7];

        for meta in &file.chunk_order {
            let data = match meta.id {
                CHUNK_XTD_HEADER => {
                    counts[0] += 1;
                    write_header(&file.header)?
                }
                CHUNK_TERRAIN => {
                    counts[1] += 1;
                    let visual = file.visual_chunks.get(visual_index).ok_or_else(|| {
                        Error::InvalidChunkData(
                            "Chunk order contains more visual chunks than values".into(),
                        )
                    })?;
                    visual_index += 1;
                    write_visual_chunk(visual)?
                }
                CHUNK_ATLAS => {
                    counts[2] += 1;
                    file.atlas_data.clone()
                }
                CHUNK_TESS => {
                    counts[3] += 1;
                    file.tess_data.clone()
                }
                CHUNK_LIGHTING => {
                    counts[4] += 1;
                    file.lighting_data.clone()
                }
                CHUNK_AO => {
                    counts[5] += 1;
                    file.ao_data.clone()
                }
                CHUNK_ALPHA => {
                    counts[6] += 1;
                    file.alpha_data.clone()
                }
                id => return Err(Error::UnsupportedChunk(id)),
            };
            ecf.add_chunk_with_metadata(
                meta.id,
                data,
                meta.alignment_log2,
                meta.flags,
                meta.resource_flags,
            )?;
        }

        validate_chunk_inventory(file, counts)?;
        Ok(ecf.finalize()?)
    }
}

fn validate_file_layout(file: &XtdFile) -> Result<()> {
    if file.ecf_file_id != XTD_FILE_ID {
        return Err(Error::InvalidFileId {
            expected: XTD_FILE_ID,
            actual: file.ecf_file_id,
        });
    }
    if file.header.version != XTD_VERSION {
        return Err(Error::InvalidVersion {
            expected: XTD_VERSION,
            actual: file.header.version,
        });
    }
    let vertex_axis = positive_usize(file.header.num_x_verts, "terrain vertex count")?;
    let chunk_axis = positive_usize(file.header.num_x_chunks, "terrain chunk count")?;
    let expected_vertex_axis = chunk_axis
        .checked_mul(VERTICES_PER_CHUNK_AXIS)
        .ok_or(Error::SizeOverflow("terrain vertex count"))?;
    if vertex_axis != expected_vertex_axis {
        return Err(Error::InvalidChunkData(format!(
            "Terrain has {vertex_axis} vertices per axis, but {chunk_axis} chunks require {expected_vertex_axis}"
        )));
    }

    validate_visual_grid(file, chunk_axis)?;
    validate_atlas(file, vertex_axis)?;
    validate_tessellation(file, vertex_axis)?;
    validate_auxiliary_texture("AO", &file.ao_data, vertex_axis)?;
    validate_auxiliary_texture("alpha", &file.alpha_data, vertex_axis)?;
    validate_lighting(&file.lighting_data, vertex_axis)?;
    Ok(())
}

fn positive_usize(value: i32, field: &'static str) -> Result<usize> {
    let value = usize::try_from(value)
        .map_err(|_| Error::InvalidChunkData(format!("Invalid {field}: {value}")))?;
    if value == 0 {
        Err(Error::InvalidChunkData(format!("{field} must be positive")))
    } else {
        Ok(value)
    }
}

fn validate_visual_grid(file: &XtdFile, chunk_axis: usize) -> Result<()> {
    let expected_count = chunk_axis
        .checked_mul(chunk_axis)
        .ok_or(Error::SizeOverflow("visual chunk count"))?;
    if file.visual_chunks.len() != expected_count {
        return Err(Error::InvalidChunkData(format!(
            "Expected {expected_count} visual chunks, got {}",
            file.visual_chunks.len()
        )));
    }
    let mut occupied = vec![false; expected_count];
    for visual in &file.visual_chunks {
        let grid_x = usize::try_from(visual.grid_x)
            .map_err(|_| Error::InvalidChunkData("Negative visual grid X".into()))?;
        let grid_z = usize::try_from(visual.grid_z)
            .map_err(|_| Error::InvalidChunkData("Negative visual grid Z".into()))?;
        if grid_x >= chunk_axis || grid_z >= chunk_axis {
            return Err(Error::InvalidChunkData(format!(
                "Visual grid coordinate ({grid_x}, {grid_z}) is outside {chunk_axis}x{chunk_axis} terrain"
            )));
        }
        let index = grid_x
            .checked_mul(chunk_axis)
            .and_then(|row| row.checked_add(grid_z))
            .ok_or(Error::SizeOverflow("visual grid index"))?;
        if occupied[index] {
            return Err(Error::InvalidChunkData(format!(
                "Duplicate visual grid coordinate ({grid_x}, {grid_z})"
            )));
        }
        occupied[index] = true;
    }
    Ok(())
}

fn validate_atlas(file: &XtdFile, vertex_axis: usize) -> Result<()> {
    let expected = vertex_axis
        .checked_mul(vertex_axis)
        .and_then(|vertices| vertices.checked_mul(8))
        .and_then(|payload| payload.checked_add(crate::AtlasHeader::SIZE))
        .ok_or(Error::SizeOverflow("terrain atlas"))?;
    check_size("atlas", file.atlas_data.len(), expected)
}

fn validate_tessellation(file: &XtdFile, vertex_axis: usize) -> Result<()> {
    let tessellation = file
        .decode_tessellation()?
        .ok_or(Error::MissingChunk(CHUNK_TESS))?;
    if !vertex_axis.is_multiple_of(VERTICES_PER_PATCH_AXIS) {
        return Err(Error::InvalidChunkData(format!(
            "Terrain vertex count {vertex_axis} is not divisible by {VERTICES_PER_PATCH_AXIS}"
        )));
    }
    let expected_axis = vertex_axis / VERTICES_PER_PATCH_AXIS;
    let x_axis = positive_usize(tessellation.num_x_patches, "X patch count")?;
    let z_axis = positive_usize(tessellation.num_z_patches, "Z patch count")?;
    if x_axis != expected_axis || z_axis != expected_axis {
        return Err(Error::InvalidChunkData(format!(
            "Expected {expected_axis}x{expected_axis} tessellation patches, got {x_axis}x{z_axis}"
        )));
    }
    Ok(())
}

fn block_compressed_size(vertex_axis: usize) -> Result<usize> {
    vertex_axis
        .div_ceil(4)
        .checked_mul(vertex_axis.div_ceil(4))
        .and_then(|blocks| blocks.checked_mul(8))
        .ok_or(Error::SizeOverflow("terrain block-compressed texture"))
}

fn validate_auxiliary_texture(context: &'static str, data: &[u8], axis: usize) -> Result<()> {
    if data.is_empty() {
        return Ok(());
    }
    check_size(context, data.len(), block_compressed_size(axis)?)
}

fn validate_lighting(data: &[u8], axis: usize) -> Result<()> {
    if data.is_empty() {
        return Ok(());
    }
    let encoded_size_bytes = data.get(..4).ok_or(Error::UnexpectedEof)?;
    let encoded_size = i32::from_be_bytes(
        encoded_size_bytes
            .try_into()
            .map_err(|_| Error::UnexpectedEof)?,
    );
    let encoded_size = positive_usize(encoded_size, "lighting payload size")?;
    let expected_payload = block_compressed_size(axis)?;
    check_size("lighting payload", encoded_size, expected_payload)?;
    let expected_chunk = expected_payload
        .checked_add(4)
        .ok_or(Error::SizeOverflow("lighting chunk"))?;
    check_size("lighting chunk", data.len(), expected_chunk)
}

fn check_size(context: &'static str, actual: usize, expected: usize) -> Result<()> {
    if actual == expected {
        Ok(())
    } else {
        Err(Error::InvalidChunkData(format!(
            "Invalid {context} size: expected {expected}, got {actual}"
        )))
    }
}

fn validate_chunk_inventory(file: &XtdFile, counts: [usize; 7]) -> Result<()> {
    let [header, visual, atlas, tessellation, lighting, ao, alpha] = counts;
    if header != 1 {
        return Err(if header == 0 {
            Error::MissingChunk(CHUNK_XTD_HEADER)
        } else {
            Error::InvalidChunkData("XTD must contain exactly one header chunk".into())
        });
    }
    if visual != file.visual_chunks.len() {
        return Err(Error::InvalidChunkData(format!(
            "Chunk order contains {visual} visual chunks, but {} values exist",
            file.visual_chunks.len()
        )));
    }
    if atlas != 1 {
        return Err(if atlas == 0 {
            Error::MissingChunk(CHUNK_ATLAS)
        } else {
            Error::InvalidChunkData("XTD must contain exactly one atlas chunk".into())
        });
    }
    if tessellation != 1 {
        return Err(if tessellation == 0 {
            Error::MissingChunk(CHUNK_TESS)
        } else {
            Error::InvalidChunkData("XTD must contain exactly one tessellation chunk".into())
        });
    }
    validate_optional_inventory("lighting", lighting, !file.lighting_data.is_empty())?;
    validate_optional_inventory("AO", ao, !file.ao_data.is_empty())?;
    validate_optional_inventory("alpha", alpha, !file.alpha_data.is_empty())?;
    Ok(())
}

fn validate_optional_inventory(context: &'static str, count: usize, present: bool) -> Result<()> {
    let expected = usize::from(present);
    if count == expected {
        Ok(())
    } else {
        Err(Error::InvalidChunkData(format!(
            "Expected {expected} {context} chunk, found {count}"
        )))
    }
}

fn write_header(header: &XtdHeader) -> Result<Vec<u8>> {
    let mut buffer = Vec::with_capacity(XtdHeader::SIZE);
    buffer.write_i32_be(header.version)?;
    buffer.write_i32_be(header.num_x_verts)?;
    buffer.write_i32_be(header.num_x_chunks)?;
    buffer.write_f32_be(header.tile_scale)?;
    for value in &header.world_min {
        buffer.write_f32_be(*value)?;
    }
    for value in &header.world_max {
        buffer.write_f32_be(*value)?;
    }
    Ok(buffer)
}

fn write_visual_chunk(chunk: &XtdVisualChunk) -> Result<Vec<u8>> {
    let mut buffer = Vec::with_capacity(XtdVisualChunk::SIZE);
    buffer.write_i32_be(chunk.grid_x)?;
    buffer.write_i32_be(chunk.grid_z)?;
    buffer.write_i32_be(chunk.max_v_stride)?;
    for value in &chunk.min {
        buffer.write_f32_be(*value)?;
    }
    for value in &chunk.max {
        buffer.write_f32_be(*value)?;
    }
    buffer.push(u8::from(chunk.can_cast_shadows));
    Ok(buffer)
}
