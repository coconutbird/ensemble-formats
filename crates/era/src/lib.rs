//! ERA archive format for Halo Wars Definitive Edition
//!
//! ERA files are ECF-based archives containing compressed game assets.
//! Files are encrypted using TEA cipher in CTR mode with 64-byte blocks.
//!
//! # Reading ERA archives
//!
//! ```no_run
//! use era::EraArchive;
//!
//! let mut archive = EraArchive::open("root.era").unwrap();
//! for entry in archive.iter() {
//!     println!("{}", entry.filename.as_deref().unwrap_or("<unnamed>"));
//! }
//! ```
//!
//! # Writing ERA archives
//!
//! ```no_run
//! use era::EraWriter;
//!
//! let mut writer = EraWriter::new();
//! writer.add_file("data/test.txt", b"Hello, world!".to_vec());
//! writer.write_to_file("output.era").unwrap();
//! ```

pub mod buffer_pool;
pub mod crypto;
mod decrypt_reader;
mod encrypt_writer;
mod era;
mod error;
pub mod mmap;
mod writer;

pub use buffer_pool::{BufferPool, PooledBuffer};
pub use crypto::{
    tea_decrypt_data_parallel, tea_encrypt_data_parallel, TeaKeys, ARCHIVE_PASSWORD, TEA_BLOCK_SIZE,
};
pub use decrypt_reader::DecryptReader;
pub use encrypt_writer::EncryptWriter;
pub use era::*;
pub use error::*;
pub use mmap::MmapEraArchive;
pub use writer::{compress_file_data, CompressedData, EraWriter};

