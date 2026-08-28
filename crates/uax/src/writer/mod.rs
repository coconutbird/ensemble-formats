//! UAX writer for the packed x64 Granny animation graph.

use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;

use crate::types::{
    Animation, CurveData, PeriodicLoop, TextTrack, TrackGroup, Transform, TransformTrack,
    VectorTrack, animation, curve2, file_info, periodic_loop, text_track, text_track_entry,
    track_group, transform, transform_track, vector_track,
};
use crate::{Error, Result, UAX_CHUNK_ID, UAX_FILE_ID, UAX_FROM_FILENAME};

mod curve;
mod string_table;
mod type_tree;

use curve::CurveLayout;
use string_table::StringTable;

/// UAX file writer.
pub struct Writer;

impl Writer {
    /// Serialize one animation as a loadable UAX ECF file.
    ///
    /// The result contains the animation and its referenced track groups, but
    /// no ancillary Granny model or skeleton roots.
    ///
    /// # Errors
    ///
    /// Returns an error for oversized collections, embedded NUL characters,
    /// unknown curves, or a curve whose format byte does not match its typed
    /// payload.
    pub fn write(animation: &Animation) -> Result<Vec<u8>> {
        let file_info = build_file_info(animation)?;
        let mut ecf = ecf::Writer::new(UAX_FILE_ID);
        ecf.add_chunk_with_metadata(UAX_CHUNK_ID, file_info, 4, 0, 0)?;
        Ok(ecf.finalize()?)
    }
}

struct Planner {
    cursor: usize,
}

impl Planner {
    fn new(cursor: usize) -> Self {
        Self { cursor }
    }

    fn allocate_aligned(
        &mut self,
        size: usize,
        alignment: usize,
        field: &'static str,
    ) -> Result<usize> {
        if size == 0 {
            return Ok(0);
        }
        let mask = alignment.checked_sub(1).ok_or(Error::SizeOverflow(field))?;
        let start = self
            .cursor
            .checked_add(mask)
            .ok_or(Error::SizeOverflow(field))?
            & !mask;
        self.cursor = start.checked_add(size).ok_or(Error::SizeOverflow(field))?;
        Ok(start)
    }

    fn array(&mut self, count: usize, element_size: usize, field: &'static str) -> Result<usize> {
        let size = count
            .checked_mul(element_size)
            .ok_or(Error::SizeOverflow(field))?;
        self.allocate_aligned(size, 16, field)
    }
}

struct GroupLayout {
    vector_tracks: usize,
    transform_tracks: usize,
    lod_errors: usize,
    text_tracks: usize,
    text_entries: Vec<usize>,
    periodic_loop: usize,
    vector_curves: Vec<CurveLayout>,
    transform_curves: Vec<[CurveLayout; 3]>,
}

struct FileLayout {
    group_pointer_array: usize,
    group_structures: usize,
    groups: Vec<GroupLayout>,
    animation_pointer_array: usize,
    animation_structure: usize,
    animation_group_pointer_array: usize,
    type_trees: Vec<(u8, usize)>,
    size: usize,
}

fn checked_i32(value: usize, field: &'static str) -> Result<i32> {
    i32::try_from(value).map_err(|_| Error::SizeOverflow(field))
}

fn checked_u64(value: usize, field: &'static str) -> Result<u64> {
    u64::try_from(value).map_err(|_| Error::SizeOverflow(field))
}

fn write_bytes(output: &mut [u8], offset: usize, bytes: &[u8], field: &'static str) -> Result<()> {
    let end = offset
        .checked_add(bytes.len())
        .ok_or(Error::SizeOverflow(field))?;
    output
        .get_mut(offset..end)
        .ok_or(Error::SizeOverflow(field))?
        .copy_from_slice(bytes);
    Ok(())
}

fn put_u64(output: &mut [u8], offset: usize, value: u64) -> Result<()> {
    write_bytes(output, offset, &value.to_le_bytes(), "u64 field")
}

fn put_u32(output: &mut [u8], offset: usize, value: u32) -> Result<()> {
    write_bytes(output, offset, &value.to_le_bytes(), "u32 field")
}

