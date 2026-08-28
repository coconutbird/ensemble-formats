//! UAX writer implementation.
//!
//! Serializes a parsed [`Animation`] back into Granny `file_info` format
//! wrapped in an ECF container, producing bytes that the game engine can load.
//!
//! The writer follows the same structure layout as the original game files:
//! 1. `file_info` header (0x94 bytes)
//! 2. `TrackGroup` pointer array + structs
//! 3. `TransformTrack` arrays
//! 4. Curve type trees (one per unique format)
//! 5. Animation pointer array + struct
//! 6. Curve data objects
//! 7. File-info type tree
//! 8. String table (names, `FromFileName`)
//!
//! Uses [`nostdio::Cursor`] and [`nostdio::WriteLe`] for all binary writes.

use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;

use crate::types::{
    Animation, CurveData, CurvePayload, Transform, TransformTrack, animation, curve_data_header,
    curve2, track_group, transform, transform_track,
};
use crate::{Error, Result, UAX_CHUNK_ID, UAX_FILE_ID, UAX_FROM_FILENAME};

mod string_table;
mod type_tree;

use string_table::StringTable;

/// UAX file writer.
pub struct Writer;

impl Writer {
    /// Write an [`Animation`] to UAX file bytes (ECF container).
    ///
    /// # Errors
    ///
    /// Returns an error if a collection is too large for Granny's signed
    /// 32-bit count fields or the ECF container exceeds its format limits.
    pub fn write(anim: &Animation) -> Result<Vec<u8>> {
        let fi_data = build_file_info(anim)?;

        let mut ecf = ecf::Writer::new(UAX_FILE_ID);
        ecf.add_chunk(UAX_CHUNK_ID, fi_data);
        Ok(ecf.finalize()?)
    }
}

/// Align to 16-byte boundary.
fn align16(n: usize) -> usize {
    (n + 15) & !15
}

fn checked_i32(value: usize, field: &'static str) -> Result<i32> {
    i32::try_from(value).map_err(|_| Error::SizeOverflow(field))
}

/// Write a u64 LE at offset in buf.
fn put_u64(buf: &mut [u8], off: usize, val: u64) {
    buf[off..off + 8].copy_from_slice(&val.to_le_bytes());
}

/// Write a u32 LE at offset in buf.
fn put_u32(buf: &mut [u8], off: usize, val: u32) {
    buf[off..off + 4].copy_from_slice(&val.to_le_bytes());
}

/// Write an i32 LE at offset in buf.
fn put_i32(buf: &mut [u8], off: usize, val: i32) {
    buf[off..off + 4].copy_from_slice(&val.to_le_bytes());
}

/// Write an f32 LE at offset in buf.
fn put_f32(buf: &mut [u8], off: usize, val: f32) {
    buf[off..off + 4].copy_from_slice(&val.to_le_bytes());
}

/// Write a u16 LE at offset in buf.
fn put_u16(buf: &mut [u8], off: usize, val: u16) {
    buf[off..off + 2].copy_from_slice(&val.to_le_bytes());
}

