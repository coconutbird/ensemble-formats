//! UAX reader implementation.
//!
//! Parses the ECF container and extracts the Granny `file_info` chunk,
//! then traverses the x64 native pointer layout to build high-level
//! [`Animation`] / [`TrackGroup`] / [`TransformTrack`] types.
//!
//! Uses [`nostdio::Cursor`] and [`nostdio::ReadLe`] for all binary
//! field access.

use alloc::vec::Vec;

use nostdio::{Cursor, ReadLe, Seek, SeekFrom};

use crate::types::{
    Animation, CurveData, CurvePayload, TrackGroup, Transform, TransformTrack, animation,
    curve_data_header, curve2, file_info, track_group, transform, transform_track,
};
use crate::{Error, Result, UAX_CHUNK_ID, UAX_FILE_ID};
use ecf::Reader as EcfReader;

/// UAX file reader.
pub struct Reader;

impl Reader {
    /// Read a UAX animation from raw file bytes.
    ///
    /// Returns the first animation in the file (UAX files always contain
    /// exactly one animation).
    ///
    /// # Errors
    ///
    /// Returns an error if the ECF container is invalid, the UAX chunk is
    /// missing or truncated, or no animation can be resolved.
    pub fn read(data: &[u8]) -> Result<Animation> {
        let ecf = EcfReader::new(data)?;

        // Validate file ID
        let hdr_id = ecf.header().id;
        if hdr_id != UAX_FILE_ID {
            return Err(Error::InvalidFileId(hdr_id));
        }

        // Find chunk 0x0700
        let chunk_idx = ecf
            .chunks()
            .iter()
            .position(|c| c.id == UAX_CHUNK_ID)
            .ok_or(Error::ChunkNotFound)?;

        let fi = ecf.chunk_data(chunk_idx)?;

        // Chunk data IS file_info (no header to skip)
        if fi.len() < file_info::MIN_SIZE {
            return Err(Error::ChunkTooSmall(fi.len(), file_info::MIN_SIZE));
        }

        parse_file_info(&fi)
    }
}

// ============================================================================
// Cursor helpers
// ============================================================================

/// Seek to `off` and read a u64 LE, returning `None` on failure.
fn cursor_u64(c: &mut Cursor<&[u8]>, off: usize) -> Option<u64> {
    c.seek(SeekFrom::Start(off as u64)).ok()?;
    c.read_u64_le().ok()
}

/// Read a Granny pointer (u64 LE offset) and validate it as an in-bounds offset.
fn cursor_ptr(c: &mut Cursor<&[u8]>, off: usize) -> Option<usize> {
    let v = usize::try_from(cursor_u64(c, off)?).ok()?;
    if v == 0 || v >= c.get_ref().len() {
        None
    } else {
        Some(v)
    }
}

/// Seek to `off` and read an i32 LE, returning `None` on failure.
fn cursor_i32(c: &mut Cursor<&[u8]>, off: usize) -> Option<i32> {
    c.seek(SeekFrom::Start(off as u64)).ok()?;
    c.read_i32_le().ok()
}

/// Seek to `off` and read a u32 LE, returning `None` on failure.
fn cursor_u32(c: &mut Cursor<&[u8]>, off: usize) -> Option<u32> {
    c.seek(SeekFrom::Start(off as u64)).ok()?;
    c.read_u32_le().ok()
}

/// Seek to `off` and read an f32 LE, returning `None` on failure.
fn cursor_f32(c: &mut Cursor<&[u8]>, off: usize) -> Option<f32> {
    c.seek(SeekFrom::Start(off as u64)).ok()?;
    c.read_f32_le().ok()
}

/// Read a null-terminated C string at `off`.
fn cursor_cstring(c: &mut Cursor<&[u8]>, off: usize) -> Option<alloc::string::String> {
    if off >= c.get_ref().len() {
        return None;
    }
    let s = nostdio::read_null_terminated_string(&c.get_ref()[off..]);
    if s.is_empty() { None } else { Some(s) }
}

