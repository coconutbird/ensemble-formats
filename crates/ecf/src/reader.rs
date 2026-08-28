//! ECF container reader — zero-copy, operates on a borrowed byte slice.
//!
//! [`Reader`] parses the file and chunk headers up-front, then provides
//! indexed or ID-based access to chunk data. Compressed chunks (`BDeflateStream`)
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

/// Validation controls for parsing an ECF container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadOptions {
    /// Validate the game ECF magic (`0xDABA7737`).
    pub validate_magic: bool,
    /// Validate the header and chunk Adler-32 checksums.
    pub validate_checksums: bool,
}

impl ReadOptions {
    /// Strict validation matching the game loader.
    #[must_use]
    pub const fn strict() -> Self {
        Self {
            validate_magic: true,
            validate_checksums: true,
        }
    }

    /// Validate the magic and structure but skip checksums.
    #[must_use]
    pub const fn unchecked_checksums() -> Self {
        Self {
            validate_checksums: false,
            ..Self::strict()
        }
    }

    /// Accept a bad ECF magic while retaining checksum validation.
    #[must_use]
    pub const fn accepting_bad_magic() -> Self {
        Self {
            validate_magic: false,
            ..Self::strict()
        }
    }
}

impl Default for ReadOptions {
    fn default() -> Self {
        Self::strict()
    }
}

/// Zero-copy ECF reader backed by a byte slice.
pub struct Reader<'a> {
    data: &'a [u8],
    header: EcfHeader,
    chunks: Vec<EcfChunkHeader>,
}

impl<'a> Reader<'a> {
    /// Parse an ECF container from a byte slice, validating checksums.
    ///
    /// # Errors
    ///
    /// Returns an error if a header is truncated or invalid, a chunk lies
    /// outside `data`, or a header or chunk checksum does not match.
    pub fn new(data: &'a [u8]) -> Result<Self> {
        Self::new_with_options(data, ReadOptions::strict())
    }

    /// Parse an ECF container from a byte slice, skipping checksum validation.
    ///
    /// # Errors
    ///
    /// Returns an error if a header is truncated or invalid or a chunk lies
    /// outside `data`.
    pub fn new_unchecked(data: &'a [u8]) -> Result<Self> {
        Self::new_with_options(data, ReadOptions::unchecked_checksums())
    }

    /// Parse an ECF container with explicit validation controls.
    ///
    /// Structural bounds checks are always enabled. Disabling magic validation
    /// is intended for recovery and inspection; it does not make the data
    /// game-compatible.
    ///
    /// # Errors
    ///
    /// Returns an error if an enabled validation fails or any header or chunk
    /// range is malformed or truncated.
    pub fn new_with_options(data: &'a [u8], options: ReadOptions) -> Result<Self> {
        let header = EcfHeader::from_bytes_with_magic_validation(data, options.validate_magic)?;
        let header_size =
            usize::try_from(header.header_size).map_err(|_| Error::SizeOverflow("header size"))?;
        if header_size < EcfHeader::SIZE {
            return Err(Error::InvalidHeaderSize {
                minimum: EcfHeader::SIZE,
                actual: header_size,
            });
        }
        if header_size > data.len() {
            return Err(Error::UnexpectedEof);
        }

        // Validate header adler32 (bytes 12..header_size, matching HW2 ECF::validateHeader)
        if options.validate_checksums {
            let computed = adler32(&data[12..header_size]);
            if computed != header.adler32 {
                return Err(Error::HeaderChecksumMismatch {
                    expected: header.adler32,
                    computed,
                });
            }
        }

        // Chunk headers start right after the (possibly extended) ECF header
        let mut offset = header_size;
        let chunk_stride = EcfChunkHeader::SIZE
            .checked_add(usize::from(header.chunk_extra_data_size))
            .ok_or(Error::SizeOverflow("chunk header stride"))?;

        let mut chunks = Vec::with_capacity(usize::from(header.num_chunks));
        for i in 0..header.num_chunks {
            let fixed_header_end = offset
                .checked_add(EcfChunkHeader::SIZE)
                .ok_or(Error::SizeOverflow("chunk header range"))?;
            let chunk_header_end = offset
                .checked_add(chunk_stride)
                .ok_or(Error::SizeOverflow("chunk header range"))?;
            if fixed_header_end > data.len() || chunk_header_end > data.len() {
                return Err(Error::UnexpectedEof);
            }

            let chunk = EcfChunkHeader::from_bytes(&data[offset..])?;

            if chunk.size > 0 {
                let cstart = usize::try_from(chunk.offset)
                    .map_err(|_| Error::SizeOverflow("chunk offset"))?;
                let cend = cstart
                    .checked_add(
                        usize::try_from(chunk.size)
                            .map_err(|_| Error::SizeOverflow("chunk size"))?,
                    )
                    .ok_or(Error::SizeOverflow("chunk range"))?;

                if cend > data.len() {
                    return Err(Error::UnexpectedEof);
                }

                // Validate per-chunk adler32 (matching HW2 ECF::validateChunks).
                if options.validate_checksums {
                    let computed = adler32(&data[cstart..cend]);
                    if computed != chunk.adler32 {
                        return Err(Error::ChunkChecksumMismatch {
                            index: usize::from(i),
                            expected: chunk.adler32,
                            computed,
                        });
                    }
                }
            }

            chunks.push(chunk);
            offset = chunk_header_end;
        }

        Ok(Self {
            data,
            header,
            chunks,
        })
    }

    /// The parsed ECF header.
    #[must_use]
    pub fn header(&self) -> &EcfHeader {
        &self.header
    }

    /// The parsed chunk headers.
    #[must_use]
    pub fn chunks(&self) -> &[EcfChunkHeader] {
        &self.chunks
    }

    /// Find a chunk header by ID.
    #[must_use]
    pub fn find_chunk(&self, id: u64) -> Option<&EcfChunkHeader> {
        self.chunks.iter().find(|c| c.id == id)
    }

    /// Get raw (possibly compressed) chunk bytes by index.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ChunkNotFound`] when `index` is out of range, or
    /// [`Error::UnexpectedEof`] if the chunk range lies outside the container.
    pub fn raw_chunk_data(&self, index: usize) -> Result<&'a [u8]> {
        let chunk = self
            .chunks
            .get(index)
            .ok_or(Error::ChunkNotFound(index as u64))?;
        let start =
            usize::try_from(chunk.offset).map_err(|_| Error::SizeOverflow("chunk offset"))?;
        let end = start
            .checked_add(
                usize::try_from(chunk.size).map_err(|_| Error::SizeOverflow("chunk size"))?,
            )
            .ok_or(Error::SizeOverflow("chunk range"))?;
        if end > self.data.len() {
            return Err(Error::UnexpectedEof);
        }

        Ok(&self.data[start..end])
    }

    /// Get chunk data by index, automatically decompressing if needed.
    ///
    /// # Errors
    ///
    /// Returns an error when `index` is invalid, the chunk range is truncated,
    /// or a compressed chunk cannot be decompressed.
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
    ///
    /// # Errors
    ///
    /// Returns [`Error::ChunkNotFound`] if no chunk has `id`, or propagates an
    /// error while reading or decompressing the selected chunk.
    pub fn chunk_data_by_id(&self, id: u64) -> Result<Vec<u8>> {
        let index = self
            .chunks
            .iter()
            .position(|c| c.id == id)
            .ok_or(Error::ChunkNotFound(id))?;
        self.chunk_data(index)
    }

    /// The underlying byte slice.
    #[must_use]
    pub fn as_bytes(&self) -> &'a [u8] {
        self.data
    }
}