/// Build the Granny `file_info` chunk data from an Animation.
fn build_file_info(anim: &Animation) -> Result<Vec<u8>> {
    let tg_count = anim.track_groups.len();
    let mut strings = StringTable::new();

    // ---- Phase 1: Compute layout offsets ----
    let header_size: usize = 0x94;

    // TrackGroup pointer array
    let tg_ptr_array = align16(header_size);
    // TrackGroup structs
    let tg_structs_start = align16(tg_ptr_array + tg_count * 8);

    // Compute transform track counts and offsets
    let mut tt_offsets = Vec::with_capacity(tg_count);
    let mut cursor = tg_structs_start + tg_count * track_group::SIZE;

    for tg in &anim.track_groups {
        let tt_start = align16(cursor);
        tt_offsets.push(tt_start);
        cursor = tt_start + tg.transform_tracks.len() * transform_track::SIZE;
    }

    // LOD error arrays (one per track group, only if non-empty)
    let mut lod_offsets = Vec::with_capacity(tg_count);
    for tg in &anim.track_groups {
        if tg.transform_lod_errors.is_empty() {
            lod_offsets.push(None);
        } else {
            let lod_start = align16(cursor);
            lod_offsets.push(Some(lod_start));
            cursor = lod_start + tg.transform_lod_errors.len() * 4;
        }
    }

    // Phase 2: Curve type trees
    // Collect unique curve formats used
    let unique_formats = collect_unique_formats(anim);
    let mut type_tree_offsets: Vec<(u8, usize)> = Vec::new(); // (format, offset)
    for &fmt in &unique_formats {
        let tt_off = align16(cursor);
        type_tree_offsets.push((fmt, tt_off));
        cursor = tt_off + type_tree::curve_type_tree_size(fmt);
    }

    // Animation pointer array + struct
    let anim_ptr_array = align16(cursor);
    cursor = anim_ptr_array + 8; // single animation pointer
    let anim_struct = align16(cursor);
    cursor = anim_struct + animation::SIZE;

    // Animation's own track group ptr array
    let anim_tg_ptr_array = align16(cursor);
    cursor = anim_tg_ptr_array + tg_count * 8;

    // Phase 3: Curve data objects - placed after animation structs
    // We need to emit curve objects for each transform track
    let curve_layout = plan_curve_data(anim, &mut cursor);

    // Phase 4: File info type tree
    let fi_type_tree_start = align16(cursor);
    cursor = fi_type_tree_start + type_tree::FILE_INFO_TYPE_TREE_SIZE;

    // Now allocate the buffer (strings will be appended later)
    let mut buf = vec![0u8; cursor];

    // ---- Write file_info header ----
    strings.add(0x10, UAX_FROM_FILENAME.to_string());

    // TrackGroups RTA at 0x6C
    put_i32(&mut buf, 0x6C, checked_i32(tg_count, "track group count")?);
    put_u64(&mut buf, 0x70, tg_ptr_array as u64);

    // Animations RTA at 0x78
    put_i32(&mut buf, 0x78, 1); // always 1 animation
    put_u64(&mut buf, 0x7C, anim_ptr_array as u64);

    // ---- Write TrackGroup pointer array ----
    for (i, _) in anim.track_groups.iter().enumerate() {
        let tg_struct_off = tg_structs_start + i * track_group::SIZE;
        put_u64(&mut buf, tg_ptr_array + i * 8, tg_struct_off as u64);
    }

    // ---- Write TrackGroup structs ----
    write_track_groups(
        &mut buf,
        &mut strings,
        anim,
        &TrackGroupLayout {
            start: tg_structs_start,
            track_offsets: &tt_offsets,
            lod_offsets: &lod_offsets,
            type_tree_offsets: &type_tree_offsets,
            curves: &curve_layout,
        },
    )?;

    // ---- Write Animation pointer array + struct ----
    put_u64(&mut buf, anim_ptr_array, anim_struct as u64);
    write_animation(&mut buf, &mut strings, anim, anim_struct, anim_tg_ptr_array)?;

    // ---- Write Animation's TG ptr array (points to same TG structs) ----
    for i in 0..tg_count {
        let tg_struct_off = tg_structs_start + i * track_group::SIZE;
        put_u64(&mut buf, anim_tg_ptr_array + i * 8, tg_struct_off as u64);
    }

    // ---- Write type trees for curve formats ----
    for &(fmt, off) in &type_tree_offsets {
        type_tree::write_curve_type_tree(&mut buf, &mut strings, fmt, off);
    }

    // ---- Write curve data objects ----
    write_curve_data(&mut buf, anim, &curve_layout)?;

    // ---- Write file_info type tree ----
    type_tree::write_file_info_type_tree(&mut buf, &mut strings, fi_type_tree_start);

    // ---- Phase 5: String table (appended at end) ----
    strings.write(&mut buf);

    Ok(buf)
}

// ============================================================================
// Curve format collection
// ============================================================================

