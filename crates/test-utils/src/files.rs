//! File and archive discovery helpers.

use std::io::BufReader;
use std::path::{Path, PathBuf};

// ---- Generic file discovery ------------------------------------------------

/// Recursively find all files with a given extension under a directory.
#[must_use]
pub fn find_files_by_ext(dir: &Path, ext: &str) -> Vec<PathBuf> {
    fn walk(d: &Path, ext: &str, out: &mut Vec<PathBuf>) {
        if let Ok(entries) = std::fs::read_dir(d) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    walk(&p, ext, out);
                } else if p.extension().is_some_and(|e| e == ext) {
                    out.push(p);
                }
            }
        }
    }

    let mut out = Vec::new();
    walk(dir, ext, &mut out);
    out.sort();
    out
}

/// Find all files with a given extension (non-recursive, single directory).
#[must_use]
pub fn find_files_flat(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == ext))
        .collect();
    out.sort();
    out
}

// ---- ERA archive helpers ---------------------------------------------------

/// The concrete ERA reader type returned by [`open_era`].
pub type EraReader = era::Reader<era::crypto::decrypt::Reader<BufReader<std::fs::File>>>;

/// Open an ERA archive with default encryption keys.
///
/// # Errors
///
/// Returns an error if `path` cannot be opened or the archive header cannot
/// be read through the encrypted ERA reader.
pub fn open_era(path: &Path) -> Result<EraReader, Box<dyn std::error::Error>> {
    let file = std::fs::File::open(path)?;
    let archive =
        era::Reader::from_encrypted(BufReader::new(file), era::TeaKeys::default_archive_keys())?;
    Ok(archive)
}

/// Find all entries in an ERA archive whose filenames end with `ext`
/// (case-insensitive). Returns `(entry_index, filename)` pairs.
#[must_use]
pub fn find_entries_in_era(archive: &EraReader, ext: &str) -> Vec<(usize, String)> {
    let ext_lower = ext.to_lowercase();
    archive
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            e.filename
                .as_ref()
                .filter(|f| f.to_lowercase().ends_with(&ext_lower))
                .map(|f| (i, f.clone()))
        })
        .collect()
}
