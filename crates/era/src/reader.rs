//! ERA archive reader.

use alloc::borrow::Cow;
use alloc::vec::Vec;

use ecf::{EcfChunkHeader, EcfHeader, HEADER_MAGIC};

use crate::crypto::{TeaKeys, tea_decrypt_data};
use crate::error::{Error, Result};
use crate::header::{EraArchiveHeader, EraChunkExtra, EraEntry, resolve_filename};

/// Compressed entry data: (compressed_bytes, decompressed_size, tiger128_hash).
pub type CompressedEntryData = (Vec<u8>, u32, [u8; 16]);

/// Parse chunk headers from a byte slice at the given offset.
///
/// Returns the parsed entries and the new offset after all headers.
fn parse_chunk_headers(
    data: &[u8],
    offset: usize,
    ecf_header: &EcfHeader,
) -> Result<(Vec<EraEntry>, usize)> {
    let mut entries = Vec::with_capacity(ecf_header.num_chunks as usize);
    let mut pos = offset;
    let stride = EcfChunkHeader::SIZE + ecf_header.chunk_extra_data_size as usize;

    for _ in 0..ecf_header.num_chunks {
        if pos + EcfChunkHeader::SIZE > data.len() {
            return Err(Error::UnexpectedEof);
        }
        let chunk = EcfChunkHeader::from_bytes(&data[pos..pos + EcfChunkHeader::SIZE])?;
        pos += EcfChunkHeader::SIZE;

        let extra = if ecf_header.chunk_extra_data_size >= EraChunkExtra::SIZE as u16 {
            if pos + EraChunkExtra::SIZE > data.len() {
                return Err(Error::UnexpectedEof);
            }
            let extra = EraChunkExtra::from_bytes(&data[pos..])?;
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

    // Verify we didn't skip past stride boundaries
    let expected_end = offset + stride * ecf_header.num_chunks as usize;
    Ok((entries, expected_end))
}

/// An ERA archive reader.
///
/// Holds either a borrowed slice (already decrypted) or an owned `Vec<u8>`
/// (decrypted in-place during auto-detection).
pub struct Reader<'a> {
    data: Cow<'a, [u8]>,
    /// ECF header.
    pub ecf_header: EcfHeader,
    /// Archive header extension.
    pub archive_header: EraArchiveHeader,
    /// File entries.
    pub entries: Vec<EraEntry>,
}

/// Check whether the first 4 bytes match the ECF magic.
fn looks_decrypted(data: &[u8]) -> bool {
    if data.len() < 4 {
        return false;
    }
    let magic = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
    magic == HEADER_MAGIC
}

impl<'a> Reader<'a> {
    /// Parse an ERA archive from a decrypted byte slice (zero-copy).
    pub fn from_decrypted(data: &'a [u8]) -> Result<Self> {
        Self::parse(Cow::Borrowed(data))
    }

    /// Decrypt `data` in-place with the given keys, then parse.
    ///
    /// The `Vec` is consumed and owned by the returned `Reader`.
    pub fn from_encrypted(mut data: Vec<u8>, keys: &TeaKeys) -> Result<Self> {
        // Pad to TEA_BLOCK_SIZE boundary for in-place decrypt
        let block = crate::crypto::TEA_BLOCK_SIZE;
        let remainder = data.len() % block;
        if remainder != 0 {
            data.resize(data.len() + (block - remainder), 0);
        }
        tea_decrypt_data(keys, &mut data, 0);
        Self::parse(Cow::Owned(data))
    }

    /// Auto-detect: if the data starts with the ECF magic it is treated as
    /// already decrypted (borrowed); otherwise it is decrypted in-place with
    /// the supplied keys and owned by the reader.
    pub fn new(data: &'a [u8], keys: &TeaKeys) -> Result<Self> {
        if looks_decrypted(data) {
            Self::from_decrypted(data)
        } else {
            Self::from_encrypted(data.to_vec(), keys)
        }
    }

    /// Internal: parse from a `Cow` that already contains decrypted bytes.
    fn parse(data: Cow<'a, [u8]>) -> Result<Self> {
        if data.len() < EcfHeader::SIZE + EraArchiveHeader::SIZE {
            return Err(Error::UnexpectedEof);
        }

        let ecf_header = EcfHeader::from_bytes(&data[..EcfHeader::SIZE])?;

        let archive_header = EraArchiveHeader::from_bytes(
            &data[EcfHeader::SIZE..EcfHeader::SIZE + EraArchiveHeader::SIZE],
        )?;

        let chunk_start = ecf_header.header_size as usize;
        let (mut entries, _) = parse_chunk_headers(&data, chunk_start, &ecf_header)?;

        // Read and decompress filename table (always at index 0)
        let filename_table = if !entries.is_empty() {
            let e = &entries[0];
            let start = e.chunk.offset as usize;
            let end = start + e.chunk.size as usize;
            if end > data.len() {
                return Err(Error::UnexpectedEof);
            }
            entries[0].decompress(&data[start..end])?
        } else {
            Vec::new()
        };

        for entry in entries.iter_mut().skip(1) {
            entry.filename = resolve_filename(&filename_table, entry.extra.name_offset);
        }

        Ok(Self {
            data,
            ecf_header,
            archive_header,
            entries,
        })
    }

    /// Get the number of entries in the archive.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if the archive is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get an entry by index.
    pub fn entry(&self, index: usize) -> Option<&EraEntry> {
        self.entries.get(index)
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
    pub fn read_entry(&self, index: usize) -> Result<Vec<u8>> {
        let entry = self
            .entries
            .get(index)
            .ok_or(Error::ChunkIndexOutOfBounds {
                index,
                count: self.entries.len(),
            })?;

        let start = entry.chunk.offset as usize;
        let end = start + entry.chunk.size as usize;
        if end > self.data.len() {
            return Err(Error::UnexpectedEof);
        }

        entry.decompress(&self.data[start..end])
    }

    /// Read compressed data for an entry WITHOUT decompressing.
    ///
    /// Returns: (compressed_data, decompressed_size, tiger128_hash).
    pub fn read_entry_compressed(&self, index: usize) -> Result<CompressedEntryData> {
        let entry = self
            .entries
            .get(index)
            .ok_or(Error::ChunkIndexOutOfBounds {
                index,
                count: self.entries.len(),
            })?;

        let start = entry.chunk.offset as usize;
        let end = start + entry.chunk.size as usize;
        if end > self.data.len() {
            return Err(Error::UnexpectedEof);
        }

        Ok((
            self.data[start..end].to_vec(),
            entry.extra.decomp_size,
            entry.extra.comp_tiger128,
        ))
    }

    /// Read multiple entries sequentially.
    pub fn read_entries(&self, indices: &[usize]) -> Result<Vec<Vec<u8>>> {
        indices.iter().map(|&idx| self.read_entry(idx)).collect()
    }

    /// Read compressed data for multiple entries sequentially.
    pub fn read_entries_compressed(&self, indices: &[usize]) -> Result<Vec<CompressedEntryData>> {
        indices
            .iter()
            .map(|&idx| self.read_entry_compressed(idx))
            .collect()
    }
}