/// Collect unique curve format IDs across all track groups.
fn collect_unique_formats(anim: &Animation) -> Vec<u8> {
    let mut seen = Vec::new();
    for tg in &anim.track_groups {
        for tt in &tg.transform_tracks {
            for curve in [&tt.orientation, &tt.position, &tt.scale_shear] {
                if !seen.contains(&curve.format) {
                    seen.push(curve.format);
                }
            }
        }
    }
    seen
}

// ============================================================================
// Curve data layout planning
// ============================================================================

/// Layout information for a single curve data object.
struct CurveObjLayout {
    /// Offset of the curve data object in the buffer.
    obj_offset: usize,
    /// Optional: offsets for `ref_arr` data (knots, controls, `knots_controls`).
    ref_arr_offsets: Vec<usize>,
}

/// Complete layout for all curve data in the animation.
struct CurveDataLayout {
    /// Per track-group, per transform-track, per curve (ori/pos/ss) layout.
    /// Indexed as [`tg_idx`][tt_idx][`curve_idx`] where `curve_idx`: 0=ori, 1=pos, 2=ss
    layouts: Vec<Vec<[CurveObjLayout; 3]>>,
}

/// Plan where all curve data objects and their `ref_arr` data will be placed.
fn plan_curve_data(anim: &Animation, cursor: &mut usize) -> CurveDataLayout {
    let mut layouts = Vec::with_capacity(anim.track_groups.len());

    for tg in &anim.track_groups {
        let mut tg_layouts = Vec::with_capacity(tg.transform_tracks.len());
        for tt in &tg.transform_tracks {
            let curves = [&tt.orientation, &tt.position, &tt.scale_shear];
            let mut curve_layouts: [CurveObjLayout; 3] = core::array::from_fn(|_| CurveObjLayout {
                obj_offset: 0,
                ref_arr_offsets: Vec::new(),
            });

            for (ci, curve) in curves.iter().enumerate() {
                let obj_off = align16(*cursor);
                let (obj_size, ref_arr_sizes) = curve_obj_size(curve);
                *cursor = obj_off + obj_size;

                let mut ref_arr_offsets = Vec::new();
                for &ra_size in &ref_arr_sizes {
                    let ra_off = *cursor; // ref_arr data follows immediately (no alignment needed)
                    ref_arr_offsets.push(ra_off);
                    *cursor += ra_size;
                }

                curve_layouts[ci] = CurveObjLayout {
                    obj_offset: obj_off,
                    ref_arr_offsets,
                };
            }
            tg_layouts.push(curve_layouts);
        }
        layouts.push(tg_layouts);
    }

    CurveDataLayout { layouts }
}

/// Compute the size of a curve data object (header + payload) and sizes of `ref_arr` data.
fn curve_obj_size(curve: &CurveData) -> (usize, Vec<usize>) {
    let header = curve_data_header::SIZE; // 2 bytes (format + degree)
    match &curve.payload {
        CurvePayload::Identity { .. } => (header + 2, vec![]),
        CurvePayload::DaConstant32f { controls, .. } => {
            // padding(2) + ref_arr header (i32 count + u64 ptr = 12)
            (header + 2 + 12, vec![controls.len() * 4])
        }
        CurvePayload::D3Constant32f { .. } => (header + 2 + 12, vec![]),
        CurvePayload::D4Constant32f { .. } => (header + 2 + 16, vec![]),
        CurvePayload::DaK32fC32f {
            knots, controls, ..
        } => {
            // padding(2) + 2x ref_arr headers (12 each)
            (header + 2 + 24, vec![knots.len() * 4, controls.len() * 4])
        }
        CurvePayload::D4nK16uC15u { knots_controls, .. }
        | CurvePayload::D4nK8uC7u { knots_controls, .. } => {
            // sote(2) + ooks(4) + ref_arr(12)
            (header + 2 + 4 + 12, vec![knots_controls.len()])
        }
        CurvePayload::D3K16uC16u { knots_controls, .. }
        | CurvePayload::D3K8uC8u { knots_controls, .. }
        | CurvePayload::D3I1K8uC8u { knots_controls, .. } => {
            // ooks_trunc(2) + scales(12) + offsets(12) + ref_arr(12)
            (header + 2 + 12 + 12 + 12, vec![knots_controls.len()])
        }
        CurvePayload::Unknown { raw } => (header + raw.len(), vec![]),
    }
}