fn put_i32(output: &mut [u8], offset: usize, value: i32) -> Result<()> {
    write_bytes(output, offset, &value.to_le_bytes(), "i32 field")
}

fn put_u16(output: &mut [u8], offset: usize, value: u16) -> Result<()> {
    write_bytes(output, offset, &value.to_le_bytes(), "u16 field")
}

fn put_i16(output: &mut [u8], offset: usize, value: i16) -> Result<()> {
    write_bytes(output, offset, &value.to_le_bytes(), "i16 field")
}

fn put_f32(output: &mut [u8], offset: usize, value: f32) -> Result<()> {
    write_bytes(output, offset, &value.to_bits().to_le_bytes(), "f32 field")
}

fn put_pointer(
    output: &mut [u8],
    field_offset: usize,
    target_offset: usize,
    field: &'static str,
) -> Result<()> {
    put_u64(output, field_offset, checked_u64(target_offset, field)?)
}

fn add(base: usize, relative: usize, field: &'static str) -> Result<usize> {
    base.checked_add(relative).ok_or(Error::SizeOverflow(field))
}

fn element_offset(
    base: usize,
    index: usize,
    element_size: usize,
    field: &'static str,
) -> Result<usize> {
    let relative = index
        .checked_mul(element_size)
        .ok_or(Error::SizeOverflow(field))?;
    add(base, relative, field)
}

fn validate_string(value: &str, field: &'static str) -> Result<()> {
    if value.as_bytes().contains(&0) {
        return Err(Error::EmbeddedNul(field));
    }
    Ok(())
}

fn validate_strings(animation: &Animation) -> Result<()> {
    if let Some(name) = &animation.name {
        validate_string(name, "animation name")?;
    }
    for group in &animation.track_groups {
        if let Some(name) = &group.name {
            validate_string(name, "track-group name")?;
        }
        for track in &group.vector_tracks {
            if let Some(name) = &track.name {
                validate_string(name, "vector-track name")?;
            }
        }
        for track in &group.transform_tracks {
            if let Some(name) = &track.name {
                validate_string(name, "transform-track name")?;
            }
        }
        for track in &group.text_tracks {
            if let Some(name) = &track.name {
                validate_string(name, "text-track name")?;
            }
            for entry in &track.entries {
                if let Some(text) = &entry.text {
                    validate_string(text, "text-track entry")?;
                }
            }
        }
    }
    Ok(())
}

fn add_curve_format(formats: &mut Vec<u8>, curve: &CurveData) -> Result<()> {
    curve::validate(curve)?;
    if !formats.contains(&curve.format) {
        formats.push(curve.format);
    }
    Ok(())
}

fn collect_formats(animation: &Animation) -> Result<Vec<u8>> {
    let mut formats = Vec::new();
    for group in &animation.track_groups {
        for track in &group.vector_tracks {
            add_curve_format(&mut formats, &track.value)?;
        }
        for track in &group.transform_tracks {
            add_curve_format(&mut formats, &track.orientation)?;
            add_curve_format(&mut formats, &track.position)?;
            add_curve_format(&mut formats, &track.scale_shear)?;
        }
    }
    Ok(formats)
}

fn plan_curve(planner: &mut Planner, value: &CurveData) -> Result<CurveLayout> {
    let (object_size, array_sizes) = curve::sizes(value)?;
    let object_offset = planner.allocate_aligned(object_size, 16, "curve object")?;
    let mut array_offsets = Vec::with_capacity(array_sizes.len());
    for size in array_sizes {
        array_offsets.push(planner.allocate_aligned(size, 4, "curve referenced array")?);
    }
    Ok(CurveLayout {
        object_offset,
        array_offsets,
    })
}

