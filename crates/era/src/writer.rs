//! ERA archive writer

use std::fs::OpenOptions;
use std::io::{Seek, Write};
use std::path::Path;

use byteorder::{BigEndian, WriteBytesExt};
use flate2::Compression;
use flate2::write::DeflateEncoder;
use rayon::prelude::*;
use tiger::{Digest, Tiger};

use crate::crypto::TeaKeys;
use crate::encrypt_writer::EncryptWriter;
use crate::era::{EraArchiveHeader, EraChunkExtra};
use crate::error::Result;

/// ERA file ID constant
const ERA_FILE_ID: u32 = 0x17FDBA9C;

/// ERA chunk ID (used for both filename table and file entries)
const ERA_CHUNK_ID: u64 = 0x8DAFB100;

/// A file to be added to an ERA archive (uncompressed)
struct PendingFile {
    /// Filename (relative path with backslashes)
    filename: String,
    /// Uncompressed file data
    data: Vec<u8>,
}

/// A pre-compressed file to be added to an ERA archive
struct PreCompressedFile {
    /// Filename (relative path with backslashes)
    filename: String,
    /// Already compressed data
    compressed_data: Vec<u8>,
    /// Decompressed size
    decompressed_size: u32,
    /// Tiger128 hash of compressed data
    tiger128: [u8; 16],
}

/// Compressed data with its Tiger128 hash
pub struct CompressedData {
    /// Compressed bytes
    pub data: Vec<u8>,
    /// Tiger128 hash of compressed data
    pub tiger128: [u8; 16],
    /// Original decompressed size
    pub decompressed_size: u32,
}

/// ERA archive writer with parallel compression support
pub struct EraWriter {
    /// Uncompressed files to be compressed during write
    files: Vec<PendingFile>,
    /// Pre-compressed files (skip compression)
    precompressed: Vec<PreCompressedFile>,
}