// ============================================================================
// Structure writers
// ============================================================================

/// Write a Granny transform into `buf` at `offset` (68 bytes).
fn write_transform(buf: &mut [u8], offset: usize, t: &Transform) {
    put_u32(buf, offset + transform::FLAGS, t.flags);
    for i in 0..3 {
        put_f32(buf, offset + transform::POSITION + i * 4, t.position[i]);
    }
    for i in 0..4 {
        put_f32(
            buf,
            offset + transform::ORIENTATION + i * 4,
            t.orientation[i],
        );
    }
    for i in 0..9 {
        put_f32(
            buf,
            offset + transform::SCALE_SHEAR + i * 4,
            t.scale_shear[i],
        );
    }
}

/// Write all track group structs and their transform track arrays.
struct TrackGroupLayout<'a> {
    start: usize,
    track_offsets: &'a [usize],
    lod_offsets: &'a [Option<usize>],
    type_tree_offsets: &'a [(u8, usize)],
    curves: &'a CurveDataLayout,
}

fn write_track_groups(
    buf: &mut [u8],
    strings: &mut StringTable,
    anim: &Animation,
    layout: &TrackGroupLayout<'_>,
) -> Result<()> {
    for (tgi, tg) in anim.track_groups.iter().enumerate() {
        let base = layout.start + tgi * track_group::SIZE;

        // Name
        if let Some(ref name) = tg.name {
            strings.add(base + track_group::NAME_PTR, name.clone());
        }

        // TransformTrack count + ptr
        let tt_count = tg.transform_tracks.len();
        put_i32(
            buf,
            base + track_group::TRANSFORM_TRACK_COUNT,
            checked_i32(tt_count, "transform track count")?,
        );
        if tt_count > 0 {
            put_u64(
                buf,
                base + track_group::TRANSFORM_TRACKS_PTR,
                layout.track_offsets[tgi] as u64,
            );
        }

        // LOD errors
        if let Some(lod_off) = layout.lod_offsets[tgi] {
            put_i32(
                buf,
                base + track_group::TRANSFORM_LOD_ERROR_COUNT,
                checked_i32(tg.transform_lod_errors.len(), "LOD error count")?,
            );
            put_u64(
                buf,
                base + track_group::TRANSFORM_LOD_ERRORS_PTR,
                lod_off as u64,
            );
            for (i, &e) in tg.transform_lod_errors.iter().enumerate() {
                put_f32(buf, lod_off + i * 4, e);
            }
        }

        // InitialPlacement
        write_transform(
            buf,
            base + track_group::INITIAL_PLACEMENT,
            &tg.initial_placement,
        );

        // Flags
        put_u32(buf, base + track_group::FLAGS, tg.flags);

        // Write transform tracks
        write_transform_tracks(
            buf,
            strings,
            &tg.transform_tracks,
            layout.track_offsets[tgi],
            layout.type_tree_offsets,
            &layout.curves.layouts[tgi],
        );
    }
    Ok(())
}

/// Find the type tree offset for a given curve format.
fn find_type_tree_offset(type_tree_offsets: &[(u8, usize)], fmt: u8) -> u64 {
    type_tree_offsets
        .iter()
        .find(|&&(f, _)| f == fmt)
        .map_or(0, |&(_, off)| off as u64)
}

/// Write a curve2 variant (`type_ptr` + `obj_ptr`) at `offset`.
fn write_curve2(buf: &mut [u8], offset: usize, type_tree_off: u64, obj_off: u64) {
    put_u64(buf, offset + curve2::TYPE_PTR, type_tree_off);
    put_u64(buf, offset + curve2::OBJECT_PTR, obj_off);
}

