//! ERA archive reader — generic over any [`Read`] + [`Seek`] source.
//!
//! [`Reader`] parses headers and the filename table on construction, then
//! reads individual entry data on demand via seek + read.
//!
//! # Streaming from a file (with decryption)
//!
//! ```ignore
//! use era::{Reader, TeaKeys, crypto};
//!
//! let file = std::fs::File::open("root.era")?;
//! let decrypt = crypto::decrypt::Reader::new(file, TeaKeys::default_archive_keys());
//! let mut reader = Reader::new(decrypt)?;
//!
//! if let Some(idx) = reader.find_by_name("scenario\\design\\mymap.scn") {
//!     let data = reader.read_entry(idx)?;
//! }
//! ```
//!
//! # From an in-memory byte slice
//!
//! ```ignore
//! let mut reader = era::Reader::from_bytes(&decrypted_bytes)?;
//! for entry in reader.iter() {
//!     println!("{}", entry.filename.as_deref().unwrap_or("<unnamed>"));
//! }
//! ```

use alloc::vec;
use alloc::vec::Vec;

use ecf::io::{Read, Seek, SeekFrom, SliceCursor};
use ecf::{EcfChunkHeader, EcfHeader};

use crate::error::{Error, Result};
use crate::header::{EraArchiveHeader, EraChunkExtra, EraEntry, resolve_filename};

/// Compressed entry data: (compressed_bytes, decompressed_size, tiger128_hash).
pub type CompressedEntryData = (Vec<u8>, u32, [u8; 16]);

/// An ERA archive reader backed by any [`Read`] + [`Seek`] source.
///
/// Headers and the filename table are parsed on construction.
/// Entry data is read on demand.
pub struct Reader<R> {
    inner: R,
    /// ECF header.
    pub ecf_header: EcfHeader,
    /// Archive header extension.
    pub archive_header: EraArchiveHeader,
    /// File entries (with filenames resolved).
    pub entries: Vec<EraEntry>,
    /// Raw chunk header bytes (big-endian, as on disk) for signature hashing.
    chunk_headers_raw: Vec<u8>,
    /// Signature block bytes (empty if unsigned).
    signature: Vec<u8>,
}

impl<R: Read + Seek> Reader<R> {
    /// Parse an ERA archive from any [`Read`] + [`Seek`] source.
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

        // Read signature block from the header region if present.
        // The signature sits at offset 48 (immediately after the 32-byte ECF
        // header and 16-byte ERA archive header extension) and spans
        // `signature_size` bytes.
        let signature = if archive_header.signature_size > 0 {
            let sig_size = archive_header.signature_size as usize;
            let sig_offset = EcfHeader::SIZE as u64 + EraArchiveHeader::SIZE as u64; // 32 + 16 = 48
            inner
                .seek(SeekFrom::Start(sig_offset))
                .map_err(|_| Error::UnexpectedEof)?;
            let mut sig_buf = vec![0u8; sig_size];
            inner
                .read_exact(&mut sig_buf)
                .map_err(|_| Error::UnexpectedEof)?;
            sig_buf
        } else {
            Vec::new()
        };

        Ok(Self {
            inner,
            ecf_header,
            archive_header,
            entries,
            chunk_headers_raw: chunk_buf,
            signature,
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

    /// Read compressed data for multiple entries sequentially.
    pub fn read_entries_compressed(
        &mut self,
        indices: &[usize],
    ) -> Result<Vec<CompressedEntryData>> {
        let mut results = Vec::with_capacity(indices.len());
        for &idx in indices {
            results.push(self.read_entry_compressed(idx)?);
        }
        Ok(results)
    }

    /// Whether this archive has a digital signature.
    pub fn has_signature(&self) -> bool {
        !self.signature.is_empty()
    }

    /// Get the raw signature bytes (empty if unsigned).
    pub fn signature(&self) -> &[u8] {
        &self.signature
    }

    /// Compute the header hash used for signature verification.
    ///
    /// This hashes the ECF header fields and all chunk headers exactly as
    /// the game does in `ERA_LoadArchiveHeaders`.
    pub fn header_hash(&self) -> [u8; 20] {
        crate::crypto::merkle::compute_header_hash(
            self.ecf_header.header_size,
            self.ecf_header.num_chunks,
            self.ecf_header.chunk_extra_data_size,
            self.ecf_header.file_size,
            &self.chunk_headers_raw,
        )
    }

    /// Verify the archive's Merkle signature against a public key.
    ///
    /// Returns `Ok(true)` if valid, `Ok(false)` if verification fails,
    /// or `Err` if the signature is malformed or missing.
    pub fn verify_signature(&self, public_key: &[u8; 20]) -> Result<bool> {
        if self.signature.is_empty() {
            return Ok(false);
        }
        let hash = self.header_hash();
        crate::crypto::merkle::verify(public_key, &hash, &self.signature)
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
    /// Parse an ERA archive from a decrypted byte slice.
    ///
    /// This wraps the slice in a [`SliceCursor`] so no copy is made.
    pub fn from_bytes(data: &'a [u8]) -> Result<Self> {
        Self::new(SliceCursor::new(data))
    }
}

impl<R: Read + Seek> Reader<crate::crypto::decrypt::Reader<R>> {
    /// Parse an encrypted ERA archive, decrypting on the fly.
    ///
    /// Wraps the source in a [`crypto::decrypt::Reader`] so data is decrypted
    /// block-by-block as it is read — the full archive is never materialised
    /// in memory.
    pub fn from_encrypted(inner: R, keys: crate::TeaKeys) -> Result<Self> {
        Self::new(crate::crypto::decrypt::Reader::new(inner, keys))
    }
}