fn plan_layout(animation: &Animation) -> Result<FileLayout> {
    validate_strings(animation)?;
    checked_i32(animation.track_groups.len(), "track-group count")?;

    let formats = collect_formats(animation)?;
    let mut planner = Planner::new(file_info::SIZE);
    let group_pointer_array =
        planner.array(animation.track_groups.len(), 8, "group pointer array")?;
    let group_structures = planner.array(
        animation.track_groups.len(),
        track_group::SIZE,
        "track-group structures",
    )?;

    let mut groups = Vec::with_capacity(animation.track_groups.len());
    for group in &animation.track_groups {
        checked_i32(group.vector_tracks.len(), "vector-track count")?;
        checked_i32(group.transform_tracks.len(), "transform-track count")?;
        checked_i32(group.transform_lod_errors.len(), "LOD error count")?;
        checked_i32(group.text_tracks.len(), "text-track count")?;
        let vector_tracks = planner.array(
            group.vector_tracks.len(),
            vector_track::SIZE,
            "vector-track array",
        )?;
        let transform_tracks = planner.array(
            group.transform_tracks.len(),
            transform_track::SIZE,
            "transform-track array",
        )?;
        let lod_errors = planner.array(group.transform_lod_errors.len(), 4, "LOD error array")?;
        let text_tracks = planner.array(
            group.text_tracks.len(),
            text_track::SIZE,
            "text-track array",
        )?;
        let mut text_entries = Vec::with_capacity(group.text_tracks.len());
        for track in &group.text_tracks {
            checked_i32(track.entries.len(), "text-track entry count")?;
            text_entries.push(planner.array(
                track.entries.len(),
                text_track_entry::SIZE,
                "text-track entry array",
            )?);
        }
        let periodic_loop = if group.periodic_loop.is_some() {
            planner.allocate_aligned(periodic_loop::SIZE, 16, "periodic loop")?
        } else {
            0
        };
        groups.push(GroupLayout {
            vector_tracks,
            transform_tracks,
            lod_errors,
            text_tracks,
            text_entries,
            periodic_loop,
            vector_curves: Vec::new(),
            transform_curves: Vec::new(),
        });
    }

    let animation_pointer_array = planner.allocate_aligned(8, 16, "animation pointer array")?;
    let animation_structure =
        planner.allocate_aligned(animation::SIZE, 16, "animation structure")?;
    let animation_group_pointer_array = planner.array(
        animation.track_groups.len(),
        8,
        "animation group pointer array",
    )?;

    let mut type_trees = Vec::with_capacity(formats.len());
    for format in formats {
        let size = type_tree::curve_type_tree_size(format)?;
        let offset = planner.allocate_aligned(size, 16, "curve type tree")?;
        type_trees.push((format, offset));
    }

    for (group, layout) in animation.track_groups.iter().zip(&mut groups) {
        for track in &group.vector_tracks {
            layout
                .vector_curves
                .push(plan_curve(&mut planner, &track.value)?);
        }
        for track in &group.transform_tracks {
            layout.transform_curves.push([
                plan_curve(&mut planner, &track.orientation)?,
                plan_curve(&mut planner, &track.position)?,
                plan_curve(&mut planner, &track.scale_shear)?,
            ]);
        }
    }

    Ok(FileLayout {
        group_pointer_array,
        group_structures,
        groups,
        animation_pointer_array,
        animation_structure,
        animation_group_pointer_array,
        type_trees,
        size: planner.cursor,
    })
}

fn build_file_info(animation: &Animation) -> Result<Vec<u8>> {
    let layout = plan_layout(animation)?;
    let mut output = vec![0; layout.size];
    let mut strings = StringTable::new();
    strings.add(file_info::FROM_FILE_NAME_PTR, UAX_FROM_FILENAME.to_string());

    put_i32(
        &mut output,
        file_info::TRACK_GROUP_COUNT,
        checked_i32(animation.track_groups.len(), "track-group count")?,
    )?;
    put_pointer(
        &mut output,
        file_info::TRACK_GROUPS_PTR,
        layout.group_pointer_array,
        "group pointer array",
    )?;
    put_i32(&mut output, file_info::ANIMATION_COUNT, 1)?;
    put_pointer(
        &mut output,
        file_info::ANIMATIONS_PTR,
        layout.animation_pointer_array,
        "animation pointer array",
    )?;

    for (index, group) in animation.track_groups.iter().enumerate() {
        let group_offset = element_offset(
            layout.group_structures,
            index,
            track_group::SIZE,
            "track-group offset",
        )?;
        put_pointer(
            &mut output,
            element_offset(layout.group_pointer_array, index, 8, "group pointer")?,
            group_offset,
            "track-group pointer",
        )?;
        write_track_group(
            &mut output,
            &mut strings,
            group,
            group_offset,
            &layout.groups[index],
            &layout.type_trees,
        )?;
    }

    put_pointer(
        &mut output,
        layout.animation_pointer_array,
        layout.animation_structure,
        "animation structure pointer",
    )?;
    write_animation(&mut output, &mut strings, animation, &layout)?;

    for &(format, offset) in &layout.type_trees {
        type_tree::write_curve_type_tree(&mut output, &mut strings, format, offset)?;
    }
    strings.write(&mut output)?;
    Ok(output)
}

