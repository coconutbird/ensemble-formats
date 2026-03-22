//! Streaming ERA reader — reads from any [`Read`] + [`Seek`] source.
//!
//! Unlike [`Reader`](crate::Reader) which requires the full decrypted archive
//! in memory, `StreamingReader` reads headers and the filename table up-front,
//! then fetches individual entry data on demand via seek + read.
//!
//! This is ideal for large ERA archives where only a few files are needed, or
//! when combined with [`DecryptReader`](crate::DecryptReader) for streaming
//! decryption without materialising the entire archive.
//!
//! ```ignore
//! use era::{StreamingReader, DecryptReader, TeaKeys};
//!
//! let file = std::fs::File::open("root.era")?;
//! let decrypt = DecryptReader::new(file, TeaKeys::default_archive_keys());
//! let mut reader = StreamingReader::new(decrypt)?;
//!
//! for (i, entry) in reader.entries().iter().enumerate() {
//!     if let Some(name) = &entry.filename {
//!         println!("{}: {}", i, name);
//!     }
//! }
//!
//! // Read only the entry you need
//! if let Some(idx) = reader.find_by_name("scenario\\design\\mymap.scn") {
//!     let data = reader.read_entry(idx)?;
//! }
//! ```

use alloc::vec;
use alloc::vec::Vec;

use ecf::io::{Read, Seek, SeekFrom};
use ecf::{EcfChunkHeader, EcfHeader};

use crate::error::{Error, Result};
use crate::header::{EraArchiveHeader, EraChunkExtra, EraEntry, resolve_filename};
use crate::reader::CompressedEntryData;

/// A streaming ERA reader backed by any [`Read`] + [`Seek`] source.
///
/// Headers and the filename table are parsed on construction.
/// Entry data is read on demand.
pub struct StreamingReader<R> {
    inner: R,
    /// ECF header.
    pub ecf_header: EcfHeader,
    /// Archive header extension.
    pub archive_header: EraArchiveHeader,
    /// File entries (with filenames resolved).
    pub entries: Vec<EraEntry>,
}

impl<R: Read + Seek> StreamingReader<R> {
    /// Parse an ERA archive from a stream.
    ///
    /// Reads all headers and the filename table (chunk 0) up-front.
    /// Entry data is **not** read until [`read_entry`](Self::read_entry) is called.
    pub fn new(mut inner: R) -> Result<Self> {
        // Read ECF header (32 bytes)
        let mut hdr_buf = [0u8; EcfHeader::SIZE];
        inner
            .read_exact(&mut hdr_buf)
            .map_err(|_| Error::UnexpectedEof)?;
        let ecf_header = EcfHeader::from_bytes(&hdr_buf)?;

        // Read ERA archive header extension (immediately after ECF header)
        let mut archive_buf = [0u8; EraArchiveHeader::SIZE];
        inner
            .read_exact(&mut archive_buf)
            .map_err(|_| Error::UnexpectedEof)?;
        let archive_header = EraArchiveHeader::from_bytes(&archive_buf)?;

        // Seek to chunk headers start
        let chunk_start = ecf_header.header_size as u64;
        inner
            .seek(SeekFrom::Start(chunk_start))
            .map_err(|_| Error::UnexpectedEof)?;

        // Read all chunk headers (base + extra)
        let chunk_stride = EcfChunkHeader::SIZE + ecf_header.chunk_extra_data_size as usize;
        let total = chunk_stride * ecf_header.num_chunks as usize;
        let mut chunk_buf = vec![0u8; total];
        inner
            .read_exact(&mut chunk_buf)
            .map_err(|_| Error::UnexpectedEof)?;

        // Parse entries
        let mut entries = Vec::with_capacity(ecf_header.num_chunks as usize);
        let mut pos = 0usize;
        for _ in 0..ecf_header.num_chunks {
            let chunk = EcfChunkHeader::from_bytes(&chunk_buf[pos..])?;
            pos += EcfChunkHeader::SIZE;

            let extra = if ecf_header.chunk_extra_data_size >= EraChunkExtra::SIZE as u16 {
                let extra = EraChunkExtra::from_bytes(&chunk_buf[pos..])?;
                pos += ecf_header.chunk_extra_data_size as usize;
                extra
            } else {
                pos += ecf_header.chunk_extra_data_size as usize;
                EraChunkExtra {
                    date: 0,
                    decomp_size: chunk.size,
                    comp_tiger128: [0; 16],
                    name_offset: 0,
                }
            };

            entries.push(EraEntry {
                chunk,
                extra,
                filename: None,
            });
        }

        // Read & decompress filename table (always chunk 0)
        let filename_table = if !entries.is_empty() {
            let e = &entries[0];
            let start = e.chunk.offset as u64;
            let size = e.chunk.size as usize;
            inner
                .seek(SeekFrom::Start(start))
                .map_err(|_| Error::UnexpectedEof)?;
            let mut raw = vec![0u8; size];
            inner
                .read_exact(&mut raw)
                .map_err(|_| Error::UnexpectedEof)?;
            entries[0].decompress(&raw)?
        } else {
            Vec::new()
        };

        // Resolve filenames
        for entry in entries.iter_mut().skip(1) {
            entry.filename = resolve_filename(&filename_table, entry.extra.name_offset);
        }

        Ok(Self {
            inner,
            ecf_header,
            archive_header,
            entries,
        })
    }