/// Read a Granny `ref_arr` (i32 count + u64 ptr) at the cursor's current
/// position after seeking to `off`.  Returns `(count, data_offset)`.
fn cursor_ref_arr(c: &mut Cursor<&[u8]>, off: usize) -> Option<(usize, usize)> {
    c.seek(SeekFrom::Start(off as u64)).ok()?;
    let count = usize::try_from(c.read_i32_le().ok()?).ok()?;
    let ptr = usize::try_from(c.read_u64_le().ok()?).ok()?;
    if count == 0 || ptr == 0 || ptr >= c.get_ref().len() {
        return None;
    }
    Some((count, ptr))
}

// ============================================================================
// Internal parsing
// ============================================================================

/// Parse the `file_info` structure and extract the first animation.
fn parse_file_info(fi: &[u8]) -> Result<Animation> {
    let mut c = Cursor::new(fi);

    let anim_count = cursor_i32(&mut c, file_info::ANIMATION_COUNT).unwrap_or(0);
    if anim_count < 1 {
        return Err(Error::NoAnimations);
    }

    let anim_arr = cursor_ptr(&mut c, file_info::ANIMATIONS_PTR).ok_or(Error::NoAnimations)?;
    let anim_off = cursor_ptr(&mut c, anim_arr).ok_or(Error::NoAnimations)?;

    // Parse animation struct
    let name =
        cursor_ptr(&mut c, anim_off + animation::NAME_PTR).and_then(|p| cursor_cstring(&mut c, p));
    let duration = cursor_f32(&mut c, anim_off + animation::DURATION).unwrap_or(0.0);
    let time_step = cursor_f32(&mut c, anim_off + animation::TIME_STEP).unwrap_or(0.0);
    let oversampling = cursor_f32(&mut c, anim_off + animation::OVERSAMPLING).unwrap_or(0.0);

    // Parse track groups from file_info level (the canonical list)
    let tg_count = cursor_i32(&mut c, file_info::TRACK_GROUP_COUNT).unwrap_or(0);
    let mut track_groups = Vec::new();

    if tg_count > 0
        && let Some(tg_arr) = cursor_ptr(&mut c, file_info::TRACK_GROUPS_PTR)
    {
        for i in 0..usize::try_from(tg_count).unwrap_or_default() {
            if let Some(tg_off) = cursor_ptr(&mut c, tg_arr + i * 8) {
                track_groups.push(parse_track_group(&mut c, tg_off));
            }
        }
    }

    Ok(Animation {
        name,
        duration,
        time_step,
        oversampling,
        track_groups,
    })
}

/// Parse a single track group at the given offset.
fn parse_track_group(c: &mut Cursor<&[u8]>, off: usize) -> TrackGroup {
    let name = cursor_ptr(c, off + track_group::NAME_PTR).and_then(|p| cursor_cstring(c, p));

    let xform_count = cursor_i32(c, off + track_group::TRANSFORM_TRACK_COUNT).unwrap_or(0);
    let xform_ptr = cursor_ptr(c, off + track_group::TRANSFORM_TRACKS_PTR);

    let mut transform_tracks = Vec::new();
    if let Some(base) = xform_ptr {
        for i in 0..usize::try_from(xform_count).unwrap_or_default() {
            let tt_off = base + i * transform_track::SIZE;
            transform_tracks.push(parse_transform_track(c, tt_off));
        }
    }

    // LOD errors (array of f32, one per transform track)
    let lod_count = cursor_i32(c, off + track_group::TRANSFORM_LOD_ERROR_COUNT).unwrap_or(0);
    let lod_ptr = cursor_ptr(c, off + track_group::TRANSFORM_LOD_ERRORS_PTR);
    let mut transform_lod_errors = Vec::new();
    if let Some(base) = lod_ptr {
        for i in 0..usize::try_from(lod_count).unwrap_or_default() {
            if let Some(v) = cursor_f32(c, base + i * 4) {
                transform_lod_errors.push(v);
            }
        }
    }

    let initial_placement = read_transform(c, off + track_group::INITIAL_PLACEMENT);
    let flags = cursor_u32(c, off + track_group::FLAGS).unwrap_or(0);

    TrackGroup {
        name,
        transform_tracks,
        transform_lod_errors,
        initial_placement,
        flags,
    }
}

