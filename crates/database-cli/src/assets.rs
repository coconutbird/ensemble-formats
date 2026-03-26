//! Unified ERA asset source — mirrors the game's `BArchiveManager` lookup.
//!
//! ERA archives are loaded in priority order.  When a file is requested the
//! source searches **all** archives and returns the entry from the archive
//! with the **highest load order ID** — i.e. the last-loaded archive wins.
//! This matches the behaviour confirmed in IDA's `BFileManager::resolveFile`
//! loop at 0x140807090, where each file cache entry stores a load order ID
//! at offset 8 and the `jbe` comparison picks the largest.
//!
//! # Confirmed ERA load order (from IDA `BArchiveManager`)
//!
//! The engine loads archives across several init phases.  Archives loaded
//! later have **higher priority** (last loaded wins).
//!
//! ## Phase 1 — Early init (`sub_140820B60`)
//!  1. `locale.era`            — localised strings (lowest priority)
//!  2. `locale_update.era`     — locale-specific patches
//!  3. `root.era`              — base game data
//!  4. `root_update.era`       — base patches
//!  5. `shader.era`            — compiled shaders
//!
//! ## Phase 2 — Game init (`BArchiveManager::beginGameInit`)
//!  6. `miniloader.era`        — mini loading screen assets
//!  7. `pregameUI.era`         — pre-game menu UI
//!
//! ## Phase 3 — Scenario load (`BArchiveManager::beginScenarioPrefetch`)
//!  8. `ingameUI.era`          — in-game UI
//!  9. `scenarioshared.era`    — shared scenario models/anims
//! 10. `{scenario}.era`        — per-scenario assets
//!
//! ## Phase 4 — DLC (`BArchiveManager::loadDLCArchives`)
//! 11. `dlc01.era`             — DLC pack 1
//! 12. `dlc02.era`             — DLC pack 2 (highest priority)
//!
//! All paths are normalised to **lowercase with backslash** separators before
//! lookup, matching the engine's `tolower` pass in `resolveFile`.

use std::collections::HashMap;

type EraReader = era::Reader<era::crypto::decrypt::Reader<std::io::BufReader<std::fs::File>>>;

/// A loaded ERA archive with a pre-built filename → entry index map.
struct LoadedArchive {
    reader: EraReader,
    /// Archive label for diagnostics (e.g. "root.era").
    label: String,
    /// Normalised filename → entry index.
    index: HashMap<String, usize>,
}

/// Unified asset source backed by one or more ERA archives.
///
/// Files are resolved across all archives using the same algorithm as the
/// game engine: the archive with the **highest load order** (last loaded) wins.
pub struct AssetSource {
    archives: Vec<LoadedArchive>,
}

impl Default for AssetSource {
    fn default() -> Self {
        Self::new()
    }
}

impl AssetSource {
    /// Create an empty asset source (no archives loaded yet).
    pub fn new() -> Self {
        Self {
            archives: Vec::new(),
        }
    }

    /// Open an encrypted ERA archive and add it to the source.
    ///
    /// Archives added later have **higher priority** — file resolution follows
    /// the engine rule: highest load order ID (last loaded) wins.
    pub fn add_era(&mut self, path: &str) -> Result<usize, String> {
        let file = std::fs::File::open(path).map_err(|e| format!("Failed to open {path}: {e}"))?;
        let buf = std::io::BufReader::new(file);
        let reader = era::Reader::from_encrypted(buf, era::TeaKeys::default_archive_keys())
            .map_err(|e| format!("Failed to parse ERA archive {path}: {e}"))?;

        let mut index = HashMap::with_capacity(reader.entries().len());
        for (i, entry) in reader.entries().iter().enumerate() {
            if let Some(name) = &entry.filename {
                let key = normalise_path(name);
                index.insert(key, i);
            }
        }

        let entry_count = reader.entries().len();
        let label = std::path::Path::new(path)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string());

        self.archives.push(LoadedArchive {
            reader,
            label,
            index,
        });

        Ok(entry_count)
    }

    /// Read a file by virtual path (e.g. `"data\\objects.xml.xmb"`).
    ///
    /// The path is normalised before lookup.  Returns the decompressed bytes
    /// from the archive with the **highest load order** (last loaded wins).
    pub fn read(&mut self, path: &str) -> Option<Vec<u8>> {
        let key = normalise_path(path);

        // Last loaded archive wins — iterate in reverse to find the highest
        // load-order archive that contains this file.
        for archive in self.archives.iter_mut().rev() {
            if let Some(&entry_idx) = archive.index.get(&key) {
                return archive.reader.read_entry(entry_idx).ok();
            }
        }

        None
    }

    /// Check whether a file exists in any loaded archive.
    pub fn exists(&self, path: &str) -> bool {
        let key = normalise_path(path);
        self.archives.iter().any(|a| a.index.contains_key(&key))
    }

    /// Read and parse an XMB document by virtual path.
    pub fn read_xmb(&mut self, path: &str) -> Option<xmb::Document> {
        let data = self.read(path)?;
        xmb::Reader::read(&data).ok()
    }

    /// Return a summary of loaded archives (for diagnostics).
    pub fn summary(&self) -> Vec<(&str, usize)> {
        self.archives
            .iter()
            .map(|a| (a.label.as_str(), a.index.len()))
            .collect()
    }

    /// Return all filenames per archive (label → sorted file list).
    pub fn files_per_archive(&self) -> Vec<(&str, Vec<&str>)> {
        self.archives
            .iter()
            .map(|a| {
                let mut files: Vec<&str> = a.index.keys().map(|k| k.as_str()).collect();
                files.sort();
                (a.label.as_str(), files)
            })
            .collect()
    }
}

/// Normalise a game path to lowercase with backslash separators.
fn normalise_path(path: &str) -> String {
    path.to_lowercase().replace('/', "\\")
}
