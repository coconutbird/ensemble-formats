//! ECF container writer — assembles chunks into a `Vec<u8>`.
//!
//! [`Writer`] collects chunk data (optionally compressing it with
//! `BDeflateStream`), then [`Writer::finalize`] lays out headers and data
//! with proper alignment and checksums.
//!
//! ```ignore
//! let mut ecf = ecf::Writer::new(0xAAC93746);
//! ecf.add_chunk(0x700, mesh_data);
//! ecf.add_chunk_compressed(0x701, vertex_data)?;
//! let bytes: Vec<u8> = ecf.finalize()?;
//! std::fs::write("out.ugx", &bytes)?;
//! ```

use alloc::{vec, vec::Vec};

use crate::{
    EcfChunkHeader, EcfHeader, Error, HEADER_MAGIC, Result, adler32, compress, resource_flags,
};

/// Default alignment for chunks (16-byte, log2 = 4).
pub const DEFAULT_ALIGNMENT_LOG2: u8 = 4;

/// In-memory ECF builder that produces a `Vec<u8>`.
pub struct Writer {
    header: EcfHeader,
    chunks: Vec<(EcfChunkHeader, Vec<u8>)>,
    default_alignment_log2: u8,
}

impl Writer {
    /// Create a new ECF writer with default 16-byte alignment.
    #[must_use]
    pub fn new(file_id: u32) -> Self {
        Self::with_alignment(file_id, DEFAULT_ALIGNMENT_LOG2)
    }