    /// Number of entries in the archive.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the archive is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get an entry by index.
    pub fn entry(&self, index: usize) -> Option<&EraEntry> {
        self.entries.get(index)
    }

    /// The parsed entries.
    pub fn entries(&self) -> &[EraEntry] {
        &self.entries
    }

    /// Iterate over all entries.
    pub fn iter(&self) -> impl Iterator<Item = &EraEntry> {
        self.entries.iter()
    }

    /// Find an entry by filename.
    pub fn find_by_name(&self, name: &str) -> Option<usize> {
        let name_lower = name.to_lowercase().replace('/', "\\");
        self.entries.iter().position(|e| {
            e.filename
                .as_ref()
                .is_some_and(|f| f.to_lowercase() == name_lower)
        })
    }

    /// Read and decompress the data for an entry.
    pub fn read_entry(&mut self, index: usize) -> Result<Vec<u8>> {
        let entry = self
            .entries
            .get(index)
            .ok_or(Error::ChunkIndexOutOfBounds {
                index,
                count: self.entries.len(),
            })?;

        let start = entry.chunk.offset as u64;
        let size = entry.chunk.size as usize;

        self.inner
            .seek(SeekFrom::Start(start))
            .map_err(|_| Error::UnexpectedEof)?;

        let mut raw = vec![0u8; size];
        self.inner
            .read_exact(&mut raw)
            .map_err(|_| Error::UnexpectedEof)?;

        self.entries[index].decompress(&raw)
    }

    /// Read compressed data for an entry WITHOUT decompressing.
    ///
    /// Returns: (compressed_data, decompressed_size, tiger128_hash).
    pub fn read_entry_compressed(&mut self, index: usize) -> Result<CompressedEntryData> {
        let entry = self
            .entries
            .get(index)
            .ok_or(Error::ChunkIndexOutOfBounds {
                index,
                count: self.entries.len(),
            })?;

        let start = entry.chunk.offset as u64;
        let size = entry.chunk.size as usize;
        let decomp_size = entry.extra.decomp_size;
        let tiger128 = entry.extra.comp_tiger128;

        self.inner
            .seek(SeekFrom::Start(start))
            .map_err(|_| Error::UnexpectedEof)?;

        let mut raw = vec![0u8; size];
        self.inner
            .read_exact(&mut raw)
            .map_err(|_| Error::UnexpectedEof)?;

        Ok((raw, decomp_size, tiger128))
    }

    /// Read multiple entries sequentially.
    pub fn read_entries(&mut self, indices: &[usize]) -> Result<Vec<Vec<u8>>> {
        let mut results = Vec::with_capacity(indices.len());
        for &idx in indices {
            results.push(self.read_entry(idx)?);
        }
        Ok(results)
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
