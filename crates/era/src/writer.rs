//! ERA archive writer.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use tiger::{Digest, Tiger};

use crate::era::{EraArchiveHeader, EraChunkExtra};
use crate::error::Result;

/// ERA file ID constant.
const ERA_FILE_ID: u32 = 0x17FDBA9C;

/// ERA chunk ID (used for both filename table and file entries).
const ERA_CHUNK_ID: u64 = 0x8DAFB100;

/// A file to be added to an ERA archive (uncompressed).
struct PendingFile {
    filename: String,
    data: Vec<u8>,
}

/// A pre-compressed file to be added to an ERA archive.
struct PreCompressedFile {
    filename: String,
    compressed_data: Vec<u8>,
    decompressed_size: u32,
    tiger128: [u8; 16],
}

/// Compressed data with its Tiger128 hash.
pub struct CompressedData {
    /// Compressed bytes.
    pub data: Vec<u8>,
    /// Tiger128 hash of compressed data.
    pub tiger128: [u8; 16],
    /// Original decompressed size.
    pub decompressed_size: u32,
}

/// ERA archive writer.
pub struct Writer {
    files: Vec<PendingFile>,
    precompressed: Vec<PreCompressedFile>,
}

impl Writer {
    /// Create a new ERA writer.
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            precompressed: Vec::new(),
        }
    }

    /// Add a file to the archive (will be compressed during write).
    pub fn add_file(&mut self, filename: impl Into<String>, data: Vec<u8>) {
        let filename = filename.into().replace('/', "\\");
        self.files.push(PendingFile { filename, data });
    }

    /// Add a pre-compressed file to the archive (skips compression).
    pub fn add_compressed_file(
        &mut self,
        filename: impl Into<String>,
        compressed_data: Vec<u8>,
        decompressed_size: u32,
        tiger128: [u8; 16],
    ) {
        let filename = filename.into().replace('/', "\\");
        self.precompressed.push(PreCompressedFile {
            filename,
            compressed_data,
            decompressed_size,
            tiger128,
        });
    }

    /// Build the archive into a `Vec<u8>`.
    pub fn finalize(&self) -> Result<Vec<u8>> {
        self.finalize_with_progress(None)
    }

    /// Build the archive into a `Vec<u8>` with optional progress callback.
    ///
    /// The progress callback receives `(bytes_written, total_bytes)` and should
    /// return `true` to continue or `false` to cancel.
    pub fn finalize_with_progress(
        &self,
        mut progress: Option<&mut dyn FnMut(u64, u64) -> bool>,
    ) -> Result<Vec<u8>> {
        // Build filename table
        let mut filename_table = Vec::new();
        let mut name_offsets = Vec::new();

        for file in &self.files {
            name_offsets.push(filename_table.len() as u32);
            filename_table.extend_from_slice(file.filename.as_bytes());
            filename_table.push(0);
        }

        let precomp_name_start = name_offsets.len();
        for file in &self.precompressed {
            name_offsets.push(filename_table.len() as u32);
            filename_table.extend_from_slice(file.filename.as_bytes());
            filename_table.push(0);
        }

        // Compress filename table
        let compressed_names = compress_data(&filename_table);

        // Compress all file data
        let compressed_files: Vec<CompressedData> =
            self.files.iter().map(|f| compress_data(&f.data)).collect();

        // Calculate layout
        let total_files = self.files.len() + self.precompressed.len();
        let num_chunks = 1 + total_files;
        let total_header_size = ecf::EcfHeader::SIZE + EraArchiveHeader::SIZE;
        let chunk_header_size = ecf::EcfChunkHeader::SIZE + EraChunkExtra::SIZE;
        let headers_size = total_header_size + chunk_header_size * num_chunks;

        let mut data_offset = align16(headers_size);
        let mut chunks = Vec::with_capacity(num_chunks);

        // Filename table chunk (index 0)
        chunks.push(ChunkLayout {
            id: ERA_CHUNK_ID,
            offset: data_offset as u32,
            size: compressed_names.data.len() as u32,
            decomp_size: filename_table.len() as u32,
            name_offset: 0,
            comp_tiger128: compressed_names.tiger128,
        });
        data_offset = align16(data_offset + compressed_names.data.len());

        // Regular file chunks
        for (i, (file, compressed)) in self.files.iter().zip(&compressed_files).enumerate() {
            chunks.push(ChunkLayout {
                id: ERA_CHUNK_ID,
                offset: data_offset as u32,
                size: compressed.data.len() as u32,
                decomp_size: file.data.len() as u32,
                name_offset: name_offsets[i],
                comp_tiger128: compressed.tiger128,
            });
            data_offset = align16(data_offset + compressed.data.len());
        }

        // Pre-compressed file chunks
        for (i, file) in self.precompressed.iter().enumerate() {
            chunks.push(ChunkLayout {
                id: ERA_CHUNK_ID,
                offset: data_offset as u32,
                size: file.compressed_data.len() as u32,
                decomp_size: file.decompressed_size,
                name_offset: name_offsets[precomp_name_start + i],
                comp_tiger128: file.tiger128,
            });
            data_offset = align16(data_offset + file.compressed_data.len());
        }

        let ecf_header = HeaderLayout {
            header_size: total_header_size as u32,
            file_size: data_offset as u32,
            num_chunks: num_chunks as u16,
            id: ERA_FILE_ID,
            chunk_extra_data_size: 32,
        };

        let adler32 = compute_header_adler32(&ecf_header, &chunks);

        // Allocate output buffer
        let mut out = vec![0u8; data_offset];

        // Write ECF header (32 bytes)
        write_ecf_header(&mut out[..ecf::EcfHeader::SIZE], &ecf_header, adler32);

        // Write ERA archive header
        out[ecf::EcfHeader::SIZE..ecf::EcfHeader::SIZE + EraArchiveHeader::SIZE]
            .copy_from_slice(&EraArchiveHeader::new().to_bytes());

        // Write chunk headers
        let mut pos = total_header_size;
        for chunk in &chunks {
            write_chunk_header(&mut out[pos..], chunk);
            pos += chunk_header_size;
        }

        // Write chunk data with progress
        let total_bytes: u64 = compressed_names.data.len() as u64
            + compressed_files
                .iter()
                .map(|f| f.data.len() as u64)
                .sum::<u64>()
            + self
                .precompressed
                .iter()
                .map(|f| f.compressed_data.len() as u64)
                .sum::<u64>();
        let mut bytes_written: u64 = 0;

        // Filename table
        let off = chunks[0].offset as usize;
        out[off..off + compressed_names.data.len()].copy_from_slice(&compressed_names.data);
        bytes_written += compressed_names.data.len() as u64;
        if let Some(cb) = &mut progress
            && !cb(bytes_written, total_bytes)
        {
            return Err(crate::error::Error::Cancelled);
        }

        // Regular files
        let regular_count = compressed_files.len();
        for (chunk, file) in chunks[1..=regular_count].iter().zip(&compressed_files) {
            let off = chunk.offset as usize;
            out[off..off + file.data.len()].copy_from_slice(&file.data);
            bytes_written += file.data.len() as u64;
            if let Some(cb) = &mut progress
                && !cb(bytes_written, total_bytes)
            {
                return Err(crate::error::Error::Cancelled);
            }
        }

        // Pre-compressed files
        for (chunk, file) in chunks[regular_count + 1..].iter().zip(&self.precompressed) {
            let off = chunk.offset as usize;
            out[off..off + file.compressed_data.len()].copy_from_slice(&file.compressed_data);
            bytes_written += file.compressed_data.len() as u64;
            if let Some(cb) = &mut progress
                && !cb(bytes_written, total_bytes)
            {
                return Err(crate::error::Error::Cancelled);
            }
        }

        Ok(out)
    }
}