impl EraWriter {
    /// Create a new ERA writer
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            precompressed: Vec::new(),
        }
    }

    /// Add a file to the archive (will be compressed during write)
    pub fn add_file(&mut self, filename: impl Into<String>, data: Vec<u8>) {
        let filename = filename.into().replace('/', "\\");
        self.files.push(PendingFile { filename, data });
    }

    /// Add a pre-compressed file to the archive (skips compression)
    ///
    /// This is useful when copying files from another archive without
    /// decompressing and recompressing them.
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

    /// Write the archive to a file
    pub fn write_to_file<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        // Open with read+write since EncryptWriter may need to read back blocks
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;
        let keys = TeaKeys::default_archive_keys();
        let encrypt_writer = EncryptWriter::new(file, keys);
        self.write(encrypt_writer)
    }

    /// Write the archive to a writer (should be an EncryptWriter for proper encryption)
    pub fn write<W: Write + Seek>(&self, writer: W) -> Result<()> {
        self.write_with_progress(writer, None)
    }

    /// Write the archive to a writer with optional progress callback
    ///
    /// The progress callback receives `(bytes_written, total_bytes)` and should return
    /// `true` to continue or `false` to cancel the operation.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use era::{EraWriter, EncryptWriter, TeaKeys};
    /// use std::fs::OpenOptions;
    ///
    /// let writer = EraWriter::new();
    /// let file = OpenOptions::new()
    ///     .read(true)
    ///     .write(true)
    ///     .create(true)
    ///     .truncate(true)
    ///     .open("output.era")
    ///     .unwrap();
    /// let keys = TeaKeys::default_archive_keys();
    /// let encrypt_writer = EncryptWriter::new(file, keys);
    ///
    /// writer.write_with_progress(encrypt_writer, Some(&mut |written, total| {
    ///     println!("Progress: {}/{} bytes ({:.1}%)", written, total, (written as f64 / total as f64) * 100.0);
    ///     true // return false to cancel
    /// })).unwrap();
    /// ```
    pub fn write_with_progress<W: Write + Seek>(
        &self,
        mut writer: W,
        mut progress: Option<&mut dyn FnMut(u64, u64) -> bool>,
    ) -> Result<()> {
        // Build filename table (includes both regular and precompressed files)
        let mut filename_table = Vec::new();
        let mut name_offsets = Vec::new();

        // Add regular files to filename table
        for file in &self.files {
            name_offsets.push(filename_table.len() as u32);
            filename_table.extend_from_slice(file.filename.as_bytes());
            filename_table.push(0); // null terminator
        }

        // Add precompressed files to filename table
        let precomp_name_start = name_offsets.len();
        for file in &self.precompressed {
            name_offsets.push(filename_table.len() as u32);
            filename_table.extend_from_slice(file.filename.as_bytes());
            filename_table.push(0); // null terminator
        }

        // Compress filename table
        let compressed_names = compress_data(&filename_table)?;

        // Compress all file data IN PARALLEL using rayon
        let compressed_files: Vec<CompressedData> = self
            .files
            .par_iter()
            .map(|f| compress_data(&f.data))
            .collect::<Result<_>>()?;

        // Calculate layout
        let total_files = self.files.len() + self.precompressed.len();
        let num_chunks = 1 + total_files; // filename table + files
        let total_header_size = 32 + 16; // EcfHeader + EraArchiveHeader
        let chunk_header_size = 24 + 32; // EcfChunkHeader + EraChunkExtra
        let headers_size = total_header_size + chunk_header_size * num_chunks;

        let mut data_offset = align16(headers_size);
        let mut chunks = Vec::with_capacity(num_chunks);

        // Filename table chunk (index 0)
        chunks.push(ChunkData {
            id: ERA_CHUNK_ID,
            offset: data_offset as u32,
            size: compressed_names.data.len() as u32,
            decomp_size: filename_table.len() as u32,
            name_offset: 0,
            comp_tiger128: compressed_names.tiger128,
        });
        data_offset = align16(data_offset + compressed_names.data.len());

        // Regular file chunks (freshly compressed)
        for (i, (file, compressed)) in self.files.iter().zip(&compressed_files).enumerate() {
            chunks.push(ChunkData {
                id: ERA_CHUNK_ID,
                offset: data_offset as u32,
                size: compressed.data.len() as u32,
                decomp_size: file.data.len() as u32,
                name_offset: name_offsets[i],
                comp_tiger128: compressed.tiger128,
            });
            data_offset = align16(data_offset + compressed.data.len());
        }

        // Pre-compressed file chunks (skip compression)
        for (i, file) in self.precompressed.iter().enumerate() {
            chunks.push(ChunkData {
                id: ERA_CHUNK_ID,
                offset: data_offset as u32,
                size: file.compressed_data.len() as u32,
                decomp_size: file.decompressed_size,
                name_offset: name_offsets[precomp_name_start + i],
                comp_tiger128: file.tiger128,
            });
            data_offset = align16(data_offset + file.compressed_data.len());
        }

        // Build ECF header
        let ecf_header = EcfHeaderData {
            header_size: total_header_size as u32,
            file_size: data_offset as u32,
            num_chunks: num_chunks as u16,
            id: ERA_FILE_ID,
            chunk_extra_data_size: 32,
        };

        // Compute adler32 over header fields and chunk headers
        let adler32 = compute_header_adler32(&ecf_header, &chunks);

        // Write ECF header
        write_ecf_header(&mut writer, &ecf_header, adler32)?;

        // Write ERA archive header
        EraArchiveHeader::new().write(&mut writer)?;

        // Write chunk headers
        for chunk in &chunks {
            write_chunk_header(&mut writer, chunk)?;
        }

        // Write chunk data with padding and progress reporting
        write_chunk_data_all(
            &mut writer,
            &chunks,
            &compressed_names,
            &compressed_files,
            &self.precompressed,
            &mut progress,
        )?;

        writer.flush()?;
        Ok(())
    }
}

impl Default for EraWriter {
    fn default() -> Self {
        Self::new()
    }
}

/// Internal ECF header data
struct EcfHeaderData {
    header_size: u32,
    file_size: u32,
    num_chunks: u16,
    id: u32,
    chunk_extra_data_size: u16,
}

/// Internal chunk data
struct ChunkData {
    id: u64,
    offset: u32,
    size: u32,
    decomp_size: u32,
    name_offset: u32,
    comp_tiger128: [u8; 16],
}

/// Align to 16-byte boundary
fn align16(n: usize) -> usize {
    (n + 15) & !15
}

/// Compress data and compute its Tiger128 hash
fn compress_data(data: &[u8]) -> Result<CompressedData> {
    let decompressed_size = data.len() as u32;
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data)?;
    let compressed = encoder.finish()?;

    // Tiger128 = first 16 bytes of Tiger-192
    let hash = Tiger::digest(&compressed);
    let mut tiger128 = [0u8; 16];
    tiger128.copy_from_slice(&hash[..16]);

    Ok(CompressedData {
        data: compressed,
        tiger128,
        decompressed_size,
    })
}

/// Compress data and compute its Tiger128 hash (public version for external use)
pub fn compress_file_data(data: &[u8]) -> Result<CompressedData> {
    compress_data(data)
}