fn write_track_group(
    output: &mut [u8],
    strings: &mut StringTable,
    group: &TrackGroup,
    offset: usize,
    layout: &GroupLayout,
    type_trees: &[(u8, usize)],
) -> Result<()> {
    if let Some(name) = &group.name {
        strings.add(
            add(offset, track_group::NAME_PTR, "track-group name")?,
            name.clone(),
        );
    }
    write_array_header(
        output,
        offset,
        track_group::VECTOR_TRACK_COUNT,
        track_group::VECTOR_TRACKS_PTR,
        group.vector_tracks.len(),
        layout.vector_tracks,
        "vector tracks",
    )?;
    write_array_header(
        output,
        offset,
        track_group::TRANSFORM_TRACK_COUNT,
        track_group::TRANSFORM_TRACKS_PTR,
        group.transform_tracks.len(),
        layout.transform_tracks,
        "transform tracks",
    )?;
    write_array_header(
        output,
        offset,
        track_group::TRANSFORM_LOD_ERROR_COUNT,
        track_group::TRANSFORM_LOD_ERRORS_PTR,
        group.transform_lod_errors.len(),
        layout.lod_errors,
        "transform LOD errors",
    )?;
    write_array_header(
        output,
        offset,
        track_group::TEXT_TRACK_COUNT,
        track_group::TEXT_TRACKS_PTR,
        group.text_tracks.len(),
        layout.text_tracks,
        "text tracks",
    )?;

    write_transform(
        output,
        add(offset, track_group::INITIAL_PLACEMENT, "initial placement")?,
        &group.initial_placement,
    )?;
    put_u32(
        output,
        add(offset, track_group::FLAGS, "accumulation flags")?,
        group.flags,
    )?;
    write_f32_values(
        output,
        add(offset, track_group::LOOP_TRANSLATION, "loop translation")?,
        &group.loop_translation,
    )?;
    if let Some(periodic) = &group.periodic_loop {
        put_pointer(
            output,
            add(
                offset,
                track_group::PERIODIC_LOOP_PTR,
                "periodic-loop pointer",
            )?,
            layout.periodic_loop,
            "periodic-loop pointer",
        )?;
        write_periodic_loop(output, layout.periodic_loop, periodic)?;
    }

    write_group_track_data(output, strings, group, layout, type_trees)
}

fn write_group_track_data(
    output: &mut [u8],
    strings: &mut StringTable,
    group: &TrackGroup,
    layout: &GroupLayout,
    type_trees: &[(u8, usize)],
) -> Result<()> {
    for (index, track) in group.vector_tracks.iter().enumerate() {
        let track_offset = element_offset(
            layout.vector_tracks,
            index,
            vector_track::SIZE,
            "vector-track offset",
        )?;
        write_vector_track(
            output,
            strings,
            track,
            track_offset,
            &layout.vector_curves[index],
            type_trees,
        )?;
    }
    for (index, track) in group.transform_tracks.iter().enumerate() {
        let track_offset = element_offset(
            layout.transform_tracks,
            index,
            transform_track::SIZE,
            "transform-track offset",
        )?;
        write_transform_track(
            output,
            strings,
            track,
            track_offset,
            &layout.transform_curves[index],
            type_trees,
        )?;
    }
    for (index, error) in group.transform_lod_errors.iter().enumerate() {
        put_f32(
            output,
            element_offset(layout.lod_errors, index, 4, "LOD error")?,
            *error,
        )?;
    }
    for (index, track) in group.text_tracks.iter().enumerate() {
        let track_offset = element_offset(
            layout.text_tracks,
            index,
            text_track::SIZE,
            "text-track offset",
        )?;
        write_text_track(
            output,
            strings,
            track,
            track_offset,
            layout.text_entries[index],
        )?;
    }
    Ok(())
}

