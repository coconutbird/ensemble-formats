//! PKG archive reader — generic over any [`Read`] + [`Seek`] source.
//!
//! [`Reader`] parses headers and the entry table on construction, then
//! reads individual entry data on demand via seek + read.
//!
//! # Streaming from a file
//!
//! ```ignore
//! let file = std::io::BufReader::new(std::fs::File::open("fonts.pkg")?);
//! let mut reader = pkg::Reader::new(file)?;
//!
//! if let Some(idx) = reader.find("data\\fonts\\arial.fnt") {
//!     let data = reader.read_entry(idx)?;
//! }
//! ```
//!
//! # From an in-memory byte slice
//!
//! ```ignore
//! let mut reader = pkg::Reader::from_bytes(&bytes)?;
//! for entry in reader.iter() {
//!     println!("{} ({} bytes)", entry.filename, entry.data_size);
//! }
//! ```

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use nostdio::{NoProgress, Progress, Read, Seek, SeekFrom, SliceCursor};

use crate::header::{MAGIC, MAX_FILENAME_LEN, MAX_VERSION};
use crate::{Error, Result};

/// A parsed file entry from a PKG archive.
#[derive(Debug, Clone)]
pub struct PkgEntry {
    /// Filename (lowercased, backslash-separated, no leading backslash).
    pub filename: String,
    /// Offset of this file's data within the PKG.
    pub data_offset: u64,
    /// Size of this file's data in bytes.
    pub data_size: u64,
    /// FNV-1a 64-bit hash of the filename.
    pub name_hash: u64,
}

/// A PKG archive reader backed by any [`Read`] + [`Seek`] source.
///
/// Headers and the entry table are parsed on construction.
/// Entry data is read on demand.
pub struct Reader<R> {
    inner: R,
    version: u64,
    alignment: u64,
    entries: Vec<PkgEntry>,
    /// Byte offset where the data section begins.
    data_section_offset: u64,
}

/// Compute FNV-1a 64-bit hash (matches engine behaviour).
pub fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xCBF2_9CE4_8422_2325;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x0100_0000_01B3);
    }
    hash
}

/// Read a little-endian u64 from a [`Read`] source.
fn read_u64_le(r: &mut impl Read) -> Result<u64> {
    let mut buf = [0u8; 8];
    r.read_exact(&mut buf)?;
    Ok(u64::from_le_bytes(buf))
}

impl<R: Read + Seek> Reader<R> {
    /// Parse a PKG archive from any [`Read`] + [`Seek`] source.
    ///
    /// Reads all headers and the entry table up-front.
    /// Entry data is **not** read until [`read_entry`](Self::read_entry)
    /// is called.
    pub fn new(mut inner: R) -> Result<Self> {
        // Read and validate magic.
        let mut magic = [0u8; 6];
        inner.read_exact(&mut magic)?;
        if &magic != MAGIC {
            return Err(Error::InvalidMagic { found: magic });
        }

        let version = read_u64_le(&mut inner)?;
        if version == 0 || version > MAX_VERSION {
            return Err(Error::UnsupportedVersion(version));
        }

        let entry_count = read_u64_le(&mut inner)?;

        let mut entries = Vec::with_capacity(entry_count as usize);
        for _ in 0..entry_count {
            let filename_len = read_u64_le(&mut inner)?;
            if filename_len > MAX_FILENAME_LEN {
                return Err(Error::FilenameTooLong(filename_len));
            }

            // Read filename bytes.
            let mut raw_name = vec![0u8; filename_len as usize];
            inner.read_exact(&mut raw_name)?;

            // Normalise: lowercase, / → \, strip leading \.
            let mut name = String::with_capacity(raw_name.len());
            for &b in &raw_name {
                let ch = if b == b'/' {
                    b'\\'
                } else {
                    b.to_ascii_lowercase()
                };
                name.push(ch as char);
            }
            if name.starts_with('\\') {
                name.remove(0);
            }

            let name_hash = fnv1a_64(name.as_bytes());
            let data_offset = read_u64_le(&mut inner)?;
            let data_size = read_u64_le(&mut inner)?;

            entries.push(PkgEntry {
                filename: name,
                data_offset,
                data_size,
                name_hash,
            });
        }

        // Version >= 2 has an alignment field.
        let alignment = if version >= 2 {
            read_u64_le(&mut inner)?
        } else {
            0
        };

        // Current position = end of header/entry table.
        let header_end = inner.stream_position()?;

        // Compute data section start with alignment.
        let mut data_section_offset = header_end;
        if alignment != 0 && data_section_offset % alignment != 0 {
            data_section_offset =
                alignment + data_section_offset - (data_section_offset % alignment);
        }

        Ok(Self {
            inner,
            version,
            alignment,
            entries,
            data_section_offset,
        })
    }

