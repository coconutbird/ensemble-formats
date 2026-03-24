//! Decrypting reader wrapper for encrypted ERA files.
//!
//! [`Reader`] is generic over [`ecf::io::Read`] + [`ecf::io::Seek`].
//! When the `std` feature is enabled, those traits *are* `std::io::Read` /
//! `std::io::Seek`, so a `File` or `BufReader` works directly.  In `no_std`
//! mode they are minimal replacements defined in `ecf::io`.

#[cfg(feature = "std")]
extern crate std;

use ecf::io::{IoError, Read, Seek, SeekFrom, invalid_seek};

use super::tea::{TEA_BLOCK_SIZE, TeaKeys, tea_decrypt_block64};

/// A reader that decrypts TEA-encrypted data on the fly.
///
/// Wraps any [`Read`] + [`Seek`] source and transparently decrypts
/// 64-byte TEA blocks in CTR mode as data is read.
///
/// ```ignore
/// use era::crypto::decrypt::Reader;
/// use era::TeaKeys;
/// let file = std::fs::File::open("archive.era")?;
/// let mut reader = Reader::new(file, TeaKeys::default_archive_keys());
/// ```
pub struct Reader<R> {
    inner: R,
    keys: TeaKeys,
    /// Current position in the decrypted stream.
    position: u64,
    /// Buffered decrypted block.
    buffer: [u8; TEA_BLOCK_SIZE],
    /// File offset of the start of the buffered block (aligned to TEA_BLOCK_SIZE).
    buffer_offset: u64,
    /// Whether the buffer is valid.
    buffer_valid: bool,
}

impl<R: Read + Seek> Reader<R> {
    /// Create a new decrypting reader.
    pub fn new(inner: R, keys: TeaKeys) -> Self {
        Self {
            inner,
            keys,
            position: 0,
            buffer: [0; TEA_BLOCK_SIZE],
            buffer_offset: u64::MAX,
            buffer_valid: false,
        }
    }

    /// Read and decrypt a block at the given aligned offset.
    fn read_block(&mut self, block_offset: u64) -> Result<(), IoError> {
        if self.buffer_valid && self.buffer_offset == block_offset {
            return Ok(());
        }

        self.inner.seek(SeekFrom::Start(block_offset))?;

        let mut encrypted = [0u8; TEA_BLOCK_SIZE];
        self.inner.read_exact(&mut encrypted)?;

        let counter = (block_offset / TEA_BLOCK_SIZE as u64) as u32;
        tea_decrypt_block64(&self.keys, &encrypted, &mut self.buffer, counter);

        self.buffer_offset = block_offset;
        self.buffer_valid = true;

        Ok(())
    }

    /// Consume this reader and return the underlying source.
    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: Read + Seek> Read for Reader<R> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        if buf.is_empty() {
            return Ok(0);
        }

        let mut total_read = 0;

        while total_read < buf.len() {
            let block_offset = (self.position / TEA_BLOCK_SIZE as u64) * TEA_BLOCK_SIZE as u64;
            let offset_in_block = (self.position % TEA_BLOCK_SIZE as u64) as usize;

            match self.read_block(block_offset) {
                Ok(()) => {}
                Err(ref e) if is_eof(e) => break,
                Err(e) => return Err(e),
            }

            let bytes_available = TEA_BLOCK_SIZE - offset_in_block;
            let bytes_to_copy = (buf.len() - total_read).min(bytes_available);

            buf[total_read..total_read + bytes_to_copy]
                .copy_from_slice(&self.buffer[offset_in_block..offset_in_block + bytes_to_copy]);

            self.position += bytes_to_copy as u64;
            total_read += bytes_to_copy;
        }

        Ok(total_read)
    }
}

impl<R: Read + Seek> Seek for Reader<R> {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64, IoError> {
        let new_pos = match pos {
            SeekFrom::Start(offset) => offset,
            SeekFrom::Current(offset) => if offset >= 0 {
                self.position.checked_add(offset as u64)
            } else {
                self.position.checked_sub((-offset) as u64)
            }
            .ok_or(invalid_seek())?,
            SeekFrom::End(offset) => {
                let end = self.inner.seek(SeekFrom::End(0))?;
                if offset >= 0 {
                    end.checked_add(offset as u64)
                } else {
                    end.checked_sub((-offset) as u64)
                }
                .ok_or(invalid_seek())?
            }
        };

        self.position = new_pos;
        Ok(new_pos)
    }
}

/// Check whether an IO error is an unexpected-EOF.
fn is_eof(e: &IoError) -> bool {
    #[cfg(feature = "std")]
    {
        e.kind() == std::io::ErrorKind::UnexpectedEof
    }
    #[cfg(not(feature = "std"))]
    {
        matches!(e, IoError::UnexpectedEof)
    }
}
