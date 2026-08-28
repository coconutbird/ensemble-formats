extern crate std;

use std::format;
use std::vec;

use super::*;
use crate::types::{CurvePayload, TextTrackEntry, read_u64_le};
use crate::{Reader, UaxFile};

fn curve(format: u8, payload: CurvePayload) -> CurveData {
    CurveData {
        format,
        degree: u8::from(format != 2),
        payload,
    }
}

fn all_curves() -> Vec<CurveData> {
    let mut curves = low_curves();
    curves.extend(high_curves());
    curves
}

fn low_curves() -> Vec<CurveData> {
    vec![
        curve(
            0,
            CurvePayload::DaKeyframes32f {
                dimension: 3,
                controls: vec![1.0, 2.0, 3.0],
            },
        ),
        curve(
            1,
            CurvePayload::DaK32fC32f {
                padding: 0,
                knots: vec![0.0, 1.0],
                controls: vec![1.0, 2.0, 3.0, 4.0],
            },
        ),
        curve(2, CurvePayload::Identity { dimension: 3 }),
        curve(
            3,
            CurvePayload::DaConstant32f {
                padding: 0,
                controls: vec![3.5, -2.0],
            },
        ),
        curve(
            4,
            CurvePayload::D3Constant32f {
                padding: 0,
                controls: [1.0, 2.0, 3.0],
            },
        ),
        curve(
            5,
            CurvePayload::D4Constant32f {
                padding: 0,
                controls: [0.0, 0.0, 0.0, 1.0],
            },
        ),
        curve(
            6,
            CurvePayload::DaK16uC16u {
                one_over_knot_scale_trunc: 17,
                control_scale_offsets: vec![1.0, -1.0, 2.0, -2.0],
                knots_controls: vec![0, 10, 20, u16::MAX],
            },
        ),
        curve(
            7,
            CurvePayload::DaK8uC8u {
                one_over_knot_scale_trunc: 19,
                control_scale_offsets: vec![1.0, -1.0],
                knots_controls: vec![0, 10, 20, u8::MAX],
            },
        ),
        curve(
            8,
            CurvePayload::D4nK16uC15u {
                scale_offset_table_entries: 4,
                one_over_knot_scale: 0.25,
                knots_controls: vec![0, 100, 200, u16::MAX],
            },
        ),
        curve(
            9,
            CurvePayload::D4nK8uC7u {
                scale_offset_table_entries: 4,
                one_over_knot_scale: 0.5,
                knots_controls: vec![0, 10, 20, u8::MAX],
            },
        ),
    ]
}

fn high_curves() -> Vec<CurveData> {
    let scales = [1.25, 2.5, 5.0];
    let offsets = [-1.0, 0.5, 3.0];
    vec![
        curve(
            10,
            CurvePayload::D3K16uC16u {
                one_over_knot_scale_trunc: 23,
                control_scales: scales,
                control_offsets: offsets,
                knots_controls: vec![0, 1, 2, u16::MAX],
            },
        ),
        curve(
            11,
            CurvePayload::D3K8uC8u {
                one_over_knot_scale_trunc: 29,
                control_scales: scales,
                control_offsets: offsets,
                knots_controls: vec![0, 1, 2, u8::MAX],
            },
        ),
        curve(
            12,
            CurvePayload::D9I1K16uC16u {
                one_over_knot_scale_trunc: 31,
                control_scale: 2.0,
                control_offset: -4.0,
                knots_controls: vec![0, 10, u16::MAX],
            },
        ),
        curve(
            13,
            CurvePayload::D9I3K16uC16u {
                one_over_knot_scale_trunc: 37,
                control_scales: scales,
                control_offsets: offsets,
                knots_controls: vec![0, 3, 6, u16::MAX],
            },
        ),
        curve(
            14,
            CurvePayload::D9I1K8uC8u {
                one_over_knot_scale_trunc: 41,
                control_scale: 4.0,
                control_offset: -8.0,
                knots_controls: vec![0, 10, u8::MAX],
            },
        ),
        curve(
            15,
            CurvePayload::D9I3K8uC8u {
                one_over_knot_scale_trunc: 43,
                control_scales: scales,
                control_offsets: offsets,
                knots_controls: vec![0, 3, 6, u8::MAX],
            },
        ),
        curve(
            16,
            CurvePayload::D3I1K32fC32f {
                padding: 0,
                control_scales: scales,
                control_offsets: offsets,
                knots_controls: vec![0.0, 0.25, 0.5, 1.0],
            },
        ),
        curve(
            17,
            CurvePayload::D3I1K16uC16u {
                one_over_knot_scale_trunc: 47,
                control_scales: scales,
                control_offsets: offsets,
                knots_controls: vec![0, 7, 14, u16::MAX],
            },
        ),
        curve(
            18,
            CurvePayload::D3I1K8uC8u {
                one_over_knot_scale_trunc: 53,
                control_scales: scales,
                control_offsets: offsets,
                knots_controls: vec![0, 7, 14, u8::MAX],
            },
        ),
    ]
}

