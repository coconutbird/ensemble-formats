//! PKG writer — build `"capack"` archives from files.
//!
//! # Example
//!
//! ```ignore
//! let mut writer = pkg::Writer::new();
//! writer.add_file("data\\fonts\\arial.fnt", font_data);
//! let bytes = writer.finalize()?;
//! std::fs::write("fonts.pkg", &bytes)?;
//! ```

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use nostdio::{NoProgress, Progress, Write};

use crate::header::{MAGIC, MAX_FILENAME_LEN};
use crate::reader::fnv1a_64;
use crate::{Error, Result};

/// A pending file to be written into the archive.
struct PendingFile {
    /// Normalised filename (lowercase, backslash-separated).
    filename: String,
    /// Raw file data.
    data: Vec<u8>,
}

/// PKG archive writer.
///
/// Collects files and serialises them into the `"capack"` binary format.
/// Defaults to version 2 with 4096-byte alignment.
pub struct Writer {
    files: Vec<PendingFile>,
    version: u64,
    alignment: u64,
}

impl Default for Writer {
    fn default() -> Self {
        Self::new()
    }
}

impl Writer {
    /// Create a new PKG writer (version 2, 4096-byte alignment).
    #[must_use]
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            version: 2,
            alignment: 4096,
        }
    }

    /// Create a version-1 writer (no alignment footer).
    #[must_use]
    pub fn new_v1() -> Self {
        Self {
            files: Vec::new(),
            version: 1,
            alignment: 0,
        }
    }

    /// Set the format version (1 or 2).
    pub fn set_version(&mut self, version: u64) {
        self.version = version;
        if version < 2 {
            self.alignment = 0;
        }
    }

    /// Set the data alignment (version 2+ only, ignored for v1).
    pub fn set_alignment(&mut self, alignment: u64) {
        self.alignment = alignment;
    }

    /// Add a file to the archive.
    ///
    /// The filename is normalised: lowercased, forward slashes converted to
    /// backslashes, leading backslash stripped.
    pub fn add_file(&mut self, filename: impl Into<String>, data: Vec<u8>) {
        let raw = filename.into();
        let mut normalised = String::with_capacity(raw.len());
        for b in raw.bytes() {
            let ch = if b == b'/' {
                b'\\'
            } else {
                b.to_ascii_lowercase()
            };
            normalised.push(ch as char);
        }
        if normalised.starts_with('\\') {
            normalised.remove(0);
        }
        self.files.push(PendingFile {
            filename: normalised,
            data,
        });
    }

    /// Number of files added so far.
    #[must_use]
    pub fn len(&self) -> usize {
        self.files.len()
    }

    /// Whether the writer has no files.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Serialise the archive into a `Vec<u8>`.
    ///
    /// # Errors
    ///
    /// Returns an error if a filename or archive size exceeds the format's
    /// limits.
    pub fn finalize(&self) -> Result<Vec<u8>> {
        self.finalize_with_progress(&mut NoProgress)
    }

    /// Serialise the archive into a `Vec<u8>` with progress reporting.
    ///
    /// The [`Progress`] implementation receives `(bytes_written, total_bytes)`
    /// and should return `true` to continue or `false` to cancel.
    ///
    /// # Errors
    ///
    /// Returns an error if a filename or archive size exceeds the format's
    /// limits, or [`Error::Cancelled`] if progress reporting cancels the write.
    pub fn finalize_with_progress(&self, progress: &mut impl Progress) -> Result<Vec<u8>> {
        // Validate filenames.
        for f in &self.files {
            if f.filename.len() as u64 > MAX_FILENAME_LEN {
                return Err(Error::FilenameTooLong(f.filename.len() as u64));
            }
        }

        // --- Compute sizes ---
        // Header: magic(6) + version(8) + entry_count(8) = 22
        let mut header_size: usize = 6 + 8 + 8;

        // Entries: for each file: filename_len(8) + filename(N) + offset(8) + size(8)
        for f in &self.files {
            header_size += 8 + f.filename.len() + 8 + 8;
        }

        // Footer (v2+): alignment(8)
        if self.version >= 2 {
            header_size += 8;
        }

        // Data section offset (aligned).
        let mut data_offset = header_size as u64;
        if self.alignment > 0 && !data_offset.is_multiple_of(self.alignment) {
            data_offset = self.alignment + data_offset - (data_offset % self.alignment);
        }

        // Compute per-file offsets.
        let mut file_offsets: Vec<u64> = Vec::with_capacity(self.files.len());
        let mut cursor = data_offset;
        for f in &self.files {
            file_offsets.push(cursor);
            cursor += f.data.len() as u64;
        }

        let total_size =
            usize::try_from(cursor).map_err(|_| Error::SizeOverflow("archive size"))?;

        // --- Write output ---
        let mut out = vec![0u8; total_size];

        let mut pos: usize = 0;

        // Magic.
        out[pos..pos + 6].copy_from_slice(MAGIC);
        pos += 6;

        // Version.
        out[pos..pos + 8].copy_from_slice(&self.version.to_le_bytes());
        pos += 8;

        // Entry count.
        out[pos..pos + 8].copy_from_slice(&(self.files.len() as u64).to_le_bytes());
        pos += 8;

        // Entries.
        for (i, f) in self.files.iter().enumerate() {
            // filename_length
            out[pos..pos + 8].copy_from_slice(&(f.filename.len() as u64).to_le_bytes());
            pos += 8;

            // filename
            out[pos..pos + f.filename.len()].copy_from_slice(f.filename.as_bytes());
            pos += f.filename.len();

            // data_offset
            out[pos..pos + 8].copy_from_slice(&file_offsets[i].to_le_bytes());
            pos += 8;

            // data_size
            out[pos..pos + 8].copy_from_slice(&(f.data.len() as u64).to_le_bytes());
            pos += 8;
        }

        // Alignment footer (v2+).
        if self.version >= 2 {
            out[pos..pos + 8].copy_from_slice(&self.alignment.to_le_bytes());
            pos += 8;
        }

        let _ = pos; // header section done

        // File data.
        let total_data_bytes: u64 = self.files.iter().map(|f| f.data.len() as u64).sum();
        let mut bytes_written: u64 = 0;
        for (i, f) in self.files.iter().enumerate() {
            let start =
                usize::try_from(file_offsets[i]).map_err(|_| Error::SizeOverflow("file offset"))?;
            out[start..start + f.data.len()].copy_from_slice(&f.data);
            bytes_written += f.data.len() as u64;
            if !progress.report(bytes_written, total_data_bytes) {
                return Err(Error::Cancelled);
            }
        }

        Ok(out)
    }

    /// Write the archive to any [`Write`] sink (streaming).
    ///
    /// Unlike [`finalize`](Self::finalize), this does not allocate a single
    /// contiguous buffer — it writes header, entries, padding, and data
    /// sequentially.
    ///
    /// # Errors
    ///
    /// Returns an error if the archive exceeds format limits or the sink
    /// cannot accept all bytes.
    pub fn write_to<W: Write>(&self, w: &mut W) -> Result<()> {
        self.write_to_with_progress(w, &mut NoProgress)
    }

    /// Stream the archive to a writer with progress reporting.
    ///
    /// The [`Progress`] implementation receives `(bytes_written, total_bytes)`
    /// and should return `true` to continue or `false` to cancel.
    ///
    /// # Errors
    ///
    /// Returns an error if the archive exceeds format limits, the sink cannot
    /// accept all bytes, or progress reporting cancels the write.
    pub fn write_to_with_progress<W: Write>(
        &self,
        w: &mut W,
        progress: &mut impl Progress,
    ) -> Result<()> {
        // Validate filenames.
        for f in &self.files {
            if f.filename.len() as u64 > MAX_FILENAME_LEN {
                return Err(Error::FilenameTooLong(f.filename.len() as u64));
            }
        }

        // --- Compute sizes ---
        let mut header_size: usize = 6 + 8 + 8;
        for f in &self.files {
            header_size += 8 + f.filename.len() + 8 + 8;
        }
        if self.version >= 2 {
            header_size += 8;
        }

        let mut data_offset = header_size as u64;
        if self.alignment > 0 && !data_offset.is_multiple_of(self.alignment) {
            data_offset = self.alignment + data_offset - (data_offset % self.alignment);
        }

        // Compute per-file offsets.
        let mut file_offsets: Vec<u64> = Vec::with_capacity(self.files.len());
        let mut cursor = data_offset;
        for f in &self.files {
            file_offsets.push(cursor);
            cursor += f.data.len() as u64;
        }

        // --- Write header ---
        w.write_all(MAGIC)?;
        w.write_all(&self.version.to_le_bytes())?;
        w.write_all(&(self.files.len() as u64).to_le_bytes())?;

        // --- Write entries ---
        for (i, f) in self.files.iter().enumerate() {
            w.write_all(&(f.filename.len() as u64).to_le_bytes())?;
            w.write_all(f.filename.as_bytes())?;
            w.write_all(&file_offsets[i].to_le_bytes())?;
            w.write_all(&(f.data.len() as u64).to_le_bytes())?;
        }

        // --- Alignment footer (v2+) ---
        if self.version >= 2 {
            w.write_all(&self.alignment.to_le_bytes())?;
        }

        // --- Padding ---
        let data_offset =
            usize::try_from(data_offset).map_err(|_| Error::SizeOverflow("data offset"))?;
        let padding = data_offset - header_size;
        if padding > 0 {
            let zeros = vec![0u8; padding];
            w.write_all(&zeros)?;
        }

        // --- File data ---
        let total_data_bytes: u64 = self.files.iter().map(|f| f.data.len() as u64).sum();
        let mut bytes_written: u64 = 0;
        for f in &self.files {
            w.write_all(&f.data)?;
            bytes_written += f.data.len() as u64;
            if !progress.report(bytes_written, total_data_bytes) {
                return Err(Error::Cancelled);
            }
        }

        Ok(())
    }

    /// Convenience: compute the FNV-1a 64-bit hash for a normalised filename.
    #[must_use]
    pub fn hash_filename(filename: &str) -> u64 {
        let mut normalised = String::with_capacity(filename.len());
        for b in filename.bytes() {
            let ch = if b == b'/' {
                b'\\'
            } else {
                b.to_ascii_lowercase()
            };
            normalised.push(ch as char);
        }
        if normalised.starts_with('\\') {
            normalised.remove(0);
        }
        fnv1a_64(normalised.as_bytes())
    }
}
