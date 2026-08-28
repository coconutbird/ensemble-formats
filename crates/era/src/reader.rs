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

use ecf::io::{Cursor, NoProgress, Progress, Read, Seek, SeekFrom};
use ecf::{EcfChunkHeader, EcfHeader};

use crate::error::{Error, Result};
use crate::header::{EraArchiveHeader, EraChunkExtra, EraEntry, resolve_filename};

/// Compressed entry data: (`compressed_bytes`, `decompressed_size`, `tiger128_hash`).
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
    /// Optional public key for signature verification.
    public_key: Option<[u8; 20]>,
}

/// An encrypted ERA reader backed by contiguous in-memory bytes.
pub type EncryptedBytesReader<D> = Reader<crate::crypto::decrypt::Reader<Cursor<D>>>;

impl<R: Read + Seek> Reader<R> {
    /// Parse an ERA archive from any [`Read`] + [`Seek`] source.
    ///
    /// Reads all headers and the filename table (chunk 0) up-front.
    /// Entry data is **not** read until [`read_entry`](Self::read_entry) is called.
    ///
    /// # Errors
    ///
    /// Returns an error if the archive headers or filename table are invalid,
    /// truncated, or cannot be decompressed.
    pub fn new(inner: R) -> Result<Self> {
        Self::parse(inner, None)
    }

    /// Parse an ERA archive and set a public key for signature verification.
    ///
    /// Same as [`new`](Self::new), but stores the key so that
    /// [`verify_signature`](Self::verify_signature) can be called without
    /// an explicit key argument.
    ///
    /// # Errors
    ///
    /// Returns an error if the archive headers or filename table are invalid,
    /// truncated, or cannot be decompressed.
    pub fn with_public_key(inner: R, public_key: [u8; 20]) -> Result<Self> {
        Self::parse(inner, Some(public_key))
    }