fn complete_animation() -> Animation {
    let curves = all_curves();
    let vector_tracks = curves
        .iter()
        .enumerate()
        .map(|(index, value)| VectorTrack {
            name: Some(format!("vector-{index}")),
            track_key: u32::try_from(index).expect("test index fits u32"),
            dimension: 3,
            value: value.clone(),
        })
        .collect();
    Animation {
        name: Some("complete-animation".into()),
        duration: 2.5,
        time_step: 1.0 / 30.0,
        oversampling: 1.0,
        default_loop_count: 3,
        flags: 0x1234_5678,
        track_groups: vec![TrackGroup {
            name: Some("complete-group".into()),
            vector_tracks,
            transform_tracks: vec![TransformTrack {
                name: Some("root".into()),
                flags: -7,
                orientation: curves[5].clone(),
                position: curves[4].clone(),
                scale_shear: curves[16].clone(),
            }],
            transform_lod_errors: vec![0.125, 0.5],
            text_tracks: vec![TextTrack {
                name: Some("events".into()),
                entries: vec![
                    TextTrackEntry {
                        time_stamp: 0.25,
                        text: Some("footstep".into()),
                    },
                    TextTrackEntry {
                        time_stamp: 1.5,
                        text: None,
                    },
                ],
            }],
            initial_placement: Transform {
                flags: 7,
                position: [1.0, 2.0, 3.0],
                orientation: [0.0, 0.0, 0.0, 1.0],
                scale_shear: [1.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 3.0],
            },
            flags: 6,
            loop_translation: [4.0, 5.0, 6.0],
            periodic_loop: Some(PeriodicLoop {
                radius: 2.0,
                d_angle: 0.75,
                d_z: -1.0,
                basis_x: [1.0, 0.0, 0.0],
                basis_y: [0.0, 1.0, 0.0],
                axis: [0.0, 0.0, 1.0],
            }),
        }],
    }
}

fn wrap_chunk(chunk: Vec<u8>) -> Vec<u8> {
    let mut ecf = ecf::Writer::new(UAX_FILE_ID);
    ecf.add_chunk(UAX_CHUNK_ID, chunk);
    ecf.finalize().expect("test ECF should serialize")
}

#[test]
fn roundtrips_every_engine_curve_format_and_track_kind() {
    let expected = complete_animation();
    let bytes = Writer::write(&expected).expect("complete animation should serialize");
    let actual = Reader::read(&bytes).expect("writer output should parse");
    assert_eq!(actual, expected);
    assert_eq!(
        Writer::write(&actual).expect("parsed animation should reserialize"),
        bytes,
        "all serialized float bit patterns must be stable"
    );
}

