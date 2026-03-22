//! ERA archive writer.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use ecf::io::{Read, Seek, Write};
use tiger::{Digest, Tiger};

use crate::error::Result;
use crate::header::{EraArchiveHeader, EraChunkExtra};

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

/// Pre-computed archive layout (shared between finalize and write_to).
struct ComputedLayout {
    ecf_header: HeaderLayout,
    chunks: Vec<ChunkLayout>,
    compressed_names: CompressedData,
    compressed_files: Vec<CompressedData>,
    total_data_bytes: u64,
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

    /// Compress all pending files and compute the archive layout.
    fn compute_layout(&self) -> ComputedLayout {
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

        let filename_table_raw_len = filename_table.len();
        let compressed_names = compress_data(&filename_table);
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
            decomp_size: filename_table_raw_len as u32,
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

        let total_data_bytes: u64 = compressed_names.data.len() as u64
            + compressed_files
                .iter()
                .map(|f| f.data.len() as u64)
                .sum::<u64>()
            + self
                .precompressed
                .iter()
                .map(|f| f.compressed_data.len() as u64)
                .sum::<u64>();

        let ecf_header = HeaderLayout {
            header_size: total_header_size as u32,
            file_size: data_offset as u32,
            num_chunks: num_chunks as u16,
            id: ERA_FILE_ID,
            chunk_extra_data_size: 32,
        };

        ComputedLayout {
            ecf_header,
            chunks,
            compressed_names,
            compressed_files,
            total_data_bytes,
        }
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
        let layout = self.compute_layout();
        let adler32 = compute_header_adler32(&layout.ecf_header, &layout.chunks);

        let total_header_size = layout.ecf_header.header_size as usize;
        let chunk_header_size = ecf::EcfChunkHeader::SIZE + EraChunkExtra::SIZE;
        let data_offset = layout.ecf_header.file_size as usize;

        // Allocate output buffer
        let mut out = vec![0u8; data_offset];

        // Write ECF header (32 bytes)
        write_ecf_header(
            &mut out[..ecf::EcfHeader::SIZE],
            &layout.ecf_header,
            adler32,
        );

        // Write ERA archive header
        out[ecf::EcfHeader::SIZE..ecf::EcfHeader::SIZE + EraArchiveHeader::SIZE]
            .copy_from_slice(&EraArchiveHeader::new().to_bytes());

        // Write chunk headers
        let mut pos = total_header_size;
        for chunk in &layout.chunks {
            write_chunk_header(&mut out[pos..], chunk);
            pos += chunk_header_size;
        }

        // Write chunk data with progress
        let mut bytes_written: u64 = 0;

        // Filename table
        let off = layout.chunks[0].offset as usize;
        out[off..off + layout.compressed_names.data.len()]
            .copy_from_slice(&layout.compressed_names.data);
        bytes_written += layout.compressed_names.data.len() as u64;
        if let Some(cb) = &mut progress
            && !cb(bytes_written, layout.total_data_bytes)
        {
            return Err(crate::error::Error::Cancelled);
        }

        // Regular files
        let regular_count = layout.compressed_files.len();
        for (chunk, file) in layout.chunks[1..=regular_count]
            .iter()
            .zip(&layout.compressed_files)
        {
            let off = chunk.offset as usize;
            out[off..off + file.data.len()].copy_from_slice(&file.data);
            bytes_written += file.data.len() as u64;
            if let Some(cb) = &mut progress
                && !cb(bytes_written, layout.total_data_bytes)
            {
                return Err(crate::error::Error::Cancelled);
            }
        }

        // Pre-compressed files
        for (chunk, file) in layout.chunks[regular_count + 1..]
            .iter()
            .zip(&self.precompressed)
        {
            let off = chunk.offset as usize;
            out[off..off + file.compressed_data.len()].copy_from_slice(&file.compressed_data);
            bytes_written += file.compressed_data.len() as u64;
            if let Some(cb) = &mut progress
                && !cb(bytes_written, layout.total_data_bytes)
            {
                return Err(crate::error::Error::Cancelled);
            }
        }

        Ok(out)
    }

    /// Stream the archive to a writer.
    ///
    /// Unlike [`finalize`](Self::finalize) which materialises the entire archive
    /// in memory, this writes headers and chunk data sequentially to the
    /// provided writer. Ideal for piping directly through an encryption writer
    /// to disk.
    pub fn write_to(&self, writer: impl Write) -> Result<u64> {
        self.write_to_with_progress(writer, None)
    }