fn write_array_header(
    output: &mut [u8],
    base: usize,
    count_relative: usize,
    pointer_relative: usize,
    count: usize,
    pointer: usize,
    field: &'static str,
) -> Result<()> {
    put_i32(
        output,
        add(base, count_relative, field)?,
        checked_i32(count, field)?,
    )?;
    if count > 0 {
        put_pointer(output, add(base, pointer_relative, field)?, pointer, field)?;
    }
    Ok(())
}

fn type_tree_offset(type_trees: &[(u8, usize)], format: u8) -> Result<usize> {
    type_trees
        .iter()
        .find(|(candidate, _)| *candidate == format)
        .map(|(_, offset)| *offset)
        .ok_or(Error::UnsupportedCurveFormat(format))
}

fn write_curve_variant(
    output: &mut [u8],
    offset: usize,
    value: &CurveData,
    layout: &CurveLayout,
    type_trees: &[(u8, usize)],
) -> Result<()> {
    put_pointer(
        output,
        add(offset, curve2::TYPE_PTR, "curve type pointer")?,
        type_tree_offset(type_trees, value.format)?,
        "curve type pointer",
    )?;
    put_pointer(
        output,
        add(offset, curve2::OBJECT_PTR, "curve object pointer")?,
        layout.object_offset,
        "curve object pointer",
    )?;
    curve::write(output, layout, value)
}

fn write_vector_track(
    output: &mut [u8],
    strings: &mut StringTable,
    track: &VectorTrack,
    offset: usize,
    curve_layout: &CurveLayout,
    type_trees: &[(u8, usize)],
) -> Result<()> {
    if let Some(name) = &track.name {
        strings.add(
            add(offset, vector_track::NAME_PTR, "vector-track name")?,
            name.clone(),
        );
    }
    put_u32(
        output,
        add(offset, vector_track::TRACK_KEY, "vector-track key")?,
        track.track_key,
    )?;
    put_i32(
        output,
        add(offset, vector_track::DIMENSION, "vector-track dimension")?,
        track.dimension,
    )?;
    write_curve_variant(
        output,
        add(offset, vector_track::VALUE_CURVE, "vector-track curve")?,
        &track.value,
        curve_layout,
        type_trees,
    )
}

fn write_transform_track(
    output: &mut [u8],
    strings: &mut StringTable,
    track: &TransformTrack,
    offset: usize,
    curve_layouts: &[CurveLayout; 3],
    type_trees: &[(u8, usize)],
) -> Result<()> {
    if let Some(name) = &track.name {
        strings.add(
            add(offset, transform_track::NAME_PTR, "transform-track name")?,
            name.clone(),
        );
    }
    put_i32(
        output,
        add(offset, transform_track::FLAGS, "transform-track flags")?,
        track.flags,
    )?;
    for (relative, value, layout) in [
        (
            transform_track::ORIENTATION_CURVE,
            &track.orientation,
            &curve_layouts[0],
        ),
        (
            transform_track::POSITION_CURVE,
            &track.position,
            &curve_layouts[1],
        ),
        (
            transform_track::SCALE_SHEAR_CURVE,
            &track.scale_shear,
            &curve_layouts[2],
        ),
    ] {
        write_curve_variant(
            output,
            add(offset, relative, "transform curve")?,
            value,
            layout,
            type_trees,
        )?;
    }
    Ok(())
}

fn write_text_track(
    output: &mut [u8],
    strings: &mut StringTable,
    track: &TextTrack,
    offset: usize,
    entries_offset: usize,
) -> Result<()> {
    if let Some(name) = &track.name {
        strings.add(
            add(offset, text_track::NAME_PTR, "text-track name")?,
            name.clone(),
        );
    }
    write_array_header(
        output,
        offset,
        text_track::ENTRY_COUNT,
        text_track::ENTRIES_PTR,
        track.entries.len(),
        entries_offset,
        "text-track entries",
    )?;
    for (index, entry) in track.entries.iter().enumerate() {
        let entry_offset = element_offset(
            entries_offset,
            index,
            text_track_entry::SIZE,
            "text-track entry",
        )?;
        put_f32(
            output,
            add(
                entry_offset,
                text_track_entry::TIME_STAMP,
                "text-track timestamp",
            )?,
            entry.time_stamp,
        )?;
        if let Some(text) = &entry.text {
            strings.add(
                add(entry_offset, text_track_entry::TEXT_PTR, "text-track text")?,
                text.clone(),
            );
        }
    }
    Ok(())
}

