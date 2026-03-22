//! ECF container writer — assembles chunks into a `Vec<u8>`.
//!
//! [`EcfWriter`] collects chunk data (optionally compressing it with
//! BDeflateStream), then [`EcfWriter::finalize`] lays out headers and data
//! with proper alignment and checksums.
//!
//! ```ignore
//! let mut ecf = ecf::EcfWriter::new(0xAAC93746);
//! ecf.add_chunk(0x700, mesh_data);
//! ecf.add_chunk_compressed(0x701, vertex_data)?;
//! let bytes: Vec<u8> = ecf.finalize()?;
//! std::fs::write("out.ugx", &bytes)?;
//! ```

use alloc::{vec, vec::Vec};

use crate::{
    EcfChunkHeader, EcfHeader, HEADER_MAGIC, Result, adler32, align_up, compress, resource_flags,
};

/// Default alignment for chunks (16-byte, log2 = 4).
pub const DEFAULT_ALIGNMENT_LOG2: u8 = 4;

/// In-memory ECF builder that produces a `Vec<u8>`.
pub struct EcfWriter {
    header: EcfHeader,
    chunks: Vec<(EcfChunkHeader, Vec<u8>)>,
    default_alignment_log2: u8,
}

impl EcfWriter {
    /// Create a new ECF writer with default 16-byte alignment.
    pub fn new(file_id: u32) -> Self {
        Self::with_alignment(file_id, DEFAULT_ALIGNMENT_LOG2)
    }

    /// Create a new ECF writer with specified alignment (log2).
    pub fn with_alignment(file_id: u32, alignment_log2: u8) -> Self {
        Self {
            header: EcfHeader {
                magic: HEADER_MAGIC,
                header_size: EcfHeader::SIZE as u32,
                adler32: 0,
                file_size: 0,
                num_chunks: 0,
                flags: 0,
                id: file_id,
                chunk_extra_data_size: 0,
            },
            chunks: Vec::new(),
            default_alignment_log2: alignment_log2,
        }
    }

    /// Add an uncompressed chunk.
    pub fn add_chunk(&mut self, id: u64, data: Vec<u8>) {
        self.add_chunk_with_alignment(id, data, self.default_alignment_log2);
    }

    /// Add an uncompressed chunk with specific alignment.
    pub fn add_chunk_with_alignment(&mut self, id: u64, data: Vec<u8>, alignment_log2: u8) {
        let chunk = EcfChunkHeader {
            id,
            offset: 0,
            size: data.len() as u32,
            adler32: adler32(&data),
            flags: 0,
            alignment_log2,
            resource_flags: 0,
        };
        self.chunks.push((chunk, data));
    }

    /// Add a BDeflateStream-compressed chunk (little-endian / PC).
    pub fn add_chunk_compressed(&mut self, id: u64, data: Vec<u8>) -> Result<()> {
        self.add_chunk_compressed_with_options(id, data, false, self.default_alignment_log2)
    }

    /// Add a BDeflateStream-compressed chunk (big-endian / Xbox 360).
    pub fn add_chunk_compressed_be(&mut self, id: u64, data: Vec<u8>) -> Result<()> {
        self.add_chunk_compressed_with_options(id, data, true, self.default_alignment_log2)
    }

    /// Add a compressed chunk with explicit endianness and alignment.
    pub fn add_chunk_compressed_with_options(
        &mut self,
        id: u64,
        data: Vec<u8>,
        big_endian: bool,
        alignment_log2: u8,
    ) -> Result<()> {
        let wrapped = compress(&data, big_endian)?;
        let chunk = EcfChunkHeader {
            id,
            offset: 0,
            size: wrapped.len() as u32,
            adler32: adler32(&wrapped),
            flags: 0,
            alignment_log2,
            resource_flags: resource_flags::IS_DEFLATE_STREAM,
        };
        self.chunks.push((chunk, wrapped));
        Ok(())
    }

    /// Finalize the ECF container and return the serialized bytes.
    pub fn finalize(mut self) -> Result<Vec<u8>> {
        self.header.num_chunks = self.chunks.len() as u16;

        let headers_size = EcfHeader::SIZE + (EcfChunkHeader::SIZE * self.chunks.len());

        let initial_alignment = self
            .chunks
            .first()
            .map(|(c, _)| c.alignment())
            .unwrap_or(16);
        let mut data_offset = align_up(headers_size, initial_alignment);

        for (chunk, data) in &mut self.chunks {
            chunk.offset = data_offset as u32;
            data_offset = align_up(data_offset + data.len(), chunk.alignment());
        }

        self.header.file_size = data_offset as u32;

        // Compute adler32 over header bytes 12..32 + all chunk headers
        let header_bytes = self.header.to_bytes();
        let mut checksum_data = Vec::with_capacity(20 + self.chunks.len() * EcfChunkHeader::SIZE);
        checksum_data.extend_from_slice(&header_bytes[12..32]);
        for (chunk, _) in &self.chunks {
            checksum_data.extend_from_slice(&chunk.to_bytes());
        }
        self.header.adler32 = adler32(&checksum_data);

        // Assemble output
        let mut out = vec![0u8; data_offset];
        out[..32].copy_from_slice(&self.header.to_bytes());

        let mut pos = EcfHeader::SIZE;
        for (chunk, _) in &self.chunks {
            out[pos..pos + EcfChunkHeader::SIZE].copy_from_slice(&chunk.to_bytes());
            pos += EcfChunkHeader::SIZE;
        }

        for (chunk, data) in &self.chunks {
            let start = chunk.offset as usize;
            out[start..start + data.len()].copy_from_slice(data);
        }

        Ok(out)
    }
}
