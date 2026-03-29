//! UAX reader implementation.
//!
//! Parses the ECF container and extracts the Granny `file_info` chunk,
//! then traverses the x64 native pointer layout to build high-level
//! [`Animation`] / [`TrackGroup`] / [`TransformTrack`] types.

use alloc::vec::Vec;

use crate::types::*;
use crate::{Error, Result, UAX_CHUNK_ID, UAX_FILE_ID};
use ecf::Reader as EcfReader;

/// UAX file reader.
pub struct Reader;

impl Reader {
    /// Read a UAX animation from raw file bytes.
    ///
    /// Returns the first animation in the file (UAX files always contain
    /// exactly one animation).
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
// Internal parsing
// ============================================================================

/// Parse the file_info structure and extract the first animation.
fn parse_file_info(fi: &[u8]) -> Result<Animation> {
    let anim_count = read_i32_le(fi, file_info::ANIMATION_COUNT).unwrap_or(0);
    if anim_count < 1 {
        return Err(Error::NoAnimations);
    }

    // Animations** — pointer to array of animation pointers
    let anim_arr = read_ptr(fi, file_info::ANIMATIONS_PTR)
        .ok_or(Error::NoAnimations)?;

    // First animation pointer
    let anim_off = read_ptr(fi, anim_arr)
        .ok_or(Error::NoAnimations)?;

    // Parse animation struct
    let name = read_ptr(fi, anim_off + animation::NAME_PTR)
        .and_then(|p| read_cstring(fi, p));
    let duration = read_f32_le(fi, anim_off + animation::DURATION).unwrap_or(0.0);
    let time_step = read_f32_le(fi, anim_off + animation::TIME_STEP).unwrap_or(0.0);
    let oversampling = read_f32_le(fi, anim_off + animation::OVERSAMPLING).unwrap_or(0.0);

    // Parse track groups from file_info level (the canonical list)
    let tg_count = read_i32_le(fi, file_info::TRACK_GROUP_COUNT).unwrap_or(0);
    let mut track_groups = Vec::new();

    if tg_count > 0 {
        if let Some(tg_arr) = read_ptr(fi, file_info::TRACK_GROUPS_PTR) {
            for i in 0..tg_count as usize {
                if let Some(tg_off) = read_ptr(fi, tg_arr + i * 8) {
                    track_groups.push(parse_track_group(fi, tg_off));
                }
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
fn parse_track_group(fi: &[u8], off: usize) -> TrackGroup {
    let name = read_ptr(fi, off + track_group::NAME_PTR)
        .and_then(|p| read_cstring(fi, p));

    let xform_count = read_i32_le(fi, off + track_group::TRANSFORM_TRACK_COUNT).unwrap_or(0);
    let xform_ptr = read_ptr(fi, off + track_group::TRANSFORM_TRACKS_PTR);

    let mut transform_tracks = Vec::new();
    if let Some(base) = xform_ptr {
        for i in 0..xform_count as usize {
            let tt_off = base + i * transform_track::SIZE;
            transform_tracks.push(parse_transform_track(fi, tt_off));
        }
    }

    // LOD errors (array of f32, one per transform track)
    let lod_count = read_i32_le(fi, off + track_group::TRANSFORM_LOD_ERROR_COUNT).unwrap_or(0);
    let lod_ptr = read_ptr(fi, off + track_group::TRANSFORM_LOD_ERRORS_PTR);
    let mut transform_lod_errors = Vec::new();
    if let Some(base) = lod_ptr {
        for i in 0..lod_count as usize {
            if let Some(v) = read_f32_le(fi, base + i * 4) {
                transform_lod_errors.push(v);
            }
        }
    }

    let initial_placement = read_transform(fi, off + track_group::INITIAL_PLACEMENT);
    let flags = read_u32_le(fi, off + track_group::FLAGS).unwrap_or(0);

    TrackGroup {
        name,
        transform_tracks,
        transform_lod_errors,
        initial_placement,
        flags,
    }
}

/// Parse a single transform track at the given offset.
fn parse_transform_track(fi: &[u8], off: usize) -> TransformTrack {
    let name = read_ptr(fi, off + transform_track::NAME_PTR)
        .and_then(|p| read_cstring(fi, p));
    let flags = read_i32_le(fi, off + transform_track::FLAGS).unwrap_or(0);

    let orientation = parse_curve(fi, off + transform_track::ORIENTATION_CURVE);
    let position = parse_curve(fi, off + transform_track::POSITION_CURVE);
    let scale_shear = parse_curve(fi, off + transform_track::SCALE_SHEAR_CURVE);

    TransformTrack { name, flags, orientation, position, scale_shear }
}

/// Parse a granny_curve2 (variant: type_ptr + object_ptr) at offset.
fn parse_curve(fi: &[u8], off: usize) -> CurveData {
    let obj = read_ptr(fi, off + curve2::OBJECT_PTR);

    match obj {
        Some(obj_off) if obj_off + 2 <= fi.len() => {
            let format = fi[obj_off];
            let degree = fi[obj_off + 1];

            // Extract payload — everything after the 2-byte header up to the
            // next aligned structure or a reasonable max. We determine the size
            // from the curve format. For now, capture a bounded raw slice.
            let payload_start = obj_off + curve_data_header::SIZE;
            let payload = extract_curve_payload(fi, format, payload_start);

            CurveData { format, degree, payload }
        }
        _ => CurveData { format: 0, degree: 0, payload: Vec::new() },
    }
}

/// Extract the raw payload bytes for a curve based on its format.
///
/// Granny curve formats have varying payload structures. We capture
/// the raw bytes so the writer can reproduce them exactly.
fn extract_curve_payload(fi: &[u8], format: u8, start: usize) -> Vec<u8> {
    if start >= fi.len() {
        return Vec::new();
    }

    // Payload size depends on format. Known formats:
    //  0 = DaIdentity (no payload)
    //  2 = DaIdentity (no payload — just header)
    //  4 = DaConstant32f (N × f32, typically 3 or 4 floats)
    //  6 = D3Constant32f (3 × f32 = 12 bytes)
    //  8 = D4Constant32f (4 × f32 = 16 bytes)
    // 10 = D4nK16uC15p (variable: knot count header + knots + controls)
    // 11 = DaK32fC32f (variable: dimension + knot/control arrays)
    //
    // For formats with a known fixed size, we extract exactly that.
    // For variable formats, we read the internal size fields.
    let size = match format {
        0 | 2 => 0, // Identity — no payload
        4 => {
            // DaConstant32f: padding(2) + one_over_knot_scale(4) + N×f32
            // Read the dimension from context if needed. For safety, grab 16 bytes.
            if start + 2 <= fi.len() {
                let padding = u16::from_le_bytes([fi[start], fi[start + 1]]) as usize;
                // padding field encodes control count or dimension
                let ctrl_bytes = if padding > 0 && padding <= 16 { padding * 4 } else { 16 };
                2 + 4 + ctrl_bytes // padding(2) + one_over_knot_scale(4) + controls
            } else {
                0
            }
        }
        6 => 2 + 4 + 12,  // padding(2) + ooks(4) + 3×f32
        8 => 2 + 4 + 16,  // padding(2) + ooks(4) + 4×f32
        _ => {
            // Variable-length formats: read knot_count and control_count
            // Layout: padding(2) + knot_count(u16) + control_count(u16) + ...
            if start + 6 <= fi.len() {
                let _padding = u16::from_le_bytes([fi[start], fi[start + 1]]);
                let knot_count = u16::from_le_bytes([fi[start + 2], fi[start + 3]]) as usize;
                let control_count = u16::from_le_bytes([fi[start + 4], fi[start + 5]]) as usize;

                // Determine knot/control element sizes from format
                let (knot_elem, ctrl_elem) = match format {
                    10 => (2, 2), // D4nK16uC15p: u16 knots, u16 controls
                    11 => (4, 4), // DaK32fC32f: f32 knots, f32 controls
                    _ => (4, 4),  // Conservative default
                };

                6 + knot_count * knot_elem + control_count * ctrl_elem
            } else {
                0
            }
        }
    };

    let end = (start + size).min(fi.len());
    fi[start..end].to_vec()
}

#[cfg(test)]
mod tests {
    extern crate std;
    use std::{eprintln, println};

    use super::*;

    #[test]
    fn test_parse_uax_files() {
        let test_files = [
            "../../temp_uax/art/campaign/npc/forge_01/shotgun_attack_01.uax",
            "../../temp_uax/art/campaign/npc/forge_01/shotgun_attack_02.uax",
            "../../temp_uax/art/campaign/npc/forge_01/shotgun_attack_03.uax",
            "../../temp_uax/art/campaign/npc/forge_01/shotgun_reload_01.uax",
        ];

        let mut parsed = 0;
        for test_path in test_files {
            if !std::path::Path::new(test_path).exists() {
                continue;
            }

            let data = std::fs::read(test_path).expect("Failed to read UAX file");
            let anim = Reader::read(&data)
                .unwrap_or_else(|e| panic!("Failed to parse {}: {:?}", test_path, e));

            assert!(anim.duration > 0.0, "Duration should be positive");
            println!("Parsed {}:", test_path.rsplit('/').next().unwrap());
            println!("  Name: {:?}", anim.name);
            println!("  Duration: {:.3}s", anim.duration);
            println!("  Track groups: {}", anim.track_groups.len());
            for tg in &anim.track_groups {
                println!("    '{}' - {} xform tracks, flags=0x{:X}",
                    tg.name.as_deref().unwrap_or("?"),
                    tg.transform_tracks.len(),
                    tg.flags);
                for tt in &tg.transform_tracks {
                    println!("      '{}' O:fmt={}/deg={} P:fmt={}/deg={} S:fmt={}/deg={}",
                        tt.name.as_deref().unwrap_or("?"),
                        tt.orientation.format, tt.orientation.degree,
                        tt.position.format, tt.position.degree,
                        tt.scale_shear.format, tt.scale_shear.degree);
                }
            }
            parsed += 1;
        }

        if parsed == 0 {
            eprintln!("Skipping test - no UAX files found in temp_uax/");
        } else {
            println!("\nSuccessfully parsed {} UAX files", parsed);
        }
    }
}