impl Default for Writer {
    fn default() -> Self {
        Self::new()
    }
}

/// Internal ECF header layout.
struct HeaderLayout {
    header_size: u32,
    file_size: u32,
    num_chunks: u16,
    id: u32,
    chunk_extra_data_size: u16,
}

/// Internal chunk layout.
struct ChunkLayout {
    id: u64,
    offset: u32,
    size: u32,
    decomp_size: u32,
    name_offset: u32,
    comp_tiger128: [u8; 16],
}

/// Align to 16-byte boundary.
fn align16(n: usize) -> usize {
    (n + 15) & !15
}

/// Compress data and compute its Tiger128 hash.
fn compress_data(data: &[u8]) -> CompressedData {
    let decompressed_size = data.len() as u32;
    let compressed = miniz_oxide::deflate::compress_to_vec(data, 6);

    let hash = Tiger::digest(&compressed);
    let mut tiger128 = [0u8; 16];
    tiger128.copy_from_slice(&hash[..16]);

    CompressedData {
        data: compressed,
        tiger128,
        decompressed_size,
    }
}

/// Compress data and compute its Tiger128 hash (public API).
pub fn compress_file_data(data: &[u8]) -> CompressedData {
    compress_data(data)
}

/// Compute adler32 over header fields and chunk headers.
fn compute_header_adler32(header: &HeaderLayout, chunks: &[ChunkLayout]) -> u32 {
    let mut data = Vec::new();

    data.extend_from_slice(&header.file_size.to_be_bytes());
    data.extend_from_slice(&header.num_chunks.to_be_bytes());
    data.extend_from_slice(&0u16.to_be_bytes()); // flags
    data.extend_from_slice(&header.id.to_be_bytes());
    data.extend_from_slice(&header.chunk_extra_data_size.to_be_bytes());
    data.extend_from_slice(&0u16.to_be_bytes()); // pad0
    data.extend_from_slice(&0u32.to_be_bytes()); // pad1

    for chunk in chunks {
        data.extend_from_slice(&chunk.id.to_be_bytes());
        data.extend_from_slice(&chunk.offset.to_be_bytes());
        data.extend_from_slice(&chunk.size.to_be_bytes());
        data.extend_from_slice(&0u32.to_be_bytes()); // adler32
        data.push(0); // flags
        data.push(4); // alignment_log2 = 4 (16 bytes)
        data.extend_from_slice(&0x0001u16.to_be_bytes()); // resource_flags: deflate raw
    }

    ecf::adler32(&data)
}