/// Parse a single transform track at the given offset.
fn parse_transform_track(c: &mut Cursor<&[u8]>, off: usize) -> TransformTrack {
    let name = cursor_ptr(c, off + transform_track::NAME_PTR).and_then(|p| cursor_cstring(c, p));
    let flags = cursor_i32(c, off + transform_track::FLAGS).unwrap_or(0);

    let orientation = parse_curve(c, off + transform_track::ORIENTATION_CURVE);
    let position = parse_curve(c, off + transform_track::POSITION_CURVE);
    let scale_shear = parse_curve(c, off + transform_track::SCALE_SHEAR_CURVE);

    TransformTrack {
        name,
        flags,
        orientation,
        position,
        scale_shear,
    }
}

/// Parse a `granny_curve2` (variant: `type_ptr` + `object_ptr`) at offset.
fn parse_curve(c: &mut Cursor<&[u8]>, off: usize) -> CurveData {
    let obj = cursor_ptr(c, off + curve2::OBJECT_PTR);

    match obj {
        Some(obj_off) if obj_off + 2 <= c.get_ref().len() => {
            let format = c.get_ref()[obj_off];
            let degree = c.get_ref()[obj_off + 1];
            let payload_start = obj_off + curve_data_header::SIZE;
            let payload = parse_curve_payload(c, format, payload_start);

            CurveData {
                format,
                degree,
                payload,
            }
        }
        _ => CurveData {
            format: 0,
            degree: 0,
            payload: CurvePayload::Identity { dimension: 0 },
        },
    }
}