/// Compute adler32 over header fields and chunk headers
fn compute_header_adler32(header: &EcfHeaderData, chunks: &[ChunkData]) -> u32 {
    let mut data = Vec::new();

    // Header bytes after adler32 field
    data.extend_from_slice(&header.file_size.to_be_bytes());
    data.extend_from_slice(&header.num_chunks.to_be_bytes());
    data.extend_from_slice(&0u16.to_be_bytes()); // flags
    data.extend_from_slice(&header.id.to_be_bytes());
    data.extend_from_slice(&header.chunk_extra_data_size.to_be_bytes());
    data.extend_from_slice(&0u16.to_be_bytes()); // pad0
    data.extend_from_slice(&0u32.to_be_bytes()); // pad1

    // Chunk headers (base headers only, without extra data)
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

/// Write ECF header
fn write_ecf_header<W: Write>(writer: &mut W, header: &EcfHeaderData, adler32: u32) -> Result<()> {
    writer.write_u32::<BigEndian>(0xDABA7737)?; // ECF magic
    writer.write_u32::<BigEndian>(header.header_size)?;
    writer.write_u32::<BigEndian>(adler32)?;
    writer.write_u32::<BigEndian>(header.file_size)?;
    writer.write_u16::<BigEndian>(header.num_chunks)?;
    writer.write_u16::<BigEndian>(0)?; // flags
    writer.write_u32::<BigEndian>(header.id)?;
    writer.write_u16::<BigEndian>(header.chunk_extra_data_size)?;
    writer.write_u16::<BigEndian>(0)?; // pad0
    writer.write_u32::<BigEndian>(0)?; // pad1
    Ok(())
}

/// Write a chunk header with extra data
fn write_chunk_header<W: Write>(writer: &mut W, chunk: &ChunkData) -> Result<()> {
    // EcfChunkHeader (24 bytes)
    writer.write_u64::<BigEndian>(chunk.id)?;
    writer.write_u32::<BigEndian>(chunk.offset)?;
    writer.write_u32::<BigEndian>(chunk.size)?;
    writer.write_u32::<BigEndian>(0)?; // adler32
    writer.write_u8(1)?; // flags: 1 = DeflateRaw compression
    writer.write_u8(4)?; // alignment_log2 = 4 (16 bytes)
    writer.write_u16::<BigEndian>(0x0000)?; // resource_flags

    // EraChunkExtra (32 bytes)
    let extra = EraChunkExtra::new(chunk.decomp_size, chunk.name_offset, chunk.comp_tiger128);
    extra.write(writer)?;

    Ok(())
}

/// Write chunk data with padding (handles both regular and pre-compressed files)
fn write_chunk_data_all<W: Write + Seek>(
    writer: &mut W,
    chunks: &[ChunkData],
    filename_table: &CompressedData,
    compressed_files: &[CompressedData],
    precompressed_files: &[PreCompressedFile],
    progress: &mut Option<&mut dyn FnMut(u64, u64) -> bool>,
) -> Result<()> {
    // Calculate total bytes to write for progress reporting
    let total_bytes: u64 = filename_table.data.len() as u64
        + compressed_files
            .iter()
            .map(|f| f.data.len() as u64)
            .sum::<u64>()
        + precompressed_files
            .iter()
            .map(|f| f.compressed_data.len() as u64)
            .sum::<u64>();
    let mut bytes_written: u64 = 0;

    // Write filename table (first chunk)
    write_padded(writer, chunks[0].offset as usize, &filename_table.data)?;
    bytes_written += filename_table.data.len() as u64;
    if let Some(cb) = &mut *progress
        && !cb(bytes_written, total_bytes)
    {
        return Err(crate::error::Error::Cancelled);
    }

    // Write regular compressed file data
    let regular_count = compressed_files.len();
    for (chunk, file) in chunks[1..=regular_count].iter().zip(compressed_files) {
        write_padded(writer, chunk.offset as usize, &file.data)?;
        bytes_written += file.data.len() as u64;
        if let Some(cb) = &mut *progress
            && !cb(bytes_written, total_bytes)
        {
            return Err(crate::error::Error::Cancelled);
        }
    }

    // Write pre-compressed file data
    for (chunk, file) in chunks[regular_count + 1..].iter().zip(precompressed_files) {
        write_padded(writer, chunk.offset as usize, &file.compressed_data)?;
        bytes_written += file.compressed_data.len() as u64;
        if let Some(cb) = &mut *progress
            && !cb(bytes_written, total_bytes)
        {
            return Err(crate::error::Error::Cancelled);
        }
    }

    Ok(())
}

/// Write data at offset, padding with zeros if needed
fn write_padded<W: Write + Seek>(writer: &mut W, offset: usize, data: &[u8]) -> Result<()> {
    let current = writer.stream_position()? as usize;
    if offset > current {
        writer.write_all(&vec![0u8; offset - current])?;
    }
    writer.write_all(data)?;
    Ok(())
}
