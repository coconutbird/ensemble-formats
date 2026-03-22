//! ERA archive format for Halo Wars Definitive Edition.
//!
//! ERA files are ECF-based archives containing compressed game assets.
//! Files are encrypted using TEA cipher in CTR mode with 64-byte blocks.
//!
//! # Reading ERA archives
//!
//! ```ignore
//! let reader = era::Reader::new(decrypted_bytes).unwrap();
//! for entry in reader.iter() {
//!     println!("{}", entry.filename.as_deref().unwrap_or("<unnamed>"));
//! }
//! ```
//!
//! # Writing ERA archives
//!
//! ```ignore
//! let mut writer = era::Writer::new();
//! writer.add_file("data/test.txt", b"Hello, world!".to_vec());
//! let archive_bytes = writer.finalize().unwrap();
//! ```

#![no_std]
extern crate alloc;

pub mod buffer_pool;
pub mod crypto;
mod error;
mod header;
mod reader;
mod writer;

#[cfg(feature = "std")]
mod decrypt_reader;
#[cfg(feature = "std")]
mod encrypt_writer;

pub use buffer_pool::{BufferPool, PooledBuffer};
pub use crypto::{ARCHIVE_PASSWORD, TEA_BLOCK_SIZE, TeaKeys};
#[cfg(feature = "rayon")]
pub use crypto::{tea_decrypt_data_parallel, tea_encrypt_data_parallel};
#[cfg(feature = "std")]
pub use decrypt_reader::DecryptReader;
#[cfg(feature = "std")]
pub use encrypt_writer::EncryptWriter;
pub use error::*;
pub use header::*;
pub use reader::*;
pub use writer::{CompressedData, Writer, compress_file_data};

#[cfg(test)]
mod tests {
    extern crate alloc;
    extern crate std;

    use alloc::vec;
    use alloc::vec::Vec;

    use super::*;

    #[test]
    fn test_era_roundtrip_single_file() {
        let mut writer = Writer::new();
        writer.add_file("test/hello.txt", b"Hello, World!".to_vec());

        let data = writer.finalize().expect("Failed to write");
        let archive = Reader::new(&data).expect("Failed to read");

        assert_eq!(archive.len(), 2); // filename chunk + 1 file
        let entry = archive.entry(1).unwrap();
        assert_eq!(entry.filename.as_deref(), Some("test\\hello.txt"));

        let content = archive.read_entry(1).expect("Failed to read entry");
        assert_eq!(content, b"Hello, World!");
    }

    #[test]
    fn test_era_roundtrip_multiple_files() {
        let mut writer = Writer::new();
        writer.add_file("test/hello.txt", b"Hello, World!".to_vec());
        writer.add_file("data/numbers.bin", vec![1, 2, 3, 4, 5, 6, 7, 8]);
        writer.add_file("empty.txt", vec![]);

        let data = writer.finalize().expect("Failed to write");
        let archive = Reader::new(&data).expect("Failed to read");

        assert_eq!(archive.len(), 4); // filename chunk + 3 files

        let content1 = archive.read_entry(1).expect("Failed to read entry 1");
        assert_eq!(content1, b"Hello, World!");

        let content2 = archive.read_entry(2).expect("Failed to read entry 2");
        assert_eq!(content2, vec![1, 2, 3, 4, 5, 6, 7, 8]);

        let content3 = archive.read_entry(3).expect("Failed to read entry 3");
        assert_eq!(content3, vec![]);
    }

    #[test]
    fn test_era_roundtrip_identical_bytes() {
        let mut writer = Writer::new();
        writer.add_file("test/hello.txt", b"Hello, World!".to_vec());
        writer.add_file("data/numbers.bin", vec![1, 2, 3, 4, 5, 6, 7, 8]);

        // First write
        let data1 = writer.finalize().expect("Failed to write");

        // Read back and collect hashes
        let archive = Reader::new(&data1).expect("Failed to read");
        let hashes1: Vec<_> = archive.iter().map(|e| e.extra.comp_tiger128).collect();

        // Create new writer from extracted files
        let mut writer2 = Writer::new();
        for i in 1..archive.len() {
            let entry = archive.entry(i).unwrap();
            let filename = entry.filename.as_ref().unwrap();
            let content = archive.read_entry(i).expect("Failed to read entry");
            writer2.add_file(filename, content);
        }

        // Second write
        let data2 = writer2.finalize().expect("Failed to write second time");

        // Read second archive and collect hashes
        let archive2 = Reader::new(&data2).expect("Failed to read second");
        let hashes2: Vec<_> = archive2.iter().map(|e| e.extra.comp_tiger128).collect();

        // Verify identical bytes and hashes
        assert_eq!(data1, data2, "Round-trip should produce identical bytes");
        assert_eq!(
            hashes1, hashes2,
            "Tiger128 hashes should match after roundtrip"
        );
    }