/// Write transform tracks for a single track group.
fn write_transform_tracks(
    buf: &mut [u8],
    strings: &mut StringTable,
    tracks: &[TransformTrack],
    tt_start: usize,
    type_tree_offsets: &[(u8, usize)],
    curve_layouts: &[[CurveObjLayout; 3]],
) {
    for (tti, tt) in tracks.iter().enumerate() {
        let base = tt_start + tti * transform_track::SIZE;

        // Name
        if let Some(ref name) = tt.name {
            strings.add(base + transform_track::NAME_PTR, name.clone());
        }

        // Flags
        put_i32(buf, base + transform_track::FLAGS, tt.flags);

        // Orientation curve2
        let cl = &curve_layouts[tti];
        let ori_tt = find_type_tree_offset(type_tree_offsets, tt.orientation.format);
        write_curve2(
            buf,
            base + transform_track::ORIENTATION_CURVE,
            ori_tt,
            cl[0].obj_offset as u64,
        );

        // Position curve2
        let pos_tt = find_type_tree_offset(type_tree_offsets, tt.position.format);
        write_curve2(
            buf,
            base + transform_track::POSITION_CURVE,
            pos_tt,
            cl[1].obj_offset as u64,
        );

        // ScaleShear curve2
        let ss_tt = find_type_tree_offset(type_tree_offsets, tt.scale_shear.format);
        write_curve2(
            buf,
            base + transform_track::SCALE_SHEAR_CURVE,
            ss_tt,
            cl[2].obj_offset as u64,
        );
    }
}

/// Write the animation struct.
fn write_animation(
    buf: &mut [u8],
    strings: &mut StringTable,
    anim: &Animation,
    offset: usize,
    tg_ptr_array: usize,
) -> Result<()> {
    if let Some(ref name) = anim.name {
        strings.add(offset + animation::NAME_PTR, name.clone());
    }
    put_f32(buf, offset + animation::DURATION, anim.duration);
    put_f32(buf, offset + animation::TIME_STEP, anim.time_step);
    put_f32(buf, offset + animation::OVERSAMPLING, anim.oversampling);
    put_i32(
        buf,
        offset + animation::TRACK_GROUP_COUNT,
        checked_i32(anim.track_groups.len(), "animation track group count")?,
    );
    put_u64(
        buf,
        offset + animation::TRACK_GROUPS_PTR,
        tg_ptr_array as u64,
    );
    Ok(())
}

// ============================================================================
// Curve data writing
// ============================================================================

/// Write a `ref_arr` header (count i32 + ptr u64) and copy data.
fn write_ref_arr(
    buf: &mut [u8],
    header_off: usize,
    data_off: usize,
    data: &[u8],
    count: usize,
) -> Result<()> {
    put_i32(buf, header_off, checked_i32(count, "curve element count")?);
    put_u64(buf, header_off + 4, data_off as u64);
    buf[data_off..data_off + data.len()].copy_from_slice(data);
    Ok(())
}

/// Write all curve data objects into the buffer.
fn write_curve_data(buf: &mut [u8], anim: &Animation, layout: &CurveDataLayout) -> Result<()> {
    for (tgi, tg) in anim.track_groups.iter().enumerate() {
        for (tti, tt) in tg.transform_tracks.iter().enumerate() {
            let curves = [&tt.orientation, &tt.position, &tt.scale_shear];
            for (ci, curve) in curves.iter().enumerate() {
                let cl = &layout.layouts[tgi][tti][ci];
                write_single_curve(buf, cl, curve)?;
            }
        }
    }
    Ok(())
}