    /// Create a new ECF writer with specified alignment (log2).
    #[must_use]
    pub fn with_alignment(file_id: u32, alignment_log2: u8) -> Self {
        Self {
            header: EcfHeader {
                magic: HEADER_MAGIC,
                header_size: 32,
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

    /// Set the ECF header flags written to the container.
    pub fn set_header_flags(&mut self, flags: u16) {
        self.header.flags = flags;
    }

    /// Add a logical chunk while preserving its ECF metadata.
    ///
    /// When [`resource_flags::IS_DEFLATE_STREAM`] is set, `data` is treated as
    /// the decompressed logical payload and is wrapped in a game-compatible
    /// `BDeflateStream` before being stored.
    ///
    /// # Errors
    ///
    /// Returns an error if compression fails or the stored size cannot be
    /// represented by the ECF format.
    pub fn add_chunk_with_metadata(
        &mut self,
        id: u64,
        data: Vec<u8>,
        alignment_log2: u8,
        flags: u8,
        resource_flags: u16,
    ) -> Result<()> {
        let stored = if (resource_flags & resource_flags::IS_DEFLATE_STREAM) != 0 {
            compress(&data, true)?
        } else {
            data
        };
        let chunk = EcfChunkHeader {
            id,
            offset: 0,
            size: u32::try_from(stored.len()).map_err(|_| Error::SizeOverflow("chunk size"))?,
            adler32: adler32(&stored),
            flags,
            alignment_log2,
            resource_flags,
        };
        self.chunks.push((chunk, stored));
        Ok(())
    }

    /// Add an uncompressed chunk.
    pub fn add_chunk(&mut self, id: u64, data: Vec<u8>) {
        self.add_chunk_with_alignment(id, data, self.default_alignment_log2);
    }

    /// Add an uncompressed chunk with specific alignment.
    pub fn add_chunk_with_alignment(&mut self, id: u64, data: Vec<u8>, alignment_log2: u8) {
        self.add_chunk_full(id, data, alignment_log2, 0);
    }

    /// Add an uncompressed chunk with specific alignment and resource flags.
    ///
    /// # Panics
    ///
    /// Panics if `data` is larger than the format's 32-bit chunk-size field.
    pub fn add_chunk_full(
        &mut self,
        id: u64,
        data: Vec<u8>,
        alignment_log2: u8,
        resource_flags: u16,
    ) {
        let chunk = EcfChunkHeader {
            id,
            offset: 0,
            size: u32::try_from(data.len()).expect("ECF chunks cannot exceed u32::MAX bytes"),
            adler32: adler32(&data),
            flags: 0,
            alignment_log2,
            resource_flags,
        };
        self.chunks.push((chunk, data));
    }

    /// Add a BDeflateStream-compressed chunk.
    ///
    /// `BDeflateStream` headers are **always big-endian** on disk — both HW1
    /// and HW2 use a big-endian stream reader that byte-swaps u32/u64 fields
    /// unconditionally. The payload inside (e.g. BDT data) keeps whatever
    /// endianness the caller wrote it in.
    ///
    /// # Errors
    ///
    /// Returns an error if compression fails or the compressed chunk is too
    /// large for the ECF on-disk size field.
    pub fn add_chunk_compressed(&mut self, id: u64, data: &[u8]) -> Result<()> {
        self.add_chunk_compressed_with_alignment(id, data, self.default_alignment_log2)
    }

    /// Add a compressed chunk with specific alignment.
    ///
    /// # Errors
    ///
    /// Returns an error if compression fails or the compressed chunk is too
    /// large for the ECF on-disk size field.
    pub fn add_chunk_compressed_with_alignment(
        &mut self,
        id: u64,
        data: &[u8],
        alignment_log2: u8,
    ) -> Result<()> {
        // BDeflateStream is always big-endian on disk.
        let wrapped = compress(data, true)?;
        let chunk = EcfChunkHeader {
            id,
            offset: 0,
            size: u32::try_from(wrapped.len())
                .map_err(|_| Error::SizeOverflow("compressed chunk size"))?,
            adler32: adler32(&wrapped),
            flags: 0,
            alignment_log2,
            resource_flags: resource_flags::IS_DEFLATE_STREAM,
        };
        self.chunks.push((chunk, wrapped));
        Ok(())
    }

    /// Finalize the ECF container and return the serialized bytes.
    ///
    /// # Errors
    ///
    /// Returns [`Error::SizeOverflow`] if the chunk count, a chunk offset, or
    /// the final container size cannot be represented by the ECF format.
    pub fn finalize(mut self) -> Result<Vec<u8>> {
        self.header.num_chunks =
            u16::try_from(self.chunks.len()).map_err(|_| Error::SizeOverflow("chunk count"))?;

        let headers_size = EcfChunkHeader::SIZE
            .checked_mul(self.chunks.len())
            .and_then(|size| size.checked_add(EcfHeader::SIZE))
            .ok_or(Error::SizeOverflow("chunk header table"))?;

        let initial_alignment = self
            .chunks
            .first()
            .map_or(Ok(16), |(chunk, _)| alignment(chunk.alignment_log2))?;
        let mut data_offset = checked_align_up(headers_size, initial_alignment)?;

        for (chunk, data) in &mut self.chunks {
            chunk.offset =
                u32::try_from(data_offset).map_err(|_| Error::SizeOverflow("chunk offset"))?;
            let chunk_end = data_offset
                .checked_add(data.len())
                .ok_or(Error::SizeOverflow("chunk range"))?;
            data_offset = checked_align_up(chunk_end, alignment(chunk.alignment_log2)?)?;
        }

        self.header.file_size =
            u32::try_from(data_offset).map_err(|_| Error::SizeOverflow("file size"))?;

        // Compute adler32 over header bytes 12..32 only.
        //
        // The engine's ECF_ReadAndValidateStream reads `header_size` bytes
        // (= 32), then checksums bytes 12..header_size. Chunk headers are
        // read separately from the stream and are NOT part of the checksum.
        let header_bytes = self.header.to_bytes();
        self.header.adler32 = adler32(&header_bytes[12..32]);

        // Assemble output
        let mut out = vec![0u8; data_offset];
        out[..32].copy_from_slice(&self.header.to_bytes());

        let mut pos = EcfHeader::SIZE;
        for (chunk, _) in &self.chunks {
            out[pos..pos + EcfChunkHeader::SIZE].copy_from_slice(&chunk.to_bytes());
            pos += EcfChunkHeader::SIZE;
        }

        for (chunk, data) in &self.chunks {
            let start =
                usize::try_from(chunk.offset).map_err(|_| Error::SizeOverflow("chunk offset"))?;
            out[start..start + data.len()].copy_from_slice(data);
        }

        Ok(out)
    }
}

fn alignment(log2: u8) -> Result<usize> {
    1usize
        .checked_shl(u32::from(log2))
        .ok_or(Error::InvalidAlignment(log2))
}

fn checked_align_up(value: usize, alignment: usize) -> Result<usize> {
    value
        .checked_add(alignment - 1)
        .map(|aligned| aligned & !(alignment - 1))
        .ok_or(Error::SizeOverflow("aligned offset"))
}
