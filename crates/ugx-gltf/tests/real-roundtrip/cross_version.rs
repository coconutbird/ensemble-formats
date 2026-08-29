//! Bidirectional conversion coverage for checked-in and optional retail models.

use std::path::{Path, PathBuf};

use test_utils::prelude::*;
use ugx::{ReadOptions, Reader, UgxGeom, UgxVersion};
use ugx_gltf::convert_ugx_version_to_bytes;

use super::compare::{RoundtripResult, compare_converted_geometry};

const MAX_RETAIL_FILES: usize = 100;

#[derive(Default)]
struct ConversionStats {
    passed: usize,
    failures: Vec<String>,
}

impl ConversionStats {
    fn record(&mut self, result: RoundtripResult) {
        match result {
            RoundtripResult::Passed => self.passed += 1,
            RoundtripResult::ReadSkipped(error) | RoundtripResult::Failed(error) => {
                self.failures.push(error);
            }
        }
    }

    fn assert_success(&self, label: &str) {
        eprintln!(
            "{label}: {} converted, {} failed",
            self.passed,
            self.failures.len()
        );
        assert!(self.passed > 0, "no {label} models were converted");
        assert!(
            self.failures.is_empty(),
            "{label} failures:\n{}",
            self.failures.join("\n")
        );
    }
}

struct EraEntry {
    archive: PathBuf,
    index: usize,
    name: String,
}

fn workspace_file(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn assert_conversion(label: &str, geometry: &UgxGeom, target: UgxVersion) {
    match compare_converted_geometry(label, geometry, target) {
        RoundtripResult::Passed => {}
        RoundtripResult::ReadSkipped(error) | RoundtripResult::Failed(error) => {
            panic!("{error}")
        }
    }
}

fn convert_and_read(geometry: &UgxGeom, target: UgxVersion) -> UgxGeom {
    let bytes = convert_ugx_version_to_bytes(geometry, target).unwrap();
    assert_eq!(ugx::detect_version(&bytes).unwrap(), target);
    Reader::read(&bytes).unwrap()
}

#[test]
fn checked_in_models_convert_in_both_directions() {
    let hw1_path = workspace_file("launcher_01.ugx");
    let hw1 = Reader::read_with_options(
        &std::fs::read(&hw1_path).unwrap(),
        ReadOptions::unchecked_checksums(),
    )
    .unwrap();
    assert_conversion(&hw1_path.display().to_string(), &hw1, UgxVersion::Hw2);
    let converted_hw2 = convert_and_read(&hw1, UgxVersion::Hw2);
    assert_conversion("converted HW1 fixture", &converted_hw2, UgxVersion::Hw1);

    let hw2_path = workspace_file("input/mesh_magnum_01.ugx");
    let hw2 = Reader::read(&std::fs::read(&hw2_path).unwrap()).unwrap();
    assert_conversion(&hw2_path.display().to_string(), &hw2, UgxVersion::Hw1);
    let converted_hw1 = convert_and_read(&hw2, UgxVersion::Hw1);
    assert_conversion("converted HW2 fixture", &converted_hw1, UgxVersion::Hw2);
}

#[test]
fn retail_hw1_samples_convert_to_hw2() {
    let Some(game_dir) = load_game_dir("HW1_GAME_DIR") else {
        return;
    };
    let entries = collect_hw1_entries(&game_dir);
    assert!(!entries.is_empty(), "no HW1 UGX entries found");
    let mut stats = ConversionStats::default();
    for entry in evenly_spaced(&entries, MAX_RETAIL_FILES) {
        let mut archive = match open_era(&entry.archive) {
            Ok(archive) => archive,
            Err(error) => {
                stats.failures.push(format!(
                    "{}: open archive: {error}",
                    entry.archive.display()
                ));
                continue;
            }
        };
        let label = format!("{}:{}", entry.archive.display(), entry.name);
        let result = archive
            .read_entry(entry.index)
            .map_err(|error| format!("{label}: extract: {error}"))
            .and_then(|bytes| {
                Reader::read(&bytes).map_err(|error| format!("{label}: read: {error}"))
            });
        match result {
            Ok(geometry) => stats.record(compare_converted_geometry(
                &label,
                &geometry,
                UgxVersion::Hw2,
            )),
            Err(error) => stats.failures.push(error),
        }
    }
    stats.assert_success("HW1 v4 to HW2 v6");
}

#[test]
fn retail_hw2_samples_convert_to_hw1() {
    let Some(game_dir) = load_game_dir("HW2_GAME_DIR") else {
        return;
    };
    let files = find_files_by_ext(&game_dir, "ugx");
    assert!(!files.is_empty(), "no HW2 UGX files found");
    let mut stats = ConversionStats::default();
    for path in evenly_spaced(&files, MAX_RETAIL_FILES) {
        let label = path.display().to_string();
        let result = std::fs::read(path)
            .map_err(|error| format!("{label}: read file: {error}"))
            .and_then(|bytes| {
                Reader::read(&bytes).map_err(|error| format!("{label}: parse: {error}"))
            });
        match result {
            Ok(geometry) => stats.record(compare_converted_geometry(
                &label,
                &geometry,
                UgxVersion::Hw1,
            )),
            Err(error) => stats.failures.push(error),
        }
    }
    stats.assert_success("HW2 v6 to HW1 v4");
}

fn collect_hw1_entries(game_dir: &Path) -> Vec<EraEntry> {
    let mut entries = Vec::new();
    for archive_path in find_files_flat(game_dir, "era") {
        let Ok(archive) = open_era(&archive_path) else {
            continue;
        };
        entries.extend(
            find_entries_in_era(&archive, ".ugx")
                .into_iter()
                .map(|(index, name)| EraEntry {
                    archive: archive_path.clone(),
                    index,
                    name,
                }),
        );
    }
    entries
}

fn evenly_spaced<T>(items: &[T], limit: usize) -> Vec<&T> {
    if items.len() <= limit {
        return items.iter().collect();
    }
    if limit <= 1 {
        return items.first().into_iter().collect();
    }
    (0..limit)
        .map(|index| &items[index * (items.len() - 1) / (limit - 1)])
        .collect()
}