// Re-export ECF types that are used in the public API
pub use ecf::{CompressionMethod, EcfChunkHeader, EcfHeader};

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_era_roundtrip_single_file() {
        let mut writer = EraWriter::new();
        writer.add_file("test/hello.txt", b"Hello, World!".to_vec());

        let mut buffer = Cursor::new(Vec::new());
        let keys = TeaKeys::default_archive_keys();
        let encrypt_writer = EncryptWriter::new(&mut buffer, keys);
        writer.write(encrypt_writer).expect("Failed to write");

        let data = buffer.into_inner();
        let cursor = Cursor::new(data);
        let decrypt_reader = DecryptReader::new(cursor, keys);
        let mut archive = EraArchive::new(decrypt_reader).expect("Failed to read");

        assert_eq!(archive.len(), 2); // filename chunk + 1 file
        let entry = archive.entry(1).unwrap();
        assert_eq!(entry.filename.as_deref(), Some("test\\hello.txt"));

        let content = archive.read_entry(1).expect("Failed to read entry");
        assert_eq!(content, b"Hello, World!");
    }

    #[test]
    fn test_era_roundtrip_multiple_files() {
        let mut writer = EraWriter::new();
        writer.add_file("test/hello.txt", b"Hello, World!".to_vec());
        writer.add_file("data/numbers.bin", vec![1, 2, 3, 4, 5, 6, 7, 8]);
        writer.add_file("empty.txt", vec![]);

        let mut buffer = Cursor::new(Vec::new());
        let keys = TeaKeys::default_archive_keys();
        let encrypt_writer = EncryptWriter::new(&mut buffer, keys);
        writer.write(encrypt_writer).expect("Failed to write");

        let data = buffer.into_inner();
        let cursor = Cursor::new(data);
        let decrypt_reader = DecryptReader::new(cursor, keys);
        let mut archive = EraArchive::new(decrypt_reader).expect("Failed to read");

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
        let mut writer = EraWriter::new();
        writer.add_file("test/hello.txt", b"Hello, World!".to_vec());
        writer.add_file("data/numbers.bin", vec![1, 2, 3, 4, 5, 6, 7, 8]);

        // First write
        let mut buffer1 = Cursor::new(Vec::new());
        let keys = TeaKeys::default_archive_keys();
        let encrypt_writer = EncryptWriter::new(&mut buffer1, keys);
        writer.write(encrypt_writer).expect("Failed to write");
        let data1 = buffer1.into_inner();

        // Read back and collect hashes
        let cursor = Cursor::new(data1.clone());
        let decrypt_reader = DecryptReader::new(cursor, keys);
        let archive = EraArchive::new(decrypt_reader).expect("Failed to read");
        let hashes1: Vec<_> = archive.iter().map(|e| e.extra.comp_tiger128).collect();

        // Create new writer from extracted files
        let mut writer2 = EraWriter::new();
        for i in 1..archive.len() {
            let entry = archive.entry(i).unwrap();
            let filename = entry.filename.as_ref().unwrap();
            let cursor2 = Cursor::new(data1.clone());
            let decrypt_reader2 = DecryptReader::new(cursor2, keys);
            let mut archive2 = EraArchive::new(decrypt_reader2).expect("Failed to read");
            let content = archive2.read_entry(i).expect("Failed to read entry");
            writer2.add_file(filename, content);
        }

        // Second write
        let mut buffer2 = Cursor::new(Vec::new());
        let encrypt_writer2 = EncryptWriter::new(&mut buffer2, keys);
        writer2
            .write(encrypt_writer2)
            .expect("Failed to write second time");
        let data2 = buffer2.into_inner();

        // Read second archive and collect hashes
        let cursor2 = Cursor::new(data2.clone());
        let decrypt_reader2 = DecryptReader::new(cursor2, keys);
        let archive2 = EraArchive::new(decrypt_reader2).expect("Failed to read second");
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
        // Test with a file larger than TEA block size (64 bytes)
        let large_data: Vec<u8> = (0..1000).map(|i| (i % 256) as u8).collect();

        let mut writer = EraWriter::new();
        writer.add_file("large.bin", large_data.clone());

        let mut buffer = Cursor::new(Vec::new());
        let keys = TeaKeys::default_archive_keys();
        let encrypt_writer = EncryptWriter::new(&mut buffer, keys);
        writer.write(encrypt_writer).expect("Failed to write");

        let data = buffer.into_inner();
        let cursor = Cursor::new(data);
        let decrypt_reader = DecryptReader::new(cursor, keys);
        let mut archive = EraArchive::new(decrypt_reader).expect("Failed to read");

        let content = archive.read_entry(1).expect("Failed to read entry");
        assert_eq!(content, large_data);
    }

    #[test]
    fn test_tea_encrypt_decrypt_roundtrip() {
        use crate::crypto::{tea_decrypt_block64, tea_encrypt_block64};

        let keys = TeaKeys::default_archive_keys();
        let original: [u8; 64] = std::array::from_fn(|i| i as u8);
        let mut encrypted = [0u8; 64];
        let mut decrypted = [0u8; 64];

        tea_encrypt_block64(&keys, &original, &mut encrypted, 0);
        tea_decrypt_block64(&keys, &encrypted, &mut decrypted, 0);

        assert_eq!(original, decrypted);
    }

    #[test]
    fn test_parallel_encryption_roundtrip() {
        use crate::crypto::{
            tea_decrypt_data_parallel, tea_encrypt_data, tea_encrypt_data_parallel,
        };

        let keys = TeaKeys::default_archive_keys();

        // Create 10 blocks of data (640 bytes)
        let original: Vec<u8> = (0..640).map(|i| (i % 256) as u8).collect();

        // Test parallel encrypt + parallel decrypt
        let mut data1 = original.clone();
        tea_encrypt_data_parallel(&keys, &mut data1, 0);
        tea_decrypt_data_parallel(&keys, &mut data1, 0);
        assert_eq!(original, data1);

        // Test sequential encrypt + parallel decrypt (mixed usage)
        let mut data2 = original.clone();
        tea_encrypt_data(&keys, &mut data2, 0);
        tea_decrypt_data_parallel(&keys, &mut data2, 0);
        assert_eq!(original, data2);
    }

    #[test]
    fn test_precompressed_file() {
        // Test that we can add pre-compressed files to an archive
        let mut writer = EraWriter::new();

        // Add a regular file
        writer.add_file("regular.txt", b"Hello, World!".to_vec());

        // Add a pre-compressed file (simulate copying from another archive)
        let compressed = compress_file_data(b"Pre-compressed data").unwrap();
        writer.add_compressed_file(
            "precompressed.txt",
            compressed.data.clone(),
            compressed.decompressed_size,
            compressed.tiger128,
        );

        let mut buffer = Cursor::new(Vec::new());
        let keys = TeaKeys::default_archive_keys();
        let encrypt_writer = EncryptWriter::new(&mut buffer, keys);
        writer.write(encrypt_writer).expect("Failed to write");

        let data = buffer.into_inner();
        let cursor = Cursor::new(data);
        let decrypt_reader = DecryptReader::new(cursor, keys);
        let mut archive = EraArchive::new(decrypt_reader).expect("Failed to read");

        assert_eq!(archive.len(), 3); // filename chunk + 2 files

        let content1 = archive.read_entry(1).expect("Failed to read entry 1");
        assert_eq!(content1, b"Hello, World!");

        let content2 = archive.read_entry(2).expect("Failed to read entry 2");
        assert_eq!(content2, b"Pre-compressed data");
    }

    #[test]
    fn test_read_entry_compressed() {
        let mut writer = EraWriter::new();
        writer.add_file("test.txt", b"Test content for compression".to_vec());

        let mut buffer = Cursor::new(Vec::new());
        let keys = TeaKeys::default_archive_keys();
        let encrypt_writer = EncryptWriter::new(&mut buffer, keys);
        writer.write(encrypt_writer).expect("Failed to write");

        let data = buffer.into_inner();
        let cursor = Cursor::new(data.clone());
        let decrypt_reader = DecryptReader::new(cursor, keys);
        let mut archive = EraArchive::new(decrypt_reader).expect("Failed to read");

        // Read compressed
        let (compressed, decomp_size, tiger128) =
            archive.read_entry_compressed(1).expect("Failed to read");

        assert_eq!(decomp_size, 28); // "Test content for compression".len()
        assert!(!compressed.is_empty());
        assert_ne!(tiger128, [0u8; 16]); // Hash should be set

        // Verify we can read the same data normally
        let cursor2 = Cursor::new(data);
        let decrypt_reader2 = DecryptReader::new(cursor2, keys);
        let mut archive2 = EraArchive::new(decrypt_reader2).expect("Failed to read");
        let decompressed = archive2.read_entry(1).expect("Failed to read");
        assert_eq!(decompressed, b"Test content for compression");
    }

    #[test]
    fn test_write_with_progress() {
        let mut writer = EraWriter::new();
        writer.add_file("test/file1.txt", b"Hello, World!".to_vec());
        writer.add_file("test/file2.txt", b"Second file content".to_vec());
        writer.add_file("test/file3.txt", b"Third file with more data here".to_vec());

        let mut buffer = Cursor::new(Vec::new());
        let keys = TeaKeys::default_archive_keys();
        let encrypt_writer = EncryptWriter::new(&mut buffer, keys);

        let mut progress_calls = Vec::new();
        writer
            .write_with_progress(
                encrypt_writer,
                Some(&mut |written, total| {
                    progress_calls.push((written, total));
                    true // continue
                }),
            )
            .expect("Failed to write");

        // Should have 4 progress calls (filename table + 3 files)
        assert_eq!(progress_calls.len(), 4);

        // Verify progress is monotonically increasing
        for i in 1..progress_calls.len() {
            assert!(
                progress_calls[i].0 > progress_calls[i - 1].0,
                "Progress should increase"
            );
        }

        // Last call should have written == total
        let (last_written, last_total) = progress_calls.last().unwrap();
        assert_eq!(last_written, last_total);

        // Verify the archive can be read back
        let data = buffer.into_inner();
        let cursor = Cursor::new(data);
        let decrypt_reader = DecryptReader::new(cursor, keys);
        let mut archive = EraArchive::new(decrypt_reader).expect("Failed to read");
        assert_eq!(archive.len(), 4); // filename chunk + 3 files
        let content = archive.read_entry(1).expect("Failed to read entry");
        assert_eq!(content, b"Hello, World!");
    }

    #[test]
    fn test_write_with_progress_cancellation() {
        let mut writer = EraWriter::new();
        writer.add_file("test/file1.txt", b"Hello, World!".to_vec());
        writer.add_file("test/file2.txt", b"Second file content".to_vec());

        let mut buffer = Cursor::new(Vec::new());
        let keys = TeaKeys::default_archive_keys();
        let encrypt_writer = EncryptWriter::new(&mut buffer, keys);

        let mut call_count = 0;
        let result = writer.write_with_progress(
            encrypt_writer,
            Some(&mut |_written, _total| {
                call_count += 1;
                call_count < 2 // Cancel after first callback
            }),
        );

        // Should return Cancelled error
        assert!(matches!(result, Err(Error::Cancelled)));
    }

    #[test]
    #[ignore]
    fn search_era_foxcannon() {
        let archive = EraArchive::open("root.era").expect("Failed to open root.era");
        println!("ERA archive: {} entries", archive.len());

        let search_terms = [
            "foxcannon",
            "mesh_turret",
            "mesh_barrel",
            "mesh_chassis",
            ".ugx",
            ".gr2",
            "vertex",
            ".vtx",
        ];

        for term in &search_terms {
            println!("\n=== Searching for '{}' ===", term);
            let term_lower = term.to_lowercase();
            let mut count = 0;
            for (i, entry) in archive.iter().enumerate() {
                if let Some(ref name) = entry.filename {
                    if name.to_lowercase().contains(&term_lower) {
                        println!(
                            "  {}: {} ({} bytes -> {} bytes)",
                            i,
                            name,
                            entry.compressed_size(),
                            entry.decompressed_size()
                        );
                        count += 1;
                        if count > 20 {
                            println!("  ... and more");
                            break;
                        }
                    }
                }
            }
        }
    }
}
