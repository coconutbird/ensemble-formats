//! ERA archive writer.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use ecf::io::{NoProgress, Progress, Read, Seek, Write};
use tiger::{Digest, Tiger};

use crate::error::{Error, Result};
use crate::header::{EraArchiveHeader, EraChunkExtra};

/// ERA file ID constant.
const ERA_FILE_ID: u32 = 0x17FD_BA9C;

/// ERA chunk ID (used for both filename table and file entries).
const ERA_CHUNK_ID: u64 = 0x8DAF_B100;

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
    /// Optional private key for signing the archive.
    signing_key: Option<crate::crypto::merkle::PrivateKey>,
}

/// Pre-computed archive layout (shared between finalize and `write_to`).
struct ComputedLayout {
    ecf_header: HeaderLayout,
    chunks: Vec<ChunkLayout>,
    compressed_names: CompressedData,
    compressed_files: Vec<CompressedData>,
    total_data_bytes: u64,
    /// Size of the signature block (0 if unsigned).
    signature_size: u32,
}

impl Writer {
    /// Create a new ERA writer.
    #[must_use]
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            precompressed: Vec::new(),
            signing_key: None,
        }
    }

    /// Set a private key for signing the archive.
    ///
    /// When set, the archive will include a Merkle signature in the header.
    pub fn set_signing_key(&mut self, key: crate::crypto::merkle::PrivateKey) {
        self.signing_key = Some(key);
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

    /// Build the null-terminated filename table and its encoded offsets.
    fn build_filename_table(&self) -> Result<(Vec<u8>, Vec<u32>, usize)> {
        let mut filename_table = Vec::new();
        let mut name_offsets = Vec::new();

        for file in &self.files {
            name_offsets.push(filename_offset(filename_table.len())?);
            filename_table.extend_from_slice(file.filename.as_bytes());
            filename_table.push(0);
        }

        let precompressed_start = name_offsets.len();
        for file in &self.precompressed {
            name_offsets.push(filename_offset(filename_table.len())?);
            filename_table.extend_from_slice(file.filename.as_bytes());
            filename_table.push(0);
        }

        Ok((filename_table, name_offsets, precompressed_start))
    }

    /// Compress all pending files and compute the archive layout.
    ///
    /// `signature_size` is the number of bytes reserved for the digital
    /// signature block between the ERA archive header and the chunk headers.
    fn compute_layout(&self, signature_size: u32) -> Result<ComputedLayout> {
        let (filename_table, name_offsets, precomp_name_start) = self.build_filename_table()?;
        let filename_table_raw_len = filename_table.len();
        let compressed_names = compress_data(&filename_table)?;
        let compressed_files: Vec<CompressedData> = self
            .files
            .iter()
            .map(|file| compress_data(&file.data))
            .collect::<Result<_>>()?;

        // Calculate layout
        let total_files = self
            .files
            .len()
            .checked_add(self.precompressed.len())
            .ok_or(Error::SizeOverflow("file count"))?;
        let num_chunks = total_files
            .checked_add(1)
            .ok_or(Error::SizeOverflow("chunk count"))?;
        let signature_size =
            usize::try_from(signature_size).map_err(|_| Error::SizeOverflow("signature size"))?;
        let total_header_size = ecf::EcfHeader::SIZE
            .checked_add(EraArchiveHeader::SIZE)
            .and_then(|size| size.checked_add(signature_size))
            .ok_or(Error::SizeOverflow("archive header"))?;
        let chunk_header_size = ecf::EcfChunkHeader::SIZE
            .checked_add(EraChunkExtra::SIZE)
            .ok_or(Error::SizeOverflow("chunk header"))?;
        let headers_size = chunk_header_size
            .checked_mul(num_chunks)
            .and_then(|size| size.checked_add(total_header_size))
            .ok_or(Error::SizeOverflow("archive headers"))?;

        let mut data_offset = align16(headers_size)?;
        let mut chunks = Vec::with_capacity(num_chunks);

        // Filename table chunk (index 0)
        chunks.push(ChunkLayout {
            id: ERA_CHUNK_ID,
            offset: format_u32(data_offset, "filename-table offset")?,
            size: format_u32(compressed_names.data.len(), "compressed filename table")?,
            decomp_size: format_u32(filename_table_raw_len, "filename table")?,
            name_offset: 0,
            comp_tiger128: compressed_names.tiger128,
        });
        data_offset = aligned_end(data_offset, compressed_names.data.len())?;

        // Regular file chunks
        for (i, (file, compressed)) in self.files.iter().zip(&compressed_files).enumerate() {
            chunks.push(ChunkLayout {
                id: ERA_CHUNK_ID,
                offset: format_u32(data_offset, "file offset")?,
                size: format_u32(compressed.data.len(), "compressed file")?,
                decomp_size: format_u32(file.data.len(), "file")?,
                name_offset: name_offsets[i],
                comp_tiger128: compressed.tiger128,
            });
            data_offset = aligned_end(data_offset, compressed.data.len())?;
        }

        // Pre-compressed file chunks
        for (i, file) in self.precompressed.iter().enumerate() {
            chunks.push(ChunkLayout {
                id: ERA_CHUNK_ID,
                offset: format_u32(data_offset, "file offset")?,
                size: format_u32(file.compressed_data.len(), "compressed file")?,
                decomp_size: file.decompressed_size,
                name_offset: name_offsets[precomp_name_start + i],
                comp_tiger128: file.tiger128,
            });
            data_offset = aligned_end(data_offset, file.compressed_data.len())?;
        }

        let total_data_bytes = core::iter::once(compressed_names.data.len())
            .chain(compressed_files.iter().map(|file| file.data.len()))
            .chain(
                self.precompressed
                    .iter()
                    .map(|file| file.compressed_data.len()),
            )
            .try_fold(0u64, |total, size| {
                let size = u64::try_from(size).map_err(|_| Error::SizeOverflow("archive data"))?;
                total
                    .checked_add(size)
                    .ok_or(Error::SizeOverflow("archive data"))
            })?;

        let ecf_header = HeaderLayout {
            header_size: format_u32(total_header_size, "header size")?,
            file_size: format_u32(data_offset, "archive size")?,
            num_chunks: u16::try_from(num_chunks)
                .map_err(|_| Error::SizeOverflow("chunk count"))?,
            id: ERA_FILE_ID,
            chunk_extra_data_size: 32,
        };

        Ok(ComputedLayout {
            ecf_header,
            chunks,
            compressed_names,
            compressed_files,
            total_data_bytes,
            signature_size: format_u32(signature_size, "signature size")?,
        })
    }

    /// Compute layout and (optionally) sign the archive.
    ///
    /// Iterates until the signature size stabilises: the signature size
    /// affects the header layout, which affects the header hash, which
    /// affects the signature size.
    ///
    /// The signature size can oscillate between two values when different
    /// header hashes lead to different Merkle tree traversals with different
    /// cache hit patterns. When oscillation is detected, we break the cycle
    /// by reserving the **maximum** observed size and zero-padding shorter
    /// signatures to fit.
    fn compute_layout_and_sign(&self) -> Result<(ComputedLayout, Option<Vec<u8>>)> {
        let Some(key) = &self.signing_key else {
            return Ok((self.compute_layout(0)?, None));
        };

        // Seed: sign a dummy hash to discover the initial signature size.
        let dummy_sig = crate::crypto::merkle::sign(key, &[0u8; 20])?;
        let mut sig_size = format_u32(dummy_sig.len(), "signature size")?;

        // Track all observed signature sizes to detect oscillation.
        let mut seen_sizes: Vec<u32> = Vec::new();

        // Iterate until stable (or oscillation detected).
        for _ in 0..16 {
            let layout = self.compute_layout(sig_size)?;
            let header_hash = layout_header_hash(&layout);
            let sig = crate::crypto::merkle::sign(key, &header_hash)?;
            let new_size = format_u32(sig.len(), "signature size")?;

            if new_size == sig_size {
                return Ok((layout, Some(sig)));
            }

            // Check for oscillation: have we seen this size before?
            if seen_sizes.contains(&new_size) {
                // Oscillation detected. Use the max of all observed sizes
                // so the signature always fits, and pad shorter sigs.
                let max_size = seen_sizes
                    .iter()
                    .copied()
                    .chain(core::iter::once(new_size))
                    .max()
                    .ok_or(Error::SizeOverflow("signature size"))?;
                let layout = self.compute_layout(max_size)?;
                let header_hash = layout_header_hash(&layout);
                let sig = crate::crypto::merkle::sign(key, &header_hash)?;

                // Pad to exactly max_size
                let mut padded = sig;
                padded.resize(
                    usize::try_from(max_size).map_err(|_| Error::SizeOverflow("signature size"))?,
                    0,
                );
                return Ok((layout, Some(padded)));
            }

            seen_sizes.push(sig_size);
            sig_size = new_size;
        }

        // Final fallback: use the max observed size and pad.
        let max_size = seen_sizes
            .iter()
            .copied()
            .chain(core::iter::once(sig_size))
            .max()
            .unwrap_or(sig_size);
        let layout = self.compute_layout(max_size)?;
        let header_hash = layout_header_hash(&layout);
        let sig = crate::crypto::merkle::sign(key, &header_hash)?;
        let mut padded = sig;
        padded.resize(
            usize::try_from(max_size).map_err(|_| Error::SizeOverflow("signature size"))?,
            0,
        );
        Ok((layout, Some(padded)))
    }

    /// Build the archive into a `Vec<u8>`.
    ///
    /// # Errors
    ///
    /// Returns an error if compression, layout, signing, or output sizing fails.
    pub fn finalize(&self) -> Result<Vec<u8>> {
        self.finalize_with_progress(&mut NoProgress)
    }

    /// Build the archive into a `Vec<u8>` with progress reporting.
    ///
    /// The [`Progress`] implementation receives `(bytes_written, total_bytes)`
    /// and should return `true` to continue or `false` to cancel.
    ///
    /// # Errors
    ///
    /// Returns an error if compression, layout, signing, or output sizing fails,
    /// or if the progress callback cancels the operation.
    pub fn finalize_with_progress(&self, progress: &mut impl Progress) -> Result<Vec<u8>> {
        let (layout, signature) = self.compute_layout_and_sign()?;

        let mut archive_hdr = EraArchiveHeader::new();
        archive_hdr.signature_size = layout.signature_size;
        let adler32 =
            compute_header_adler32(&layout.ecf_header, &archive_hdr, signature.as_deref());
        let chunk_header_size = ecf::EcfChunkHeader::SIZE + EraChunkExtra::SIZE;

        // Allocate output buffer
        let mut out = vec![0u8; layout.ecf_header.file_size as usize];

        // Write ECF header (32 bytes)
        write_ecf_header(
            &mut out[..ecf::EcfHeader::SIZE],
            &layout.ecf_header,
            adler32,
        );

        // Write ERA archive header (with signature_size)
        out[ecf::EcfHeader::SIZE..ecf::EcfHeader::SIZE + EraArchiveHeader::SIZE]
            .copy_from_slice(&archive_hdr.to_bytes());

        // Write signature block (if present)
        if let Some(sig) = &signature {
            let sig_offset = ecf::EcfHeader::SIZE + EraArchiveHeader::SIZE;
            out[sig_offset..sig_offset + sig.len()].copy_from_slice(sig);
        }

        // Write chunk headers
        let header_size = layout.ecf_header.header_size as usize;
        let mut pos = header_size;
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
        if !progress.report(bytes_written, layout.total_data_bytes) {
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
            if !progress.report(bytes_written, layout.total_data_bytes) {
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
            if !progress.report(bytes_written, layout.total_data_bytes) {
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
    ///
    /// # Errors
    ///
    /// Returns an error if archive construction or writing fails.
    pub fn write_to(&self, writer: impl Write) -> Result<u64> {
        self.write_to_with_progress(writer, &mut NoProgress)
    }

    /// Stream the archive to a writer with progress reporting.
    ///
    /// The [`Progress`] implementation receives `(bytes_written, total_bytes)`
    /// and should return `true` to continue or `false` to cancel.
    ///
    /// # Errors
    ///
    /// Returns an error if archive construction or writing fails, or if the
    /// progress callback cancels the operation.
    pub fn write_to_with_progress(
        &self,
        mut writer: impl Write,
        progress: &mut impl Progress,
    ) -> Result<u64> {
        let (layout, signature) = self.compute_layout_and_sign()?;

        // Compute checksum over bytes 12..header_size (ECF tail + archive header + signature)
        let mut archive_hdr = EraArchiveHeader::new();
        archive_hdr.signature_size = layout.signature_size;
        let adler32 =
            compute_header_adler32(&layout.ecf_header, &archive_hdr, signature.as_deref());

        // Write ECF header (32 bytes)
        let mut hdr_buf = [0u8; ecf::EcfHeader::SIZE];
        write_ecf_header(&mut hdr_buf, &layout.ecf_header, adler32);
        writer
            .write_all(&hdr_buf)
            .map_err(|_| crate::error::Error::UnexpectedEof)?;

        // Write ERA archive header
        writer
            .write_all(&archive_hdr.to_bytes())
            .map_err(|_| crate::error::Error::UnexpectedEof)?;

        // Write signature block (if present)
        if let Some(sig) = &signature {
            writer
                .write_all(sig)
                .map_err(|_| crate::error::Error::UnexpectedEof)?;
        }

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
        if !progress.report(bytes_written, total_bytes) {
            return Err(crate::error::Error::Cancelled);
        }

        // Regular files
        for (i, compressed) in layout.compressed_files.iter().enumerate() {
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
            if !progress.report(bytes_written, total_bytes) {
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
            if !progress.report(bytes_written, total_bytes) {
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

        Ok(u64::from(layout.ecf_header.file_size))
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
fn align16(value: usize) -> Result<usize> {
    value
        .checked_add(15)
        .map(|aligned| aligned & !15)
        .ok_or(Error::SizeOverflow("aligned archive offset"))
}

fn aligned_end(offset: usize, size: usize) -> Result<usize> {
    let end = offset
        .checked_add(size)
        .ok_or(Error::SizeOverflow("archive data offset"))?;
    align16(end)
}

fn format_u32(value: usize, field: &'static str) -> Result<u32> {
    u32::try_from(value).map_err(|_| Error::SizeOverflow(field))
}

fn filename_offset(value: usize) -> Result<u32> {
    let offset = format_u32(value, "filename-table offset")?;
    if offset > 0x00FF_FFFF {
        return Err(Error::SizeOverflow("24-bit filename-table offset"));
    }
    Ok(offset)
}

/// Compress data and compute its Tiger128 hash.
fn compress_data(data: &[u8]) -> Result<CompressedData> {
    let decompressed_size = format_u32(data.len(), "decompressed file")?;
    let compressed = miniz_oxide::deflate::compress_to_vec(data, 6);

    let hash = Tiger::digest(&compressed);
    let mut tiger128 = [0u8; 16];
    tiger128.copy_from_slice(&hash[..16]);

    Ok(CompressedData {
        data: compressed,
        tiger128,
        decompressed_size,
    })
}

/// Compress data and compute its Tiger128 hash (public API).
///
/// # Errors
///
/// Returns an error if `data` is larger than the ERA format's `u32` size field.
pub fn compress_file_data(data: &[u8]) -> Result<CompressedData> {
    compress_data(data)
}

/// Compute adler32 over header bytes `12..header_size`.
///
/// The engine's `ECF_ValidateHeader` checksums `data[12..header_size]`,
/// which for ERA covers:
///   - bytes 12..32: ECF header tail (`file_size`, `num_chunks`, flags, id, `chunk_extra`, pad)
///   - bytes 32..48: ERA archive header (`archive_magic`, `signature_size`, reserved)
///   - bytes 48..48+sig: signature block (if present)
///
/// Chunk headers start *after* `header_size` and are NOT included.
fn compute_header_adler32(
    header: &HeaderLayout,
    archive_hdr: &EraArchiveHeader,
    signature: Option<&[u8]>,
) -> u32 {
    let mut data = Vec::new();

    // ECF header tail: bytes 12..32 (20 bytes)
    data.extend_from_slice(&header.file_size.to_be_bytes());
    data.extend_from_slice(&header.num_chunks.to_be_bytes());
    data.extend_from_slice(&0u16.to_be_bytes()); // flags
    data.extend_from_slice(&header.id.to_be_bytes());
    data.extend_from_slice(&header.chunk_extra_data_size.to_be_bytes());
    data.extend_from_slice(&0u16.to_be_bytes()); // pad0
    data.extend_from_slice(&0u32.to_be_bytes()); // pad1

    // ERA archive header: bytes 32..48 (16 bytes)
    data.extend_from_slice(&archive_hdr.to_bytes());

    // Signature block: bytes 48..48+sig_size (if present)
    if let Some(sig) = signature {
        data.extend_from_slice(sig);
    }

    ecf::adler32(&data)
}

/// Write ECF header into a buffer (must be at least 32 bytes).
fn write_ecf_header(buf: &mut [u8], header: &HeaderLayout, adler32: u32) {
    buf[0..4].copy_from_slice(&0xDABA_7737_u32.to_be_bytes());
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

/// Build the raw chunk header bytes (big-endian, as on disk) for signature hashing.
fn build_chunk_headers_raw(chunks: &[ChunkLayout]) -> Vec<u8> {
    let stride = ecf::EcfChunkHeader::SIZE + EraChunkExtra::SIZE;
    let mut buf = vec![0u8; stride * chunks.len()];
    for (i, chunk) in chunks.iter().enumerate() {
        write_chunk_header(&mut buf[i * stride..], chunk);
    }
    buf
}

/// Compute the header hash from a computed layout (same hash the reader produces).
fn layout_header_hash(layout: &ComputedLayout) -> [u8; 20] {
    let raw = build_chunk_headers_raw(&layout.chunks);
    crate::crypto::merkle::compute_header_hash(
        layout.ecf_header.header_size,
        layout.ecf_header.num_chunks,
        layout.ecf_header.chunk_extra_data_size,
        layout.ecf_header.file_size,
        &raw,
    )
}

impl Writer {
    /// Stream the archive through a [`crate::crypto::encrypt::Writer`], encrypting
    /// on the fly.
    ///
    /// The destination must implement `Write + Seek + Read` (e.g. a `File`).
    /// Returns the inner writer after finishing encryption.
    ///
    /// # Errors
    ///
    /// Returns an error if archive construction, encryption, or writing fails.
    pub fn write_to_encrypted<W: Write + Seek + Read>(
        &self,
        dest: W,
        keys: crate::TeaKeys,
    ) -> Result<W> {
        self.write_to_encrypted_with_progress(dest, keys, &mut NoProgress)
    }

    /// Stream the archive through a [`crate::crypto::encrypt::Writer`] with progress
    /// reporting.
    ///
    /// # Errors
    ///
    /// Returns an error if archive construction, encryption, or writing fails,
    /// or if the progress callback cancels the operation.
    pub fn write_to_encrypted_with_progress<W: Write + Seek + Read>(
        &self,
        dest: W,
        keys: crate::TeaKeys,
        progress: &mut impl Progress,
    ) -> Result<W> {
        let mut encrypt = crate::crypto::encrypt::Writer::new(dest, keys);
        self.write_to_with_progress(&mut encrypt, progress)?;
        encrypt
            .finish()
            .map_err(|_| crate::error::Error::UnexpectedEof)
    }
}