#[test]
fn roundtrips_animation_without_track_groups() {
    let mut expected = complete_animation();
    expected.track_groups.clear();
    let bytes = Writer::write(&expected).expect("empty track-group list should serialize");
    let actual = Reader::read(&bytes).expect("writer output should parse");
    assert_eq!(actual, expected);
    assert_eq!(
        Writer::write(&actual).expect("parsed animation should reserialize"),
        bytes,
        "serialized float bit patterns must be stable"
    );
}

#[test]
fn preserves_float_bit_patterns() {
    const NAN_BITS: u32 = 0x7FC0_1234;
    let mut animation = complete_animation();
    animation.duration = f32::from_bits(NAN_BITS);
    animation.time_step = -0.0;
    if let CurvePayload::DaKeyframes32f { controls, .. } =
        &mut animation.track_groups[0].vector_tracks[0].value.payload
    {
        controls[0] = f32::from_bits(NAN_BITS);
    }

    let bytes = Writer::write(&animation).expect("special floats should serialize");
    let actual = Reader::read(&bytes).expect("special floats should parse");
    assert_eq!(actual.duration.to_bits(), NAN_BITS);
    assert_eq!(actual.time_step.to_bits(), (-0.0_f32).to_bits());
    let CurvePayload::DaKeyframes32f { controls, .. } =
        &actual.track_groups[0].vector_tracks[0].value.payload
    else {
        panic!("format 0 payload changed variant");
    };
    assert_eq!(controls[0].to_bits(), NAN_BITS);
}

#[test]
fn rejects_curve_format_payload_mismatch() {
    let mut animation = complete_animation();
    animation.track_groups[0].vector_tracks[0].value.format = 18;
    let error = Writer::write(&animation).expect_err("mismatched curve must fail");
    assert!(matches!(error, Error::CurvePayloadMismatch { .. }));
}

#[test]
fn rejects_embedded_nul() {
    let mut animation = complete_animation();
    animation.name = Some("bad\0name".into());
    let error = Writer::write(&animation).expect_err("embedded NUL must fail");
    assert!(matches!(error, Error::EmbeddedNul("animation name")));
}

#[test]
fn reader_rejects_out_of_range_animation_pointer() {
    let mut chunk = build_file_info(&complete_animation()).expect("test animation should plan");
    chunk[file_info::ANIMATIONS_PTR..file_info::ANIMATIONS_PTR + 8]
        .copy_from_slice(&u64::MAX.to_le_bytes());
    let error = Reader::read(&wrap_chunk(chunk)).expect_err("invalid pointer must fail");
    assert!(matches!(error, Error::InvalidPointerOffset(u64::MAX, _)));
}

#[test]
fn reader_rejects_negative_animation_count() {
    let mut chunk = build_file_info(&complete_animation()).expect("test animation should plan");
    chunk[file_info::ANIMATION_COUNT..file_info::ANIMATION_COUNT + 4]
        .copy_from_slice(&(-1_i32).to_le_bytes());
    let error = Reader::read(&wrap_chunk(chunk)).expect_err("negative count must fail");
    assert!(matches!(error, Error::InvalidCount("animation count", -1)));
}

#[test]
fn raw_file_rejects_wrong_from_file_name() {
    let mut chunk = build_file_info(&complete_animation()).expect("test animation should plan");
    let string_offset = usize::try_from(
        read_u64_le(&chunk, file_info::FROM_FILE_NAME_PTR).expect("writer emits string pointer"),
    )
    .expect("test pointer fits usize");
    chunk[string_offset] = b'x';
    let error = UaxFile::from_bytes(&wrap_chunk(chunk)).expect_err("invalid marker must fail");
    assert!(matches!(error, Error::InvalidFromFileName(_)));
}

#[test]
fn raw_animation_layout_matches_ida() {
    assert_eq!(
        core::mem::size_of::<crate::types::AnimationRaw>(),
        animation::SIZE
    );
}
