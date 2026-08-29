use std::path::Path;

use test_utils::prelude::*;
use ugx::UgxVersion;

use super::compare::{RoundtripResult, roundtrip_bytes};

const MAX_FILES: usize = 100;

#[derive(Default)]
struct RunStats {
    passed: usize,
    skipped: usize,
    failures: Vec<String>,
}

impl RunStats {
    fn attempted(&self) -> usize {
        self.passed + self.skipped + self.failures.len()
    }

    fn record(&mut self, result: RoundtripResult) {
        match result {
            RoundtripResult::Passed => self.passed += 1,
            RoundtripResult::ReadSkipped(error) => {
                self.skipped += 1;
                eprintln!("  reader skip: {error}");
            }
            RoundtripResult::Failed(error) => self.failures.push(error),
        }
    }

    fn assert_success(&self, source: &str) {
        eprintln!(
            "{source}: {} passed, {} skipped, {} failed",
            self.passed,
            self.skipped,
            self.failures.len()
        );
        assert!(self.passed > 0, "No {source} UGX files were tested");
        assert!(
            self.failures.is_empty(),
            "{source} glTF roundtrip failures:\n{}",
            self.failures.join("\n")
        );
    }
}

#[test]
fn hw1_era_files_roundtrip() {
    let Some(game_dir) = load_game_dir("HW1_GAME_DIR") else {
        return;
    };
    let archives = find_files_flat(&game_dir, "era");
    assert!(
        !archives.is_empty(),
        "No ERA archives found in {}",
        game_dir.display()
    );

    let mut stats = RunStats::default();
    for archive_path in &archives {
        if stats.attempted() >= MAX_FILES {
            break;
        }
        process_archive(archive_path, &mut stats);
    }
    stats.assert_success("HW1");
}

fn process_archive(path: &Path, stats: &mut RunStats) {
    let Ok(mut archive) = open_era(path) else {
        eprintln!("Skipping unreadable archive {}", path.display());
        return;
    };
    for (entry_index, filename) in find_entries_in_era(&archive, ".ugx") {
        if stats.attempted() >= MAX_FILES {
            break;
        }
        let result = match archive.read_entry(entry_index) {
            Ok(data) => roundtrip_bytes(&filename, &data, UgxVersion::Hw1),
            Err(error) => RoundtripResult::ReadSkipped(format!(
                "{} entry {filename}: decompress: {error}",
                path.display()
            )),
        };
        stats.record(result);
    }
}

#[test]
fn hw2_loose_files_roundtrip() {
    let Some(game_dir) = load_game_dir("HW2_GAME_DIR") else {
        return;
    };
    let files = find_files_by_ext(&game_dir, "ugx");
    assert!(
        !files.is_empty(),
        "No UGX files found under {}",
        game_dir.display()
    );

    let mut stats = RunStats::default();
    for path in evenly_spaced(&files, MAX_FILES) {
        stats.record(roundtrip_loose_file(path));
    }
    stats.assert_success("HW2");
}

fn evenly_spaced<T>(items: &[T], limit: usize) -> Vec<&T> {
    if items.len() <= limit {
        return items.iter().collect();
    }
    if limit <= 1 {
        return items.first().into_iter().collect();
    }
    (0..limit)
        .map(|index| {
            let item_index = index * (items.len() - 1) / (limit - 1);
            &items[item_index]
        })
        .collect()
}

fn roundtrip_loose_file(path: &Path) -> RoundtripResult {
    let label = path.display().to_string();
    match std::fs::read(path) {
        Ok(data) => roundtrip_bytes(&label, &data, UgxVersion::Hw2),
        Err(error) => RoundtripResult::ReadSkipped(format!("{label}: read: {error}")),
    }
}