/// Parse the typed curve payload based on the Granny format ID.
///
/// Layout reference (from embedded type trees, verified across 30+ files):
///
/// | fmt | Type           | Fixed bytes | Variable           |
/// |-----|----------------|-------------|--------------------|
/// |   2 | DaIdentity     |  2 (u16)    | —                  |
/// |   3 | DaConstant32f  | 14 (pad+ref)| f32[] via ref_arr  |
/// |   4 | D3Constant32f  | 14 (pad+3f) | —                  |
/// |   5 | D4Constant32f  | 18 (pad+4f) | —                  |
/// |   1 | DaK32fC32f     | 26 (pad+2×ref)| f32[] via 2 ref_arrs |
/// |   8 | D4nK16uC15u    | 18 (u16+f32+ref) | u8[] via ref_arr |
/// |   9 | D4nK8uC7u      | 18 (u16+f32+ref) | u8[] via ref_arr |
/// |  10 | D3K16uC16u     | 38 (u16+3f+3f+ref) | u8[] via ref_arr |
/// |  11 | D3K8uC8u       | 38 (u16+3f+3f+ref) | u8[] via ref_arr |
/// |  18 | D3I1K8uC8u     | 38 (u16+3f+3f+ref) | u8[] via ref_arr |
fn parse_curve_payload(c: &mut Cursor<&[u8]>, format: u8, start: usize) -> CurvePayload {
    let fi = c.get_ref();
    if start >= fi.len() {
        return CurvePayload::Unknown { raw: Vec::new() };
    }

    match format {
        // ── Format 2: DaIdentity ────────────────────────────────────
        // Layout: Dimension(u16) — 2 bytes total after header
        2 => {
            let dim = seek_read_u16(c, start).unwrap_or(0);
            CurvePayload::Identity { dimension: dim }
        }

        // ── Format 3: DaConstant32f ─────────────────────────────────
        // Layout: Padding(u16) + Controls(ref_arr → f32[])
        3 => {
            let padding = seek_read_u16(c, start).unwrap_or(0);
            let controls = read_f32_ref_arr(c, start + 2);
            CurvePayload::DaConstant32f { padding, controls }
        }

        // ── Format 4: D3Constant32f ─────────────────────────────────
        // Layout: Padding(u16) + Controls(f32×3)
        4 => {
            let padding = seek_read_u16(c, start).unwrap_or(0);
            let controls = read_f32x3(c, start + 2);
            CurvePayload::D3Constant32f { padding, controls }
        }

        // ── Format 5: D4Constant32f ─────────────────────────────────
        // Layout: Padding(u16) + Controls(f32×4)
        5 => {
            let padding = seek_read_u16(c, start).unwrap_or(0);
            let controls = read_f32x4(c, start + 2);
            CurvePayload::D4Constant32f { padding, controls }
        }

        // ── Format 1: DaK32fC32f ────────────────────────────────────
        // Layout: Padding(u16) + Knots(ref_arr → f32[]) + Controls(ref_arr → f32[])
        1 => {
            let padding = seek_read_u16(c, start).unwrap_or(0);
            let knots = read_f32_ref_arr(c, start + 2);
            let controls = read_f32_ref_arr(c, start + 2 + 12);
            CurvePayload::DaK32fC32f {
                padding,
                knots,
                controls,
            }
        }

        // ── Format 8: D4nK16uC15u ──────────────────────────────────
        // Layout: ScaleOffsetTableEntries(u16) + OneOverKnotScale(f32) + KnotsControls(ref_arr → u8[])
        8 => {
            let sote = seek_read_u16(c, start).unwrap_or(0);
            let ooks = cursor_f32(c, start + 2).unwrap_or(0.0);
            let kc = read_u8_ref_arr(c, start + 6);
            CurvePayload::D4nK16uC15u {
                scale_offset_table_entries: sote,
                one_over_knot_scale: ooks,
                knots_controls: kc,
            }
        }

        // ── Format 9: D4nK8uC7u ────────────────────────────────────
        // Same layout as format 8
        9 => {
            let sote = seek_read_u16(c, start).unwrap_or(0);
            let ooks = cursor_f32(c, start + 2).unwrap_or(0.0);
            let kc = read_u8_ref_arr(c, start + 6);
            CurvePayload::D4nK8uC7u {
                scale_offset_table_entries: sote,
                one_over_knot_scale: ooks,
                knots_controls: kc,
            }
        }

        // ── Format 10: D3K16uC16u ──────────────────────────────────
        // Layout: OneOverKnotScaleTrunc(u16) + ControlScales(f32×3) + ControlOffsets(f32×3) + KnotsControls(ref_arr → u8[])
        10 => {
            let ooks_trunc = seek_read_u16(c, start).unwrap_or(0);
            let cs = read_f32x3(c, start + 2);
            let co = read_f32x3(c, start + 14);
            let kc = read_u8_ref_arr(c, start + 26);
            CurvePayload::D3K16uC16u {
                one_over_knot_scale_trunc: ooks_trunc,
                control_scales: cs,
                control_offsets: co,
                knots_controls: kc,
            }
        }

        // ── Format 11: D3K8uC8u ────────────────────────────────────
        // Same layout as format 10
        11 => {
            let ooks_trunc = seek_read_u16(c, start).unwrap_or(0);
            let cs = read_f32x3(c, start + 2);
            let co = read_f32x3(c, start + 14);
            let kc = read_u8_ref_arr(c, start + 26);
            CurvePayload::D3K8uC8u {
                one_over_knot_scale_trunc: ooks_trunc,
                control_scales: cs,
                control_offsets: co,
                knots_controls: kc,
            }
        }

        // ── Format 18: D3I1K8uC8u ──────────────────────────────────
        // Same layout as format 10/11
        18 => {
            let ooks_trunc = seek_read_u16(c, start).unwrap_or(0);
            let cs = read_f32x3(c, start + 2);
            let co = read_f32x3(c, start + 14);
            let kc = read_u8_ref_arr(c, start + 26);
            CurvePayload::D3I1K8uC8u {
                one_over_knot_scale_trunc: ooks_trunc,
                control_scales: cs,
                control_offsets: co,
                knots_controls: kc,
            }
        }

        // ── Unknown format ──────────────────────────────────────────
        _ => {
            // Capture up to 64 raw bytes so the writer can still round-trip
            let end = (start + 64).min(fi.len());
            CurvePayload::Unknown {
                raw: fi[start..end].to_vec(),
            }
        }
    }
}

// ============================================================================
// Payload read helpers
// ============================================================================

/// Seek to `off` and read a u16 LE.
fn seek_read_u16(c: &mut Cursor<&[u8]>, off: usize) -> Option<u16> {
    c.seek(SeekFrom::Start(off as u64)).ok()?;
    c.read_u16_le().ok()
}

/// Read 3 consecutive f32 LE values starting at `off`.
fn read_f32x3(c: &mut Cursor<&[u8]>, off: usize) -> [f32; 3] {
    [
        cursor_f32(c, off).unwrap_or(0.0),
        cursor_f32(c, off + 4).unwrap_or(0.0),
        cursor_f32(c, off + 8).unwrap_or(0.0),
    ]
}

