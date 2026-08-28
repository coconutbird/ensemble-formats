//! Game-compatible XTT writer implementation.

use alloc::format;
use alloc::vec::Vec;

use ecf::Writer as EcfWriter;
use nostdio::WriteBe;

use crate::{
    CHUNK_ATLAS_ALBEDO, CHUNK_ATLAS_LINK, CHUNK_FOLIAGE_HEADER, CHUNK_FOLIAGE_QN, CHUNK_ROAD,
    CHUNK_XTT_HEADER, Error, FILENAME_SIZE, FoliageQNChunk, Result, XTT_FILE_ID, XTT_VERSION,
    XttFile, XttFoliage, XttLinker,
};

/// XTT file writer.
pub struct Writer;

impl Writer {
    /// Write an XTT file to a byte vector.
    ///
    /// Header counts are derived from the typed texture/decal collections. The
    /// writer rejects values, chunk inventories, or payload lengths that the
    /// retail loader could not consume safely.
    ///
    /// # Errors
    ///
    /// Returns an error if the representation is not game-compatible, a value
    /// is too large for the format, or the ECF container cannot be finalized.
    pub fn write(file: &XttFile) -> Result<Vec<u8>> {
        file.to_bytes()
    }
}

impl XttFile {
    /// Serialize this XTT file to a game-compatible ECF container.
    ///
    /// # Errors
    ///
    /// Returns an error if the representation is inconsistent or cannot be
    /// encoded without truncation.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        Writer::write_inner(self)
    }
}