    /// Stream the archive to a writer with optional progress callback.
    ///
    /// The progress callback receives `(bytes_written, total_bytes)` and should
    /// return `true` to continue or `false` to cancel.
    pub fn write_to_with_progress(
        &self,
        mut writer: impl Write,
        mut progress: Option<&mut dyn FnMut(u64, u64) -> bool>,
    ) -> Result<u64> {
        let layout = self.compute_layout();

        // Write ECF header (32 bytes)
        let adler32 = compute_header_adler32(&layout.ecf_header, &layout.chunks);
        let mut hdr_buf = [0u8; ecf::EcfHeader::SIZE];
        write_ecf_header(&mut hdr_buf, &layout.ecf_header, adler32);
        writer
            .write_all(&hdr_buf)
            .map_err(|_| crate::error::Error::UnexpectedEof)?;

        // Write ERA archive header
        let archive_hdr = EraArchiveHeader::new().to_bytes();
        writer
            .write_all(&archive_hdr)
            .map_err(|_| crate::error::Error::UnexpectedEof)?;

        // Write chunk headers
        let chunk_header_size = ecf::EcfChunkHeader::SIZE + EraChunkExtra::SIZE;
        for chunk in &layout.chunks {
            let mut buf = [0u8; 56]; // 24 + 32
            write_chunk_header(&mut buf[..chunk_header_size], chunk);
            writer
                .write_all(&buf[..chunk_header_size])
                .map_err(|_| crate::error::Error::UnexpectedEof)?;
        }

        // Write alignment padding between headers and first chunk data
        let headers_end =
            layout.ecf_header.header_size as usize + chunk_header_size * layout.chunks.len();
        let first_data_offset = layout.chunks[0].offset as usize;
        if first_data_offset > headers_end {
            let pad = vec![0u8; first_data_offset - headers_end];
            writer
                .write_all(&pad)
                .map_err(|_| crate::error::Error::UnexpectedEof)?;
        }

        // Write chunk data sequentially with alignment padding
        let mut bytes_written: u64 = 0;
        let total_bytes = layout.total_data_bytes;

        // Filename table (chunk 0)
        writer
            .write_all(&layout.compressed_names.data)
            .map_err(|_| crate::error::Error::UnexpectedEof)?;
        bytes_written += layout.compressed_names.data.len() as u64;
        if let Some(cb) = &mut progress
            && !cb(bytes_written, total_bytes)
        {
            return Err(crate::error::Error::Cancelled);
        }

        // Regular files
        for (i, compressed) in layout.compressed_files.iter().enumerate() {
            // Alignment padding between previous chunk and this one
            let prev_end = layout.chunks[i].offset as usize + layout.chunks[i].size as usize;
            let next_start = layout.chunks[i + 1].offset as usize;
            if next_start > prev_end {
                let pad = vec![0u8; next_start - prev_end];
                writer
                    .write_all(&pad)
                    .map_err(|_| crate::error::Error::UnexpectedEof)?;
            }
            writer
                .write_all(&compressed.data)
                .map_err(|_| crate::error::Error::UnexpectedEof)?;
            bytes_written += compressed.data.len() as u64;
            if let Some(cb) = &mut progress
                && !cb(bytes_written, total_bytes)
            {
                return Err(crate::error::Error::Cancelled);
            }
        }

        // Pre-compressed files
        let regular_count = layout.compressed_files.len();
        for (i, file) in self.precompressed.iter().enumerate() {
            let chunk_idx = 1 + regular_count + i;
            let prev_idx = chunk_idx - 1;
            let prev_end =
                layout.chunks[prev_idx].offset as usize + layout.chunks[prev_idx].size as usize;
            let next_start = layout.chunks[chunk_idx].offset as usize;
            if next_start > prev_end {
                let pad = vec![0u8; next_start - prev_end];
                writer
                    .write_all(&pad)
                    .map_err(|_| crate::error::Error::UnexpectedEof)?;
            }
            writer
                .write_all(&file.compressed_data)
                .map_err(|_| crate::error::Error::UnexpectedEof)?;
            bytes_written += file.compressed_data.len() as u64;
            if let Some(cb) = &mut progress
                && !cb(bytes_written, total_bytes)
            {
                return Err(crate::error::Error::Cancelled);
            }
        }

        // Final padding to reach file_size
        let current_pos = layout.chunks.last().map_or(0, |c| c.offset + c.size) as usize;
        let file_size = layout.ecf_header.file_size as usize;
        if file_size > current_pos {
            let pad = vec![0u8; file_size - current_pos];
            writer
                .write_all(&pad)
                .map_err(|_| crate::error::Error::UnexpectedEof)?;
        }

        Ok(layout.ecf_header.file_size as u64)
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

impl Writer {
    /// Stream the archive through an [`EncryptWriter`](crate::EncryptWriter),
    /// encrypting on the fly.
    ///
    /// The destination must implement `Write + Seek + Read` (e.g. a `File`).
    /// Returns the inner writer after finishing encryption.
    pub fn write_to_encrypted<W: Write + Seek + Read>(
        &self,
        dest: W,
        keys: crate::TeaKeys,
    ) -> Result<W> {
        self.write_to_encrypted_with_progress(dest, keys, None)
    }

    /// Stream the archive through an [`EncryptWriter`](crate::EncryptWriter)
    /// with optional progress callback.
    pub fn write_to_encrypted_with_progress<W: Write + Seek + Read>(
        &self,
        dest: W,
        keys: crate::TeaKeys,
        progress: Option<&mut dyn FnMut(u64, u64) -> bool>,
    ) -> Result<W> {
        let mut encrypt = crate::EncryptWriter::new(dest, keys);
        self.write_to_with_progress(&mut encrypt, progress)?;
        encrypt
            .finish()
            .map_err(|_| crate::error::Error::UnexpectedEof)
    }
}
