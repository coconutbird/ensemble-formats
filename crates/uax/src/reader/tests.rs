extern crate std;

use std::eprintln;

use super::*;
use crate::{UaxFile, Writer};
use test_utils::prelude::*;

#[test]
fn rejects_short_file_info() {
    let mut ecf = ecf::Writer::new(UAX_FILE_ID);
    ecf.add_chunk(UAX_CHUNK_ID, std::vec![0; file_info::SIZE - 1]);
    let error = Reader::read(&ecf.finalize().expect("test ECF should serialize"))
        .expect_err("short file_info must fail");
    assert!(matches!(error, Error::ChunkTooSmall(_, file_info::SIZE)));
}

fn record_curve_formats(animation: &Animation, counts: &mut [usize; 19]) {
    let mut record = |curve: &CurveData| {
        counts[usize::from(curve.format)] += 1;
    };
    for group in &animation.track_groups {
        for track in &group.vector_tracks {
            record(&track.value);
        }
        for track in &group.transform_tracks {
            record(&track.orientation);
            record(&track.position);
            record(&track.scale_shear);
        }
    }
}

fn audit_uax(data: &[u8], filename: &str, formats: &mut [usize; 19]) {
    let animation = Reader::read(data)
        .unwrap_or_else(|error| panic!("strict parse failed for {filename}: {error}"));
    record_curve_formats(&animation, formats);

    let rewritten = Writer::write(&animation)
        .unwrap_or_else(|error| panic!("semantic rewrite failed for {filename}: {error}"));
    let reparsed = Reader::read(&rewritten)
        .unwrap_or_else(|error| panic!("rewritten file failed for {filename}: {error}"));
    assert_eq!(animation, reparsed, "semantic mismatch for {filename}");
    let rewritten_again = Writer::write(&reparsed)
        .unwrap_or_else(|error| panic!("second rewrite failed for {filename}: {error}"));
    assert_eq!(
        rewritten, rewritten_again,
        "serialized float bit-pattern mismatch for {filename}"
    );

    let raw = UaxFile::from_bytes(data)
        .unwrap_or_else(|error| panic!("raw parse failed for {filename}: {error}"));
    let raw_roundtrip = raw
        .to_bytes()
        .unwrap_or_else(|error| panic!("raw rewrite failed for {filename}: {error}"));
    assert_eq!(data, raw_roundtrip, "raw byte mismatch for {filename}");
}

/// Exhaustively verify every UAX in every installed Halo Wars ERA archive.
///
/// This is ignored because it requires `HW1_GAME_DIR` and reads several
/// gigabytes of archive data. Release verification runs it explicitly.
#[test]
#[ignore = "requires HW1_GAME_DIR and scans all installed ERA archives"]
fn verifies_all_installed_hw1_uax_files() {
    let game_dir =
        load_game_dir("HW1_GAME_DIR").expect("HW1_GAME_DIR must name the game directory");
    let era_paths = find_files_flat(&game_dir, "era");
    assert!(!era_paths.is_empty(), "no ERA archives found");

    let mut tested = 0usize;
    let mut archives_with_uax = 0usize;
    let mut formats = [0usize; 19];
    for era_path in &era_paths {
        let mut archive = open_era(era_path)
            .unwrap_or_else(|error| panic!("failed to open {}: {error}", era_path.display()));
        let entries = find_entries_in_era(&archive, ".uax");
        archives_with_uax += usize::from(!entries.is_empty());
        for (index, filename) in entries {
            let data = archive.read_entry(index).unwrap_or_else(|error| {
                panic!(
                    "failed to extract {filename} from {}: {error}",
                    era_path.display()
                )
            });
            audit_uax(&data, &filename, &mut formats);
            tested += 1;
        }
    }

    eprintln!(
        "verified {tested} UAX files from {archives_with_uax}/{} ERA archives; formats={formats:?}",
        era_paths.len()
    );
    assert!(tested > 0, "no UAX files found");
    for (format, count) in formats.iter().enumerate().skip(1) {
        assert!(
            *count > 0,
            "shipped corpus did not exercise format {format}"
        );
    }
}