fn write_animation(
    output: &mut [u8],
    strings: &mut StringTable,
    animation: &Animation,
    layout: &FileLayout,
) -> Result<()> {
    let offset = layout.animation_structure;
    if let Some(name) = &animation.name {
        strings.add(
            add(offset, animation::NAME_PTR, "animation name")?,
            name.clone(),
        );
    }
    put_f32(
        output,
        add(offset, animation::DURATION, "animation duration")?,
        animation.duration,
    )?;
    put_f32(
        output,
        add(offset, animation::TIME_STEP, "animation time step")?,
        animation.time_step,
    )?;
    put_f32(
        output,
        add(offset, animation::OVERSAMPLING, "animation oversampling")?,
        animation.oversampling,
    )?;
    write_array_header(
        output,
        offset,
        animation::TRACK_GROUP_COUNT,
        animation::TRACK_GROUPS_PTR,
        animation.track_groups.len(),
        layout.animation_group_pointer_array,
        "animation track groups",
    )?;
    put_i32(
        output,
        add(
            offset,
            animation::DEFAULT_LOOP_COUNT,
            "animation default loop count",
        )?,
        animation.default_loop_count,
    )?;
    put_u32(
        output,
        add(offset, animation::FLAGS, "animation flags")?,
        animation.flags,
    )?;

    for index in 0..animation.track_groups.len() {
        let group_offset = element_offset(
            layout.group_structures,
            index,
            track_group::SIZE,
            "animation track-group target",
        )?;
        put_pointer(
            output,
            element_offset(
                layout.animation_group_pointer_array,
                index,
                8,
                "animation track-group pointer",
            )?,
            group_offset,
            "animation track-group pointer",
        )?;
    }
    Ok(())
}

fn write_transform(output: &mut [u8], offset: usize, value: &Transform) -> Result<()> {
    put_u32(
        output,
        add(offset, transform::FLAGS, "transform flags")?,
        value.flags,
    )?;
    write_f32_values(
        output,
        add(offset, transform::POSITION, "transform position")?,
        &value.position,
    )?;
    write_f32_values(
        output,
        add(offset, transform::ORIENTATION, "transform orientation")?,
        &value.orientation,
    )?;
    write_f32_values(
        output,
        add(offset, transform::SCALE_SHEAR, "transform scale/shear")?,
        &value.scale_shear,
    )
}

fn write_periodic_loop(output: &mut [u8], offset: usize, value: &PeriodicLoop) -> Result<()> {
    put_f32(
        output,
        add(offset, periodic_loop::RADIUS, "loop radius")?,
        value.radius,
    )?;
    put_f32(
        output,
        add(offset, periodic_loop::D_ANGLE, "loop angle")?,
        value.d_angle,
    )?;
    put_f32(
        output,
        add(offset, periodic_loop::D_Z, "loop Z delta")?,
        value.d_z,
    )?;
    write_f32_values(
        output,
        add(offset, periodic_loop::BASIS_X, "loop basis X")?,
        &value.basis_x,
    )?;
    write_f32_values(
        output,
        add(offset, periodic_loop::BASIS_Y, "loop basis Y")?,
        &value.basis_y,
    )?;
    write_f32_values(
        output,
        add(offset, periodic_loop::AXIS, "loop axis")?,
        &value.axis,
    )
}

fn write_f32_values(output: &mut [u8], offset: usize, values: &[f32]) -> Result<()> {
    for (index, value) in values.iter().enumerate() {
        let relative = index
            .checked_mul(4)
            .ok_or(Error::SizeOverflow("f32 array offset"))?;
        put_f32(output, add(offset, relative, "f32 array offset")?, *value)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