impl Writer {
    fn write_inner(file: &XttFile) -> Result<Vec<u8>> {
        validate_file_signature(file)?;
        validate_game_relationships(file)?;
        let mut ecf = EcfWriter::new(file.ecf_file_id);
        ecf.set_header_flags(file.ecf_flags);

        let mut header_count = 0usize;
        let mut linker_index = 0usize;
        let mut albedo_count = 0usize;
        let mut road_count = 0usize;
        let mut foliage_header_count = 0usize;
        let mut foliage_qn_index = 0usize;

        for meta in &file.chunk_order {
            let data = match meta.id {
                CHUNK_XTT_HEADER => {
                    header_count += 1;
                    write_header_chunk(file)?
                }
                CHUNK_ATLAS_LINK => {
                    let linker = file.linkers.get(linker_index).ok_or_else(|| {
                        Error::InvalidChunkData(
                            "Chunk order contains more linker chunks than linker values".into(),
                        )
                    })?;
                    linker_index += 1;
                    write_linker(linker)?
                }
                CHUNK_ATLAS_ALBEDO => {
                    albedo_count += 1;
                    if file.albedo_data.is_empty() {
                        return Err(Error::InvalidChunkData(
                            "Albedo chunk is present but its payload is empty".into(),
                        ));
                    }
                    crate::decode::validate_albedo_data(&file.albedo_data)?;
                    file.albedo_data.clone()
                }
                CHUNK_ROAD => {
                    road_count += 1;
                    if file.road_data.is_empty() {
                        return Err(Error::InvalidChunkData(
                            "Road chunk is present but its payload is empty".into(),
                        ));
                    }
                    crate::decode_road_data(&file.road_data)?;
                    file.road_data.clone()
                }
                CHUNK_FOLIAGE_HEADER => {
                    foliage_header_count += 1;
                    write_foliage_header(&file.foliage)?
                }
                CHUNK_FOLIAGE_QN => {
                    let qn = file
                        .foliage
                        .qn_chunks
                        .get(foliage_qn_index)
                        .ok_or_else(|| {
                            Error::InvalidChunkData(
                                "Chunk order contains more foliage QN chunks than values".into(),
                            )
                        })?;
                    foliage_qn_index += 1;
                    write_foliage_qn_chunk(qn)?
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

        validate_chunk_inventory(
            file,
            [
                header_count,
                linker_index,
                albedo_count,
                road_count,
                foliage_header_count,
                foliage_qn_index,
            ],
        )?;
        Ok(ecf.finalize()?)
    }
}

fn validate_file_signature(file: &XttFile) -> Result<()> {
    if file.ecf_file_id != XTT_FILE_ID {
        return Err(Error::InvalidFileId {
            expected: XTT_FILE_ID,
            actual: file.ecf_file_id,
        });
    }
    if file.header.version != XTT_VERSION {
        return Err(Error::InvalidVersion {
            expected: XTT_VERSION,
            actual: file.header.version,
        });
    }
    Ok(())
}

fn validate_game_relationships(file: &XttFile) -> Result<()> {
    let linker_axis = validate_linker_grid(file)?;
    validate_linker_references(file)?;
    validate_albedo_relationship(file, linker_axis)?;
    validate_decal_and_foliage_references(file)
}

fn validate_linker_grid(file: &XttFile) -> Result<usize> {
    if file.linkers.is_empty() {
        return Err(Error::InvalidChunkData(
            "A game terrain XTT must contain linker chunks".into(),
        ));
    }
    let mut maximum_x = 0usize;
    let mut maximum_z = 0usize;
    for linker in &file.linkers {
        let grid_x = usize::try_from(linker.grid_x)
            .map_err(|_| Error::InvalidChunkData("Negative linker grid X".into()))?;
        let grid_z = usize::try_from(linker.grid_z)
            .map_err(|_| Error::InvalidChunkData("Negative linker grid Z".into()))?;
        maximum_x = maximum_x.max(grid_x);
        maximum_z = maximum_z.max(grid_z);
    }
    if maximum_x != maximum_z {
        return Err(Error::InvalidChunkData(format!(
            "Linker grid is not square: maximum coordinates are ({maximum_x}, {maximum_z})"
        )));
    }
    let linker_axis = maximum_x
        .checked_add(1)
        .ok_or(Error::SizeOverflow("linker grid axis"))?;
    let expected_linkers = linker_axis
        .checked_mul(linker_axis)
        .ok_or(Error::SizeOverflow("linker count"))?;
    if file.linkers.len() != expected_linkers {
        return Err(Error::InvalidChunkData(format!(
            "Expected {expected_linkers} linkers for a {linker_axis}x{linker_axis} grid, got {}",
            file.linkers.len()
        )));
    }
    let mut occupied = alloc::vec![false; expected_linkers];
    for linker in &file.linkers {
        let grid_x = usize::try_from(linker.grid_x)
            .map_err(|_| Error::InvalidChunkData("Negative linker grid X".into()))?;
        let grid_z = usize::try_from(linker.grid_z)
            .map_err(|_| Error::InvalidChunkData("Negative linker grid Z".into()))?;
        let index = grid_x
            .checked_mul(linker_axis)
            .and_then(|row| row.checked_add(grid_z))
            .ok_or(Error::SizeOverflow("linker grid index"))?;
        if occupied[index] {
            return Err(Error::InvalidChunkData(format!(
                "Duplicate linker grid coordinate ({grid_x}, {grid_z})"
            )));
        }
        occupied[index] = true;
    }
    Ok(linker_axis)
}

fn validate_linker_references(file: &XttFile) -> Result<()> {
    for linker in &file.linkers {
        for id in &linker.splat_layer_ids {
            let index = usize::try_from(*id).map_err(|_| {
                Error::InvalidChunkData(format!("Negative active texture index {id}"))
            })?;
            if index >= file.active_textures.len() {
                return Err(Error::InvalidChunkData(format!(
                    "Active texture index {index} exceeds {} textures",
                    file.active_textures.len()
                )));
            }
        }
        for id in &linker.decal_layer_ids {
            let index = usize::try_from(*id).map_err(|_| {
                Error::InvalidChunkData(format!("Negative decal instance index {id}"))
            })?;
            if index >= file.decal_instances.len() {
                return Err(Error::InvalidChunkData(format!(
                    "Decal instance index {index} exceeds {} instances",
                    file.decal_instances.len()
                )));
            }
        }
    }
    Ok(())
}

fn validate_albedo_relationship(file: &XttFile, linker_axis: usize) -> Result<()> {
    if file.albedo_data.is_empty() {
        return Err(Error::MissingChunk(CHUNK_ATLAS_ALBEDO));
    }
    let albedo = crate::decode::validate_albedo_data(&file.albedo_data)?;
    let expected_albedo_axis = linker_axis
        .checked_mul(128)
        .ok_or(Error::SizeOverflow("albedo dimensions"))?;
    let albedo_width = usize::try_from(albedo.width)
        .map_err(|_| Error::InvalidChunkData("Negative albedo width".into()))?;
    let albedo_height = usize::try_from(albedo.height)
        .map_err(|_| Error::InvalidChunkData("Negative albedo height".into()))?;
    if albedo_width != expected_albedo_axis || albedo_height != expected_albedo_axis {
        return Err(Error::InvalidChunkData(format!(
            "A {linker_axis}x{linker_axis} linker grid requires a {expected_albedo_axis}x{expected_albedo_axis} albedo, got {albedo_width}x{albedo_height}"
        )));
    }
    Ok(())
}

fn validate_decal_and_foliage_references(file: &XttFile) -> Result<()> {
    for instance in &file.decal_instances {
        let index = usize::try_from(instance.active_decal_index).map_err(|_| {
            Error::InvalidChunkData(format!(
                "Negative active decal index {}",
                instance.active_decal_index
            ))
        })?;
        if index >= file.active_decals.len() {
            return Err(Error::InvalidChunkData(format!(
                "Active decal index {index} exceeds {} decals",
                file.active_decals.len()
            )));
        }
    }
    for qn in &file.foliage.qn_chunks {
        for index in &qn.set_indices {
            let index = usize::try_from(*index).map_err(|_| {
                Error::InvalidChunkData(format!("Negative foliage set index {index}"))
            })?;
            if index >= file.foliage.sets.len() {
                return Err(Error::InvalidChunkData(format!(
                    "Foliage set index {index} exceeds {} sets",
                    file.foliage.sets.len()
                )));
            }
        }
        if qn.index_buffers.iter().any(|buffer| buffer.len() % 4 != 0) {
            return Err(Error::InvalidChunkData(
                "Foliage index buffers must contain complete u32 indices".into(),
            ));
        }
    }
    Ok(())
}

fn validate_chunk_inventory(file: &XttFile, counts: [usize; 6]) -> Result<()> {
    let [
        header_count,
        linker_count,
        albedo_count,
        road_count,
        foliage_header_count,
        foliage_qn_count,
    ] = counts;
    if header_count != 1 {
        return Err(if header_count == 0 {
            Error::MissingChunk(CHUNK_XTT_HEADER)
        } else {
            Error::InvalidChunkData("XTT must contain exactly one header chunk".into())
        });
    }
    if linker_count != file.linkers.len() {
        return Err(Error::InvalidChunkData(format!(
            "Chunk order contains {linker_count} linkers, but {} linker values exist",
            file.linkers.len()
        )));
    }
    if albedo_count > 1 || road_count > 1 || foliage_header_count > 1 {
        return Err(Error::InvalidChunkData(
            "XTT contains a duplicate singleton chunk".into(),
        ));
    }
    if !file.albedo_data.is_empty() && albedo_count == 0 {
        return Err(Error::MissingChunk(CHUNK_ATLAS_ALBEDO));
    }
    if !file.road_data.is_empty() && road_count == 0 {
        return Err(Error::MissingChunk(CHUNK_ROAD));
    }
    if !file.foliage.sets.is_empty() && foliage_header_count == 0 {
        return Err(Error::MissingChunk(CHUNK_FOLIAGE_HEADER));
    }
    if foliage_qn_count != file.foliage.qn_chunks.len() {
        return Err(Error::InvalidChunkData(format!(
            "Chunk order contains {foliage_qn_count} foliage QN chunks, but {} values exist",
            file.foliage.qn_chunks.len()
        )));
    }
    Ok(())
}

fn write_header_chunk(file: &XttFile) -> Result<Vec<u8>> {
    let texture_count = i32::try_from(file.active_textures.len())
        .map_err(|_| Error::SizeOverflow("active texture count"))?;
    let decal_count = i32::try_from(file.active_decals.len())
        .map_err(|_| Error::SizeOverflow("active decal count"))?;
    let instance_count = i32::try_from(file.decal_instances.len())
        .map_err(|_| Error::SizeOverflow("active decal instance count"))?;
    let capacity = file
        .active_textures
        .len()
        .checked_mul(crate::ActiveTextureInfo::SIZE)
        .and_then(|size| {
            file.active_decals
                .len()
                .checked_mul(crate::ActiveDecalInfo::SIZE)
                .and_then(|decals| size.checked_add(decals))
        })
        .and_then(|size| {
            file.decal_instances
                .len()
                .checked_mul(crate::ActiveDecalInstance::SIZE)
                .and_then(|instances| size.checked_add(instances))
        })
        .and_then(|size| size.checked_add(file.header_extra.len()))
        .and_then(|size| size.checked_add(crate::XttHeader::SIZE))
        .ok_or(Error::SizeOverflow("XTT header chunk"))?;
    let mut buffer = Vec::with_capacity(capacity);
    buffer.write_i32_be(XTT_VERSION)?;
    buffer.write_i32_be(texture_count)?;
    buffer.write_i32_be(decal_count)?;
    buffer.write_i32_be(instance_count)?;

    for texture in &file.active_textures {
        buffer.extend_from_slice(&fixed_filename(
            &texture.filename,
            "active texture filename",
        )?);
        buffer.write_i32_be(texture.u_scale)?;
        buffer.write_i32_be(texture.v_scale)?;
        buffer.write_i32_be(texture.blend_op)?;
    }
    for decal in &file.active_decals {
        buffer.extend_from_slice(&fixed_filename(&decal.filename, "active decal filename")?);
    }
    for instance in &file.decal_instances {
        buffer.write_i32_be(instance.active_decal_index)?;
        buffer.write_f32_be(instance.rotation)?;
        buffer.write_f32_be(instance.tile_center_x)?;
        buffer.write_f32_be(instance.tile_center_y)?;
        buffer.write_f32_be(instance.u_scale)?;
        buffer.write_f32_be(instance.v_scale)?;
    }
    buffer.extend_from_slice(&file.header_extra);
    Ok(buffer)
}

fn fixed_filename(value: &str, context: &'static str) -> Result<[u8; FILENAME_SIZE]> {
    let bytes = value.as_bytes();
    let maximum = FILENAME_SIZE - 1;
    if bytes.len() > maximum {
        return Err(Error::StringTooLong {
            context,
            max: maximum,
            actual: bytes.len(),
        });
    }
    if bytes.contains(&0) {
        return Err(Error::InvalidChunkData(format!(
            "{context} contains an embedded NUL byte"
        )));
    }
    let mut encoded = [0; FILENAME_SIZE];
    encoded[..bytes.len()].copy_from_slice(bytes);
    Ok(encoded)
}

fn linker_alpha_size(layer_count: usize) -> Result<usize> {
    let slices = ((layer_count - 1) >> 2) + 1;
    slices
        .checked_mul(XttLinker::ALPHA_TEXTURE_WIDTH)
        .and_then(|size| size.checked_mul(XttLinker::ALPHA_TEXTURE_HEIGHT))
        .and_then(|size| size.checked_mul(XttLinker::ALPHA_BPP / 8))
        .ok_or(Error::SizeOverflow("linker alpha payload"))
}

fn write_linker(linker: &XttLinker) -> Result<Vec<u8>> {
    let splat_count = usize::try_from(linker.num_splat_layers)
        .map_err(|_| Error::InvalidChunkData("Negative splat layer count".into()))?;
    let decal_count = usize::try_from(linker.num_decal_layers)
        .map_err(|_| Error::InvalidChunkData("Negative decal layer count".into()))?;
    if linker.splat_layer_ids.len() != splat_count {
        return Err(Error::InvalidChunkData(format!(
            "Splat layer count is {splat_count}, but {} IDs exist",
            linker.splat_layer_ids.len()
        )));
    }
    if linker.decal_layer_ids.len() != decal_count {
        return Err(Error::InvalidChunkData(format!(
            "Decal layer count is {decal_count}, but {} IDs exist",
            linker.decal_layer_ids.len()
        )));
    }
    let expected_splat_alpha = if splat_count > 1 {
        linker_alpha_size(splat_count)?
    } else {
        0
    };
    let expected_decal_alpha = if decal_count > 0 {
        linker_alpha_size(decal_count)?
    } else {
        0
    };
    check_payload_size(
        "splat alpha",
        linker.splat_alpha_data.len(),
        expected_splat_alpha,
    )?;
    check_payload_size(
        "decal alpha",
        linker.decal_alpha_data.len(),
        expected_decal_alpha,
    )?;

    let total_size = splat_count
        .checked_mul(4)
        .and_then(|size| size.checked_add(expected_splat_alpha))
        .and_then(|size| {
            decal_count
                .checked_mul(4)
                .and_then(|ids| size.checked_add(ids))
        })
        .and_then(|size| size.checked_add(expected_decal_alpha))
        .and_then(|size| size.checked_add(XttLinker::HEADER_SIZE))
        .ok_or(Error::SizeOverflow("linker chunk"))?;
    let mut buffer = Vec::with_capacity(total_size);
    buffer.write_i32_be(linker.grid_x)?;
    buffer.write_i32_be(linker.grid_z)?;
    buffer.write_i32_be(linker.spec_pass_needed)?;
    buffer.write_i32_be(linker.self_pass_needed)?;
    buffer.write_i32_be(linker.env_mask_pass_needed)?;
    buffer.write_i32_be(linker.alpha_pass_needed)?;
    buffer.write_i32_be(linker.is_fully_opaque)?;
    buffer.write_i32_be(linker.num_splat_layers)?;
    buffer.write_i32_be(linker.num_decal_layers)?;
    for id in &linker.splat_layer_ids {
        buffer.write_i32_be(*id)?;
    }
    buffer.extend_from_slice(&linker.splat_alpha_data);
    for id in &linker.decal_layer_ids {
        buffer.write_i32_be(*id)?;
    }
    buffer.extend_from_slice(&linker.decal_alpha_data);
    Ok(buffer)
}

fn check_payload_size(context: &'static str, actual: usize, expected: usize) -> Result<()> {
    if actual == expected {
        Ok(())
    } else {
        Err(Error::InvalidChunkData(format!(
            "Invalid {context} size: expected {expected}, got {actual}"
        )))
    }
}

fn write_foliage_header(foliage: &XttFoliage) -> Result<Vec<u8>> {
    let encoded_count =
        u32::try_from(foliage.sets.len()).map_err(|_| Error::SizeOverflow("foliage set count"))?;
    let capacity = foliage
        .sets
        .len()
        .checked_mul(FILENAME_SIZE)
        .and_then(|size| size.checked_add(4))
        .ok_or(Error::SizeOverflow("foliage header"))?;
    let mut buffer = Vec::with_capacity(capacity);
    buffer.write_u32_be(encoded_count)?;
    for set in &foliage.sets {
        buffer.extend_from_slice(&fixed_filename(&set.filename, "foliage set filename")?);
    }
    Ok(buffer)
}

fn write_foliage_qn_chunk(qn: &FoliageQNChunk) -> Result<Vec<u8>> {
    let set_count =
        usize::try_from(qn.num_sets).map_err(|_| Error::SizeOverflow("foliage set count"))?;
    if qn.set_indices.len() != set_count
        || qn.set_poly_counts.len() != set_count
        || qn.index_buffers.len() != set_count
    {
        return Err(Error::InvalidChunkData(format!(
            "Foliage QN declares {set_count} sets but parallel lengths are {}/{}/{}",
            qn.set_indices.len(),
            qn.set_poly_counts.len(),
            qn.index_buffers.len()
        )));
    }

    let encoded_sizes: Vec<i32> = qn
        .index_buffers
        .iter()
        .map(|buffer| {
            i32::try_from(buffer.len()).map_err(|_| Error::SizeOverflow("foliage index buffer"))
        })
        .collect::<Result<_>>()?;
    let total_physical_memory = encoded_sizes.iter().try_fold(0i32, |total, size| {
        total
            .checked_add(*size)
            .ok_or(Error::SizeOverflow("foliage index buffers"))
    })?;
    let total_index_size = qn.index_buffers.iter().try_fold(0usize, |total, buffer| {
        total
            .checked_add(buffer.len())
            .ok_or(Error::SizeOverflow("foliage index buffers"))
    })?;
    let total_size = set_count
        .checked_mul(12)
        .and_then(|tables| tables.checked_add(12))
        .and_then(|size| size.checked_add(total_index_size))
        .ok_or(Error::SizeOverflow("foliage QN chunk"))?;

    let mut output = Vec::with_capacity(total_size);
    output.write_u32_be(qn.qn_parent_index)?;
    output.write_u32_be(qn.num_sets)?;
    for index in &qn.set_indices {
        output.write_i32_be(*index)?;
    }
    for count in &qn.set_poly_counts {
        output.write_i32_be(*count)?;
    }
    output.write_i32_be(total_physical_memory)?;
    for size in encoded_sizes {
        output.write_i32_be(size)?;
    }
    for buffer in &qn.index_buffers {
        output.extend_from_slice(buffer);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use alloc::string::String;
    use alloc::vec;

    use super::*;
    use crate::{ActiveTextureInfo, ChunkMeta, XttHeader};

    fn metadata(id: u64) -> ChunkMeta {
        ChunkMeta {
            id,
            alignment_log2: 4,
            flags: 0,
            resource_flags: 0,
        }
    }

    fn test_file() -> XttFile {
        let mut albedo_data = Vec::new();
        albedo_data.extend_from_slice(&8192i32.to_be_bytes());
        albedo_data.extend_from_slice(&128i32.to_be_bytes());
        albedo_data.extend_from_slice(&128i32.to_be_bytes());
        albedo_data.extend_from_slice(&1i32.to_be_bytes());
        albedo_data.extend_from_slice(&vec![0; 8192]);
        XttFile {
            ecf_file_id: XTT_FILE_ID,
            ecf_flags: 0,
            chunk_order: vec![
                metadata(CHUNK_XTT_HEADER),
                metadata(CHUNK_ATLAS_LINK),
                metadata(CHUNK_ATLAS_ALBEDO),
            ],
            header: XttHeader {
                version: XTT_VERSION,
                ..XttHeader::default()
            },
            header_extra: vec![0xAA, 0xBB],
            active_textures: vec![ActiveTextureInfo {
                filename: "terrain/base".into(),
                u_scale: 1,
                v_scale: 1,
                blend_op: 0,
            }],
            active_decals: Vec::new(),
            decal_instances: Vec::new(),
            linkers: vec![XttLinker {
                grid_x: 0,
                grid_z: 0,
                num_splat_layers: 1,
                splat_layer_ids: vec![0],
                ..XttLinker::default()
            }],
            albedo_data,
            road_data: Vec::new(),
            foliage: XttFoliage::default(),
        }
    }

    #[test]
    fn writer_rebuilds_typed_header_tables_and_counts() {
        let mut file = test_file();
        file.header.num_active_textures = 99;
        file.active_textures.push(ActiveTextureInfo {
            filename: "terrain/detail".into(),
            u_scale: 2,
            v_scale: 3,
            blend_op: 4,
        });
        let bytes = Writer::write(&file).unwrap();
        let parsed = crate::Reader::read(&bytes).unwrap();
        assert_eq!(parsed.header.num_active_textures, 2);
        assert_eq!(parsed.active_textures.len(), 2);
        assert_eq!(parsed.active_textures[1].filename, "terrain/detail");
        assert_eq!(parsed.header_extra, [0xAA, 0xBB]);
    }

    #[test]
    fn writer_rejects_filename_truncation() {
        let mut file = test_file();
        file.active_textures[0].filename = String::from_iter(core::iter::repeat_n('x', 256));
        assert!(matches!(
            Writer::write(&file),
            Err(Error::StringTooLong { .. })
        ));
    }

    #[test]
    fn writer_rejects_bad_signatures_and_unknown_chunks() {
        let mut file = test_file();
        file.ecf_file_id = 0xDEAD_BEEF;
        assert!(matches!(
            Writer::write(&file),
            Err(Error::InvalidFileId { .. })
        ));

        let mut file = test_file();
        file.chunk_order.push(metadata(0xDEAD));
        assert!(matches!(
            Writer::write(&file),
            Err(Error::UnsupportedChunk(0xDEAD))
        ));
    }

    #[test]
    fn writer_rejects_inconsistent_linker_payloads() {
        let mut file = test_file();
        file.linkers[0].num_splat_layers = 2;
        assert!(Writer::write(&file).is_err());
    }
}