    /// PKG format version (1 or 2).
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Data alignment (version 2+; 0 for version 1).
    pub fn alignment(&self) -> u64 {
        self.alignment
    }

    /// Byte offset where the data section begins.
    pub fn data_section_offset(&self) -> u64 {
        self.data_section_offset
    }

    /// All parsed file entries.
    pub fn entries(&self) -> &[PkgEntry] {
        &self.entries
    }

    /// Number of file entries.
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Iterate over all entries.
    pub fn iter(&self) -> impl Iterator<Item = &PkgEntry> {
        self.entries.iter()
    }

    /// Read the raw data for entry at `index`.
    pub fn read_entry(&mut self, index: usize) -> Result<Vec<u8>> {
        let entry = self
            .entries
            .get(index)
            .ok_or(Error::EntryIndexOutOfBounds {
                index,
                count: self.entries.len(),
            })?;
        let offset = entry.data_offset;
        let size = entry.data_size as usize;

        self.inner.seek(SeekFrom::Start(offset))?;
        let mut buf = vec![0u8; size];
        self.inner.read_exact(&mut buf)?;
        Ok(buf)
    }

    /// Read the raw data for an entry by reference.
    pub fn read_entry_data(&mut self, entry: &PkgEntry) -> Result<Vec<u8>> {
        self.inner.seek(SeekFrom::Start(entry.data_offset))?;
        let mut buf = vec![0u8; entry.data_size as usize];
        self.inner.read_exact(&mut buf)?;
        Ok(buf)
    }

    /// Read all entries sequentially, invoking `handler` for each.
    ///
    /// Equivalent to `read_all_with_progress` with [`NoProgress`].
    pub fn read_all(&mut self, handler: impl FnMut(usize, &PkgEntry, Vec<u8>)) -> Result<()> {
        self.read_all_with_progress(handler, &mut NoProgress)
    }

    /// Read all entries sequentially, invoking `handler` for each, with
    /// progress reporting.
    ///
    /// The [`Progress`] implementation receives `(bytes_read, total_bytes)`
    /// and should return `true` to continue or `false` to cancel.
    /// The `handler` receives `(index, &PkgEntry, data)` for each entry.
    pub fn read_all_with_progress(
        &mut self,
        mut handler: impl FnMut(usize, &PkgEntry, Vec<u8>),
        progress: &mut impl Progress,
    ) -> Result<()> {
        let total_bytes: u64 = self.entries.iter().map(|e| e.data_size).sum();
        let mut bytes_read: u64 = 0;
        for i in 0..self.entries.len() {
            let data = self.read_entry(i)?;
            bytes_read += data.len() as u64;
            handler(i, &self.entries[i], data);
            if !progress.report(bytes_read, total_bytes) {
                return Err(Error::Cancelled);
            }
        }
        Ok(())
    }

    /// Find an entry index by filename (case-insensitive, FNV-1a hash).
    pub fn find(&self, filename: &str) -> Option<usize> {
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
        let hash = fnv1a_64(normalised.as_bytes());
        self.entries.iter().position(|e| e.name_hash == hash)
    }

    /// Consume the reader and return the underlying source.
    pub fn into_inner(self) -> R {
        self.inner
    }

    /// Get a mutable reference to the underlying source.
    pub fn inner_mut(&mut self) -> &mut R {
        &mut self.inner
    }
}

impl<'a> Reader<SliceCursor<'a>> {
    /// Parse a PKG archive from a byte slice.
    ///
    /// Wraps the slice in a [`SliceCursor`] so no copy is made.
    pub fn from_bytes(data: &'a [u8]) -> Result<Self> {
        Self::new(SliceCursor::new(data))
    }
}