    fn parse(mut inner: R, public_key: Option<[u8; 20]>) -> Result<Self> {
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
        let chunk_start = u64::from(ecf_header.header_size);
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

            let extra = if usize::from(ecf_header.chunk_extra_data_size) >= EraChunkExtra::SIZE {
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
        let filename_table = if entries.is_empty() {
            Vec::new()
        } else {
            let e = &entries[0];
            let start = u64::from(e.chunk.offset);
            let size = e.chunk.size as usize;
            inner
                .seek(SeekFrom::Start(start))
                .map_err(|_| Error::UnexpectedEof)?;
            let mut raw = vec![0u8; size];
            inner
                .read_exact(&mut raw)
                .map_err(|_| Error::UnexpectedEof)?;
            entries[0].decompress(&raw)?
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
            public_key,
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
    ///
    /// # Errors
    ///
    /// Returns an error if `index` is invalid or the entry cannot be read or
    /// decompressed.
    pub fn read_entry(&mut self, index: usize) -> Result<Vec<u8>> {
        let entry = self
            .entries
            .get(index)
            .ok_or(Error::ChunkIndexOutOfBounds {
                index,
                count: self.entries.len(),
            })?;

        let start = u64::from(entry.chunk.offset);
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
    /// Returns: (`compressed_data`, `decompressed_size`, `tiger128_hash`).
    ///
    /// # Errors
    ///
    /// Returns an error if `index` is invalid or the compressed entry cannot be
    /// read.
    pub fn read_entry_compressed(&mut self, index: usize) -> Result<CompressedEntryData> {
        let entry = self
            .entries
            .get(index)
            .ok_or(Error::ChunkIndexOutOfBounds {
                index,
                count: self.entries.len(),
            })?;

        let start = u64::from(entry.chunk.offset);
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
    ///
    /// # Errors
    ///
    /// Returns an error if any requested index is invalid or cannot be read or
    /// decompressed.
    pub fn read_entries(&mut self, indices: &[usize]) -> Result<Vec<Vec<u8>>> {
        let mut results = Vec::with_capacity(indices.len());
        for &idx in indices {
            results.push(self.read_entry(idx)?);
        }
        Ok(results)
    }

    /// Read compressed data for multiple entries sequentially.
    ///
    /// # Errors
    ///
    /// Returns an error if any requested index is invalid or cannot be read.
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

    /// Read all file entries sequentially, invoking `handler` for each.
    ///
    /// Skips the filename table (entry 0) and iterates entries 1..N.
    /// Equivalent to `read_all_with_progress` with [`NoProgress`].
    ///
    /// # Errors
    ///
    /// Returns an error if an entry cannot be read or decompressed.
    pub fn read_all(&mut self, handler: impl FnMut(usize, &EraEntry, Vec<u8>)) -> Result<()> {
        self.read_all_with_progress(handler, &mut NoProgress)
    }

    /// Read all file entries sequentially with progress reporting.
    ///
    /// Skips the filename table (entry 0) and iterates entries 1..N.
    /// The `handler` receives `(index, &EraEntry, decompressed_data)` for
    /// each file entry.
    ///
    /// The [`Progress`] implementation receives `(bytes_read, total_bytes)`
    /// and should return `true` to continue or `false` to cancel.
    ///
    /// # Errors
    ///
    /// Returns an error if an entry cannot be read or decompressed, or if the
    /// progress callback cancels the operation.
    pub fn read_all_with_progress(
        &mut self,
        mut handler: impl FnMut(usize, &EraEntry, Vec<u8>),
        progress: &mut impl Progress,
    ) -> Result<()> {
        let total_bytes: u64 = self
            .entries
            .iter()
            .skip(1)
            .map(|e| u64::from(e.extra.decomp_size))
            .sum();
        let mut bytes_read: u64 = 0;

        for i in 1..self.entries.len() {
            let data = self.read_entry(i)?;
            bytes_read += data.len() as u64;
            handler(i, &self.entries[i], data);
            if !progress.report(bytes_read, total_bytes) {
                return Err(Error::Cancelled);
            }
        }
        Ok(())
    }

    /// Set a public key for signature verification.
    ///
    /// When set, [`verify_signature`](Self::verify_signature) can be called
    /// without an explicit key argument.
    pub fn set_public_key(&mut self, key: [u8; 20]) {
        self.public_key = Some(key);
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

    /// Verify the archive's Merkle signature against the stored public key.
    ///
    /// Returns `Ok(true)` if valid, `Ok(false)` if unsigned or no key set,
    /// or `Err` if the signature is malformed.
    ///
    /// # Errors
    ///
    /// Returns an error if the signature is malformed or truncated.
    pub fn verify_signature(&self) -> Result<bool> {
        let Some(key) = &self.public_key else {
            return Ok(false);
        };
        if self.signature.is_empty() {
            return Ok(false);
        }
        let hash = self.header_hash();
        crate::crypto::merkle::verify(key, &hash, &self.signature)
    }

    /// Verify the archive's Merkle signature against an explicit public key.
    ///
    /// Returns `Ok(true)` if valid, `Ok(false)` if verification fails,
    /// or `Err` if the signature is malformed or missing.
    ///
    /// # Errors
    ///
    /// Returns an error if the signature is malformed or truncated.
    pub fn verify_signature_with_key(&self, public_key: &[u8; 20]) -> Result<bool> {
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

impl<'a> Reader<Cursor<&'a [u8]>> {
    /// Parse an ERA archive from a decrypted byte slice.
    ///
    /// This wraps the slice in a [`Cursor`] so no copy is made.
    ///
    /// # Errors
    ///
    /// Returns an error if the archive headers or filename table are invalid,
    /// truncated, or cannot be decompressed.
    pub fn from_bytes(data: &'a [u8]) -> Result<Self> {
        Self::new(Cursor::new(data))
    }
}

impl<D: AsRef<[u8]>> Reader<crate::crypto::decrypt::Reader<Cursor<D>>> {
    /// Parse an encrypted ERA archive backed by contiguous bytes.
    ///
    /// This constructor enables [`Self::read_entry_direct`], which decrypts
    /// complete entry ranges in bulk.
    ///
    /// # Errors
    ///
    /// Returns an error if decryption or archive parsing fails.
    pub fn from_encrypted_bytes(data: D, keys: crate::TeaKeys) -> Result<Self> {
        Self::new(crate::crypto::decrypt::Reader::new(Cursor::new(data), keys))
    }

    /// Read and decompress an entry directly from an in-memory encrypted source.
    ///
    /// Unlike [`Self::read_entry`], this copies the entry's contiguous encrypted
    /// range once and decrypts all blocks in bulk. Large entries use the Rayon
    /// implementation when that feature is enabled. This avoids one seek and
    /// read operation per 64-byte cipher block and does not mutate reader state.
    ///
    /// # Errors
    ///
    /// Returns an error if `index` is invalid, an offset or range cannot be
    /// represented, the encrypted range is truncated, or decryption or
    /// decompression fails.
    pub fn read_entry_direct(&self, index: usize) -> Result<Vec<u8>> {
        const PARALLEL_THRESHOLD: usize = 256 * 1024;

        let entry = self
            .entries
            .get(index)
            .ok_or(Error::ChunkIndexOutOfBounds {
                index,
                count: self.entries.len(),
            })?;
        let start =
            usize::try_from(entry.chunk.offset).map_err(|_| Error::SizeOverflow("entry offset"))?;
        let size =
            usize::try_from(entry.chunk.size).map_err(|_| Error::SizeOverflow("entry size"))?;
        let end = start
            .checked_add(size)
            .ok_or(Error::SizeOverflow("entry range"))?;
        let block_start = start / crate::TEA_BLOCK_SIZE * crate::TEA_BLOCK_SIZE;
        let block_end = end
            .checked_add(crate::TEA_BLOCK_SIZE - 1)
            .ok_or(Error::SizeOverflow("aligned entry range"))?
            / crate::TEA_BLOCK_SIZE
            * crate::TEA_BLOCK_SIZE;
        let encrypted_source = self.inner.get_ref().get_ref().as_ref();
        let encrypted = encrypted_source
            .get(block_start..block_end)
            .ok_or(Error::UnexpectedEof)?;
        let mut decrypted = encrypted.to_vec();
        let block_offset =
            u64::try_from(block_start).map_err(|_| Error::SizeOverflow("entry offset"))?;
        let keys = self.inner.keys();

        #[cfg(feature = "rayon")]
        if decrypted.len() >= PARALLEL_THRESHOLD {
            crate::crypto::tea::tea_decrypt_data_parallel(&keys, &mut decrypted, block_offset)?;
        } else {
            crate::crypto::tea::tea_decrypt_data(&keys, &mut decrypted, block_offset)?;
        }
        #[cfg(not(feature = "rayon"))]
        {
            let _ = PARALLEL_THRESHOLD;
            crate::crypto::tea::tea_decrypt_data(&keys, &mut decrypted, block_offset)?;
        }

        let payload_start = start - block_start;
        let payload_end = payload_start
            .checked_add(size)
            .ok_or(Error::SizeOverflow("entry range"))?;
        let payload = decrypted
            .get(payload_start..payload_end)
            .ok_or(Error::UnexpectedEof)?;
        entry.decompress(payload)
    }
}

impl<R: Read + Seek> Reader<crate::crypto::decrypt::Reader<R>> {
    /// Parse an encrypted ERA archive, decrypting on the fly.
    ///
    /// Wraps the source in a [`crate::crypto::decrypt::Reader`] so data is decrypted
    /// block-by-block as it is read — the full archive is never materialised
    /// in memory.
    ///
    /// # Errors
    ///
    /// Returns an error if decryption or archive parsing fails.
    pub fn from_encrypted(inner: R, keys: crate::TeaKeys) -> Result<Self> {
        Self::new(crate::crypto::decrypt::Reader::new(inner, keys))
    }
}
