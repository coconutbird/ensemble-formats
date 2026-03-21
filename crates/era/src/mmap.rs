//! Memory-mapped ERA archive reader with parallel decompression support
//!
//! This module provides high-performance ERA reading by memory-mapping the archive
//! file and supporting parallel decompression of multiple entries.

use std::fs::File;
use std::io::{Cursor, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;

use memmap2::Mmap;
use rayon::prelude::*;

use crate::crypto::{TEA_BLOCK_SIZE, TeaKeys, tea_decrypt_data};
use crate::era::{EraArchiveHeader, EraEntry, parse_chunk_headers, resolve_filename};
use crate::error::{Error, Result};
use ecf::EcfHeader;

/// Memory-mapped ERA archive for parallel operations
pub struct MmapEraArchive {
    /// Memory-mapped file data (encrypted)
    mmap: Arc<Mmap>,
    /// TEA decryption keys
    keys: TeaKeys,
    /// ECF header
    pub ecf_header: EcfHeader,
    /// Archive header extension
    pub archive_header: EraArchiveHeader,
    /// File entries
    pub entries: Vec<EraEntry>,
}

impl MmapEraArchive {
    /// Open an ERA archive with memory mapping for parallel access
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file)? };
        let keys = TeaKeys::default_archive_keys();

        Self::from_mmap(mmap, keys)
    }

    /// Create from an existing memory map
    fn from_mmap(mmap: Mmap, keys: TeaKeys) -> Result<Self> {
        // Decrypt and parse headers
        // Headers are at the start of the file, we need to decrypt them first
        let base_header_size: usize = 32 + 16; // EcfHeader + EraArchiveHeader minimum

        // Read and decrypt header area (at least first block)
        let header_blocks = base_header_size.div_ceil(TEA_BLOCK_SIZE);
        let header_bytes = header_blocks * TEA_BLOCK_SIZE;

        let mut decrypted_header = mmap[..header_bytes.min(mmap.len())].to_vec();
        // Pad to block boundary if needed
        decrypted_header.resize(header_bytes, 0);
        tea_decrypt_data(&keys, &mut decrypted_header, 0);

        // Parse ECF header
        let mut cursor = Cursor::new(&decrypted_header);
        let ecf_header = EcfHeader::read(&mut cursor)?;

        // Parse ERA archive header
        let archive_header = EraArchiveHeader::read(&mut cursor)?;

        // The chunk headers start at ecf_header.header_size (accounts for signature/padding)
        let chunk_headers_start = ecf_header.header_size as usize;
        let chunk_header_size = 24 + ecf_header.chunk_extra_data_size as usize; // EcfChunkHeader + extra
        let total_header_size =
            chunk_headers_start + chunk_header_size * ecf_header.num_chunks as usize;

        // Decrypt full header area
        let full_header_blocks = total_header_size.div_ceil(TEA_BLOCK_SIZE);
        let full_header_bytes = full_header_blocks * TEA_BLOCK_SIZE;

        let mut full_header = mmap[..full_header_bytes.min(mmap.len())].to_vec();
        full_header.resize(full_header_bytes, 0);
        tea_decrypt_data(&keys, &mut full_header, 0);

        // Re-parse with full header - seek to where chunk headers start
        let mut cursor = Cursor::new(&full_header);
        cursor.seek(SeekFrom::Start(chunk_headers_start as u64))?;

        // Parse chunk headers using shared function
        let mut entries = parse_chunk_headers(&mut cursor, &ecf_header)?;

        // Read and parse filename table
        let filename_table = if !entries.is_empty() {
            Self::read_filename_table_mmap(&mmap, &keys, &entries[0])?
        } else {
            Vec::new()
        };

        // Resolve filenames using shared function
        for (i, entry) in entries.iter_mut().enumerate() {
            if i > 0 {
                entry.filename = resolve_filename(&filename_table, entry.extra.name_offset);
            }
        }

        Ok(Self {
            mmap: Arc::new(mmap),
            keys,
            ecf_header,
            archive_header,
            entries,
        })
    }

    fn read_filename_table_mmap(mmap: &Mmap, keys: &TeaKeys, entry: &EraEntry) -> Result<Vec<u8>> {
        let offset = entry.chunk.offset as usize;
        let size = entry.chunk.size as usize;

        // Align to block boundaries for decryption
        let block_start = (offset / TEA_BLOCK_SIZE) * TEA_BLOCK_SIZE;
        let block_end = (offset + size).div_ceil(TEA_BLOCK_SIZE) * TEA_BLOCK_SIZE;

        let mut data = mmap[block_start..block_end.min(mmap.len())].to_vec();
        data.resize(block_end - block_start, 0);
        tea_decrypt_data(keys, &mut data, block_start as u64);

        // Extract the actual data from within the decrypted blocks
        let data_start = offset - block_start;
        let compressed = &data[data_start..data_start + size];

        entry.decompress(compressed)
    }

    /// Get the number of entries
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get an entry by index
    pub fn entry(&self, index: usize) -> Option<&EraEntry> {
        self.entries.get(index)
    }

    /// Iterate over entries
    pub fn iter(&self) -> impl Iterator<Item = &EraEntry> {
        self.entries.iter()
    }

    /// Find entry by filename
    pub fn find_by_name(&self, name: &str) -> Option<usize> {
        let name_lower = name.to_lowercase().replace('/', "\\");
        self.entries.iter().position(|e| {
            e.filename
                .as_ref()
                .is_some_and(|f| f.to_lowercase() == name_lower)
        })
    }

    /// Read a single entry (thread-safe due to memory mapping)
    pub fn read_entry(&self, index: usize) -> Result<Vec<u8>> {
        let entry = self
            .entries
            .get(index)
            .ok_or(Error::ChunkIndexOutOfBounds {
                index,
                count: self.entries.len(),
            })?;

        let offset = entry.chunk.offset as usize;
        let size = entry.chunk.size as usize;

        // Align to block boundaries
        let block_start = (offset / TEA_BLOCK_SIZE) * TEA_BLOCK_SIZE;
        let block_end = (offset + size).div_ceil(TEA_BLOCK_SIZE) * TEA_BLOCK_SIZE;

        let mut data = self.mmap[block_start..block_end.min(self.mmap.len())].to_vec();
        data.resize(block_end - block_start, 0);
        tea_decrypt_data(&self.keys, &mut data, block_start as u64);

        let data_start = offset - block_start;
        let compressed = &data[data_start..data_start + size];

        entry.decompress(compressed)
    }

    /// Read compressed data without decompressing (thread-safe)
    pub fn read_entry_compressed(&self, index: usize) -> Result<crate::CompressedEntryData> {
        let entry = self
            .entries
            .get(index)
            .ok_or(Error::ChunkIndexOutOfBounds {
                index,
                count: self.entries.len(),
            })?;

        let offset = entry.chunk.offset as usize;
        let size = entry.chunk.size as usize;

        let block_start = (offset / TEA_BLOCK_SIZE) * TEA_BLOCK_SIZE;
        let block_end = (offset + size).div_ceil(TEA_BLOCK_SIZE) * TEA_BLOCK_SIZE;

        let mut data = self.mmap[block_start..block_end.min(self.mmap.len())].to_vec();
        data.resize(block_end - block_start, 0);
        tea_decrypt_data(&self.keys, &mut data, block_start as u64);

        let data_start = offset - block_start;
        let compressed = data[data_start..data_start + size].to_vec();

        Ok((
            compressed,
            entry.extra.decomp_size,
            entry.extra.comp_tiger128,
        ))
    }

    /// Read multiple entries in parallel
    ///
    /// This is the main performance optimization - decompresses multiple files
    /// concurrently using all available CPU cores.
    pub fn read_entries_parallel(&self, indices: &[usize]) -> Result<Vec<Vec<u8>>> {
        indices
            .par_iter()
            .map(|&idx| self.read_entry(idx))
            .collect()
    }

    /// Read all file entries in parallel (skips filename table at index 0)
    pub fn read_all_parallel(&self) -> Result<Vec<Vec<u8>>> {
        let indices: Vec<usize> = (1..self.entries.len()).collect();
        self.read_entries_parallel(&indices)
    }

    /// Read compressed data for multiple entries in parallel
    pub fn read_entries_compressed_parallel(
        &self,
        indices: &[usize],
    ) -> Result<Vec<crate::CompressedEntryData>> {
        indices
            .par_iter()
            .map(|&idx| self.read_entry_compressed(idx))
            .collect()
    }
}
