//! ECF container reader — zero-copy, operates on a borrowed byte slice.
//!
//! [`Reader`] parses the file and chunk headers up-front, then provides
//! indexed or ID-based access to chunk data. Compressed chunks (BDeflateStream)
//! are decompressed transparently by [`Reader::chunk_data`].
//!
//! ```ignore
//! let bytes = std::fs::read("model.ugx")?;
//! let ecf = ecf::Reader::new(&bytes)?;
//!
//! for (i, hdr) in ecf.chunks().iter().enumerate() {
//!     let data = ecf.chunk_data(i)?;
//!     println!("chunk {} — id 0x{:X}, {} bytes", i, hdr.id, data.len());
//! }
//! ```

use alloc::vec::Vec;

use crate::{EcfChunkHeader, EcfHeader, Error, Result, adler32, decompress, resource_flags};

/// Zero-copy ECF reader backed by a byte slice.
pub struct Reader<'a> {
    data: &'a [u8],
    header: EcfHeader,
    chunks: Vec<EcfChunkHeader>,
}

impl<'a> Reader<'a> {
    /// Parse an ECF container from a byte slice, validating checksums.
    pub fn new(data: &'a [u8]) -> Result<Self> {
        let header = EcfHeader::from_bytes(data)?;

        // Validate header adler32 (bytes 12..header_size, matching HW2 ECF::validateHeader)
        let hdr_end = (header.header_size as usize).min(data.len());
        if hdr_end > 12 {
            let computed = adler32(&data[12..hdr_end]);
            if computed != header.adler32 {
                return Err(Error::HeaderChecksumMismatch {
                    expected: header.adler32,
                    computed,
                });
            }
        }

        // Chunk headers start right after the (possibly extended) ECF header
        let mut offset = header.header_size as usize;
        let chunk_stride = EcfChunkHeader::SIZE + header.chunk_extra_data_size as usize;

        let mut chunks = Vec::with_capacity(header.num_chunks as usize);
        for i in 0..header.num_chunks {
            if offset + EcfChunkHeader::SIZE > data.len() {
                return Err(Error::UnexpectedEof);
            }

            let chunk = EcfChunkHeader::from_bytes(&data[offset..])?;

            // Validate per-chunk adler32 (matching HW2 ECF::validateChunks)
            if chunk.size > 0 {
                let cstart = chunk.offset as usize;
                let cend = cstart + chunk.size as usize;

                if cend > data.len() {
                    return Err(Error::UnexpectedEof);
                }

                let computed = adler32(&data[cstart..cend]);
                if computed != chunk.adler32 {
                    return Err(Error::ChunkChecksumMismatch {
                        index: i as usize,
                        expected: chunk.adler32,
                        computed,
                    });
                }
            }

            chunks.push(chunk);
            offset += chunk_stride;
        }

        Ok(Self {
            data,
            header,
            chunks,
        })
    }

    /// The parsed ECF header.
    pub fn header(&self) -> &EcfHeader {
        &self.header
    }

    /// The parsed chunk headers.
    pub fn chunks(&self) -> &[EcfChunkHeader] {
        &self.chunks
    }

    /// Find a chunk header by ID.
    pub fn find_chunk(&self, id: u64) -> Option<&EcfChunkHeader> {
        self.chunks.iter().find(|c| c.id == id)
    }

    /// Get raw (possibly compressed) chunk bytes by index.
    pub fn raw_chunk_data(&self, index: usize) -> Result<&'a [u8]> {
        let chunk = self
            .chunks
            .get(index)
            .ok_or(Error::ChunkNotFound(index as u64))?;
        let start = chunk.offset as usize;
        let end = start + chunk.size as usize;
        if end > self.data.len() {
            return Err(Error::UnexpectedEof);
        }

        Ok(&self.data[start..end])
    }

    /// Get chunk data by index, automatically decompressing if needed.
    pub fn chunk_data(&self, index: usize) -> Result<Vec<u8>> {
        let raw = self.raw_chunk_data(index)?;
        let chunk = &self.chunks[index];

        if (chunk.resource_flags & resource_flags::IS_DEFLATE_STREAM) != 0 {
            decompress(raw)
        } else {
            Ok(raw.to_vec())
        }
    }

    /// Get chunk data by ID, automatically decompressing if needed.
    pub fn chunk_data_by_id(&self, id: u64) -> Result<Vec<u8>> {
        let index = self
            .chunks
            .iter()
            .position(|c| c.id == id)
            .ok_or(Error::ChunkNotFound(id))?;
        self.chunk_data(index)
    }

    /// The underlying byte slice.
    pub fn as_bytes(&self) -> &'a [u8] {
        self.data
    }
}