/// Read 4 consecutive f32 LE values starting at `off`.
fn read_f32x4(c: &mut Cursor<&[u8]>, off: usize) -> [f32; 4] {
    [
        cursor_f32(c, off).unwrap_or(0.0),
        cursor_f32(c, off + 4).unwrap_or(0.0),
        cursor_f32(c, off + 8).unwrap_or(0.0),
        cursor_f32(c, off + 12).unwrap_or(0.0),
    ]
}

/// Read a Granny `ReferenceToArray` (i32 count + u64 ptr) at `off`, then
/// copy the referenced f32 values.
fn read_f32_ref_arr(c: &mut Cursor<&[u8]>, off: usize) -> Vec<f32> {
    let Some((count, data_off)) = cursor_ref_arr(c, off) else {
        return Vec::new();
    };
    let mut v = Vec::with_capacity(count);
    for i in 0..count {
        v.push(cursor_f32(c, data_off + i * 4).unwrap_or(0.0));
    }
    v
}

/// Read a Granny `ReferenceToArray` (i32 count + u64 ptr) at `off`, then
/// copy the referenced raw bytes.
fn read_u8_ref_arr(c: &mut Cursor<&[u8]>, off: usize) -> Vec<u8> {
    let Some((count, data_off)) = cursor_ref_arr(c, off) else {
        return Vec::new();
    };
    let fi = c.get_ref();
    let end = (data_off + count).min(fi.len());
    fi[data_off..end].to_vec()
}