    #[test]
    fn test_era_large_file() {
        let large_data: Vec<u8> = (0..1000).map(|i| (i % 256) as u8).collect();

        let mut writer = Writer::new();
        writer.add_file("large.bin", large_data.clone());

        let data = writer.finalize().expect("Failed to write");
        let archive = Reader::new(&data).expect("Failed to read");

        let content = archive.read_entry(1).expect("Failed to read entry");
        assert_eq!(content, large_data);
    }

    #[test]
    fn test_tea_encrypt_decrypt_roundtrip() {
        use crate::crypto::{tea_decrypt_block64, tea_encrypt_block64};

        let keys = TeaKeys::default_archive_keys();
        let original: [u8; 64] = core::array::from_fn(|i| i as u8);
        let mut encrypted = [0u8; 64];
        let mut decrypted = [0u8; 64];

        tea_encrypt_block64(&keys, &original, &mut encrypted, 0);
        tea_decrypt_block64(&keys, &encrypted, &mut decrypted, 0);

        assert_eq!(original, decrypted);
    }

    #[cfg(feature = "rayon")]
    #[test]
    fn test_parallel_encryption_roundtrip() {
        use crate::crypto::{
            tea_decrypt_data_parallel, tea_encrypt_data, tea_encrypt_data_parallel,
        };

        let keys = TeaKeys::default_archive_keys();
        let original: Vec<u8> = (0..640).map(|i| (i % 256) as u8).collect();

        let mut data1 = original.clone();
        tea_encrypt_data_parallel(&keys, &mut data1, 0);
        tea_decrypt_data_parallel(&keys, &mut data1, 0);
        assert_eq!(original, data1);

        let mut data2 = original.clone();
        tea_encrypt_data(&keys, &mut data2, 0);
        tea_decrypt_data_parallel(&keys, &mut data2, 0);
        assert_eq!(original, data2);
    }

    #[test]
    fn test_precompressed_file() {
        let mut writer = Writer::new();
        writer.add_file("regular.txt", b"Hello, World!".to_vec());

        let compressed = compress_file_data(b"Pre-compressed data");
        writer.add_compressed_file(
            "precompressed.txt",
            compressed.data.clone(),
            compressed.decompressed_size,
            compressed.tiger128,
        );

        let data = writer.finalize().expect("Failed to write");
        let archive = Reader::new(&data).expect("Failed to read");

        assert_eq!(archive.len(), 3);

        let content1 = archive.read_entry(1).expect("Failed to read entry 1");
        assert_eq!(content1, b"Hello, World!");

        let content2 = archive.read_entry(2).expect("Failed to read entry 2");
        assert_eq!(content2, b"Pre-compressed data");
    }

    #[test]
    fn test_read_entry_compressed() {
        let mut writer = Writer::new();
        writer.add_file("test.txt", b"Test content for compression".to_vec());

        let data = writer.finalize().expect("Failed to write");
        let archive = Reader::new(&data).expect("Failed to read");

        let (compressed, decomp_size, tiger128) =
            archive.read_entry_compressed(1).expect("Failed to read");

        assert_eq!(decomp_size, 28);
        assert!(!compressed.is_empty());
        assert_ne!(tiger128, [0u8; 16]);

        let decompressed = archive.read_entry(1).expect("Failed to read");
        assert_eq!(decompressed, b"Test content for compression");
    }

    #[test]
    fn test_write_with_progress() {
        let mut writer = Writer::new();
        writer.add_file("test/file1.txt", b"Hello, World!".to_vec());
        writer.add_file("test/file2.txt", b"Second file content".to_vec());
        writer.add_file("test/file3.txt", b"Third file with more data here".to_vec());

        let mut progress_calls = Vec::new();
        let data = writer
            .finalize_with_progress(Some(&mut |written, total| {
                progress_calls.push((written, total));
                true
            }))
            .expect("Failed to write");

        assert_eq!(progress_calls.len(), 4);

        for i in 1..progress_calls.len() {
            assert!(
                progress_calls[i].0 > progress_calls[i - 1].0,
                "Progress should increase"
            );
        }

        let (last_written, last_total) = progress_calls.last().unwrap();
        assert_eq!(last_written, last_total);

        let archive = Reader::new(&data).expect("Failed to read");
        assert_eq!(archive.len(), 4);
        let content = archive.read_entry(1).expect("Failed to read entry");
        assert_eq!(content, b"Hello, World!");
    }

    #[test]
    fn test_write_with_progress_cancellation() {
        let mut writer = Writer::new();
        writer.add_file("test/file1.txt", b"Hello, World!".to_vec());
        writer.add_file("test/file2.txt", b"Second file content".to_vec());

        let mut call_count = 0;
        let result = writer.finalize_with_progress(Some(&mut |_written, _total| {
            call_count += 1;
            call_count < 2
        }));

        assert!(matches!(result, Err(Error::Cancelled)));
    }
}