/// Write ECF header into a buffer (must be at least 32 bytes).
fn write_ecf_header(buf: &mut [u8], header: &HeaderLayout, adler32: u32) {
    buf[0..4].copy_from_slice(&0xDABA7737u32.to_be_bytes());
    buf[4..8].copy_from_slice(&header.header_size.to_be_bytes());
    buf[8..12].copy_from_slice(&adler32.to_be_bytes());
    buf[12..16].copy_from_slice(&header.file_size.to_be_bytes());
    buf[16..18].copy_from_slice(&header.num_chunks.to_be_bytes());
    buf[18..20].copy_from_slice(&0u16.to_be_bytes()); // flags
    buf[20..24].copy_from_slice(&header.id.to_be_bytes());
    buf[24..26].copy_from_slice(&header.chunk_extra_data_size.to_be_bytes());
    buf[26..28].copy_from_slice(&0u16.to_be_bytes()); // pad0
    buf[28..32].copy_from_slice(&0u32.to_be_bytes()); // pad1
}

/// Write a chunk header with extra data into a buffer.
fn write_chunk_header(buf: &mut [u8], chunk: &ChunkLayout) {
    // ChunkHeader (24 bytes)
    buf[0..8].copy_from_slice(&chunk.id.to_be_bytes());
    buf[8..12].copy_from_slice(&chunk.offset.to_be_bytes());
    buf[12..16].copy_from_slice(&chunk.size.to_be_bytes());
    buf[16..20].copy_from_slice(&0u32.to_be_bytes()); // adler32
    buf[20] = 1; // flags: 1 = DeflateRaw compression
    buf[21] = 4; // alignment_log2 = 4 (16 bytes)
    buf[22..24].copy_from_slice(&0x0000u16.to_be_bytes()); // resource_flags

    // EraChunkExtra (32 bytes)
    let extra = EraChunkExtra::new(chunk.decomp_size, chunk.name_offset, chunk.comp_tiger128);
    buf[24..56].copy_from_slice(&extra.to_bytes());
}