fn write_quantized_d3_curve(
    buf: &mut [u8],
    layout: &CurveObjLayout,
    payload_offset: usize,
    one_over_knot_scale_trunc: u16,
    control_scales: &[f32; 3],
    control_offsets: &[f32; 3],
    knots_controls: &[u8],
) -> Result<()> {
    put_u16(buf, payload_offset, one_over_knot_scale_trunc);
    for (index, scale) in control_scales.iter().enumerate() {
        put_f32(buf, payload_offset + 2 + index * 4, *scale);
    }
    for (index, offset) in control_offsets.iter().enumerate() {
        put_f32(buf, payload_offset + 14 + index * 4, *offset);
    }
    write_ref_arr(
        buf,
        payload_offset + 26,
        layout.ref_arr_offsets[0],
        knots_controls,
        knots_controls.len(),
    )
}

fn write_constant_curve(buf: &mut [u8], payload_offset: usize, padding: u16, controls: &[f32]) {
    put_u16(buf, payload_offset, padding);
    for (index, control) in controls.iter().enumerate() {
        put_f32(buf, payload_offset + 2 + index * 4, *control);
    }
}

/// Write a single curve data object at its planned offset.
fn write_single_curve(buf: &mut [u8], cl: &CurveObjLayout, curve: &CurveData) -> Result<()> {
    let o = cl.obj_offset;
    // Header: format + degree
    buf[o] = curve.format;
    buf[o + 1] = curve.degree;
    let p = o + curve_data_header::SIZE; // payload start

    match &curve.payload {
        CurvePayload::Identity { dimension } => {
            put_u16(buf, p, *dimension);
        }
        CurvePayload::DaConstant32f { padding, controls } => {
            put_u16(buf, p, *padding);
            let f32_bytes: Vec<u8> = controls.iter().flat_map(|v| v.to_le_bytes()).collect();
            write_ref_arr(
                buf,
                p + 2,
                cl.ref_arr_offsets[0],
                &f32_bytes,
                controls.len(),
            )?;
        }
        CurvePayload::D3Constant32f { padding, controls } => {
            write_constant_curve(buf, p, *padding, controls);
        }
        CurvePayload::D4Constant32f { padding, controls } => {
            write_constant_curve(buf, p, *padding, controls);
        }
        CurvePayload::DaK32fC32f {
            padding,
            knots,
            controls,
        } => {
            put_u16(buf, p, *padding);
            let knot_bytes: Vec<u8> = knots.iter().flat_map(|v| v.to_le_bytes()).collect();
            let ctrl_bytes: Vec<u8> = controls.iter().flat_map(|v| v.to_le_bytes()).collect();
            write_ref_arr(buf, p + 2, cl.ref_arr_offsets[0], &knot_bytes, knots.len())?;
            write_ref_arr(
                buf,
                p + 2 + 12,
                cl.ref_arr_offsets[1],
                &ctrl_bytes,
                controls.len(),
            )?;
        }
        CurvePayload::D4nK16uC15u {
            scale_offset_table_entries,
            one_over_knot_scale,
            knots_controls,
        }
        | CurvePayload::D4nK8uC7u {
            scale_offset_table_entries,
            one_over_knot_scale,
            knots_controls,
        } => {
            put_u16(buf, p, *scale_offset_table_entries);
            put_f32(buf, p + 2, *one_over_knot_scale);
            write_ref_arr(
                buf,
                p + 6,
                cl.ref_arr_offsets[0],
                knots_controls,
                knots_controls.len(),
            )?;
        }
        CurvePayload::D3K16uC16u {
            one_over_knot_scale_trunc,
            control_scales,
            control_offsets,
            knots_controls,
        }
        | CurvePayload::D3K8uC8u {
            one_over_knot_scale_trunc,
            control_scales,
            control_offsets,
            knots_controls,
        }
        | CurvePayload::D3I1K8uC8u {
            one_over_knot_scale_trunc,
            control_scales,
            control_offsets,
            knots_controls,
        } => {
            write_quantized_d3_curve(
                buf,
                cl,
                p,
                *one_over_knot_scale_trunc,
                control_scales,
                control_offsets,
                knots_controls,
            )?;
        }
        CurvePayload::Unknown { raw } => {
            buf[p..p + raw.len()].copy_from_slice(raw);
        }
    }
    Ok(())
}