/// Read a Granny transform from `fi` at `offset` using cursor reads.
fn read_transform(c: &mut Cursor<&[u8]>, offset: usize) -> Transform {
    let flags = cursor_u32(c, offset + transform::FLAGS).unwrap_or(0);
    let position = read_f32x3(c, offset + transform::POSITION);
    let orientation = read_f32x4(c, offset + transform::ORIENTATION);
    let mut scale_shear = [0.0f32; 9];
    for (i, val) in scale_shear.iter_mut().enumerate() {
        *val = cursor_f32(c, offset + transform::SCALE_SHEAR + i * 4).unwrap_or(if i % 4 == 0 {
            1.0
        } else {
            0.0
        });
    }
    Transform {
        flags,
        position,
        orientation,
        scale_shear,
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use std::eprintln;

    use test_utils::prelude::*;

    use super::*;

    /// Max loose UAX files to test (keeps CI fast).
    const MAX_FILES: usize = 50;

    // -----------------------------------------------------------------------
    // HW1 — extract .uax from ERA archives
    // -----------------------------------------------------------------------

    #[test]
    fn test_hw1_era_parse() {
        let Some(game_dir) = load_game_dir("HW1_GAME_DIR") else {
            return;
        };

        let era_paths = find_files_flat(&game_dir, "era");
        if era_paths.is_empty() {
            eprintln!("No .era files in {} — skipping", game_dir.display());
            return;
        }

        let mut tested = 0usize;
        let mut errors = std::vec::Vec::new();

        for era_path in &era_paths {
            let Ok(mut archive) = open_era(era_path) else {
                continue;
            };
            let entries = find_entries_in_era(&archive, ".uax");
            for (idx, filename) in &entries {
                if tested >= MAX_FILES {
                    break;
                }
                let Ok(data) = archive.read_entry(*idx) else {
                    continue;
                };
                match Reader::read(&data) {
                    Ok(anim) => {
                        assert!(anim.duration >= 0.0, "{filename}: negative duration");
                        assert!(!anim.track_groups.is_empty(), "{filename}: no track groups");
                        tested += 1;
                    }
                    Err(e) => errors.push(std::format!("{filename}: {e:?}")),
                }
            }
            if tested >= MAX_FILES {
                break;
            }
        }

        eprintln!("HW1 ERA: parsed {tested} UAX files");
        assert!(tested > 0, "No HW1 UAX files found across any ERA");
        assert!(errors.is_empty(), "Parse failures:\n{}", errors.join("\n"));
    }

    // -----------------------------------------------------------------------
    // HW2 — read loose .uax files
    // -----------------------------------------------------------------------

    #[test]
    fn test_hw2_loose_parse() {
        let Some(game_dir) = load_game_dir("HW2_GAME_DIR") else {
            return;
        };

        let uax_files = find_files_by_ext(&game_dir, "uax");
        if uax_files.is_empty() {
            eprintln!("No .uax files in {} — skipping", game_dir.display());
            return;
        }

        let mut tested = 0usize;
        let mut errors = std::vec::Vec::new();

        for path in uax_files.iter().take(MAX_FILES) {
            let Ok(data) = std::fs::read(path) else {
                continue;
            };
            match Reader::read(&data) {
                Ok(anim) => {
                    assert!(
                        anim.duration >= 0.0,
                        "{}: negative duration",
                        path.display()
                    );
                    assert!(
                        !anim.track_groups.is_empty(),
                        "{}: no track groups",
                        path.display()
                    );
                    tested += 1;
                }
                Err(e) => errors.push(std::format!("{}: {e:?}", path.display())),
            }
        }

        eprintln!("HW2 loose: parsed {tested} UAX files");
        assert!(tested > 0, "No HW2 UAX files tested");
        assert!(errors.is_empty(), "Parse failures:\n{}", errors.join("\n"));
    }

    // -----------------------------------------------------------------------
    // HW1 — parse → serialize → re-parse roundtrip
    // -----------------------------------------------------------------------

    #[test]
    fn test_hw1_era_roundtrip() {
        let Some(game_dir) = load_game_dir("HW1_GAME_DIR") else {
            return;
        };

        let era_paths = find_files_flat(&game_dir, "era");
        if era_paths.is_empty() {
            eprintln!("No .era files — skipping");
            return;
        }

        let mut tested = 0usize;
        let mut errors = std::vec::Vec::new();

        for era_path in &era_paths {
            let Ok(mut archive) = open_era(era_path) else {
                continue;
            };
            let entries = find_entries_in_era(&archive, ".uax");
            for (idx, filename) in &entries {
                if tested >= MAX_FILES {
                    break;
                }
                let Ok(data) = archive.read_entry(*idx) else {
                    continue;
                };
                match roundtrip_check(&data) {
                    Ok(()) => tested += 1,
                    Err(e) => errors.push(std::format!("{filename}: {e}")),
                }
            }
            if tested >= MAX_FILES {
                break;
            }
        }

        eprintln!("HW1 ERA roundtrip: {tested} files");
        assert!(tested > 0, "No HW1 UAX files roundtripped");
        assert!(
            errors.is_empty(),
            "Roundtrip failures:\n{}",
            errors.join("\n")
        );
    }

    // -----------------------------------------------------------------------
    // HW2 — parse → serialize → re-parse roundtrip
    // -----------------------------------------------------------------------

    #[test]
    fn test_hw2_loose_roundtrip() {
        let Some(game_dir) = load_game_dir("HW2_GAME_DIR") else {
            return;
        };

        let uax_files = find_files_by_ext(&game_dir, "uax");
        if uax_files.is_empty() {
            eprintln!("No .uax files — skipping");
            return;
        }

        let mut tested = 0usize;
        let mut errors = std::vec::Vec::new();

        for path in uax_files.iter().take(MAX_FILES) {
            let Ok(data) = std::fs::read(path) else {
                continue;
            };
            match roundtrip_check(&data) {
                Ok(()) => tested += 1,
                Err(e) => errors.push(std::format!("{}: {e}", path.display())),
            }
        }

        eprintln!("HW2 loose roundtrip: {tested} files");
        assert!(tested > 0, "No HW2 UAX files roundtripped");
        assert!(
            errors.is_empty(),
            "Roundtrip failures:\n{}",
            errors.join("\n")
        );
    }

    /// Parse → `Writer::write` → re-parse, compare Animation structs.
    fn roundtrip_check(data: &[u8]) -> std::result::Result<(), std::string::String> {
        use crate::Writer;

        let anim1 = Reader::read(data).map_err(|e| std::format!("parse1: {e:?}"))?;
        let written = Writer::write(&anim1).map_err(|e| std::format!("write: {e:?}"))?;
        let anim2 = Reader::read(&written).map_err(|e| std::format!("parse2: {e:?}"))?;

        if anim1 != anim2 {
            return Err(std::format!(
                "mismatch: name={:?} tg={} vs {} duration={} vs {}",
                anim1.name,
                anim1.track_groups.len(),
                anim2.track_groups.len(),
                anim1.duration,
                anim2.duration,
            ));
        }
        Ok(())
    }
}
