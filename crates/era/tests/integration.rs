use era::crypto::tea::{tea_decrypt_block64, tea_encrypt_block64};
use era::{Error, Reader, TeaKeys, Writer, compress_file_data};

#[test]
fn roundtrip_single_file() {
    let mut writer = Writer::new();
    writer.add_file("test/hello.txt", b"Hello, World!".to_vec());

    let data = writer.finalize().expect("Failed to write");
    let mut archive = Reader::from_bytes(&data).expect("Failed to read");

    assert_eq!(archive.len(), 2); // filename chunk + 1 file
    let entry = archive.entry(1).unwrap();
    assert_eq!(entry.filename.as_deref(), Some("test\\hello.txt"));

    let content = archive.read_entry(1).expect("Failed to read entry");
    assert_eq!(content, b"Hello, World!");
}

#[test]
fn roundtrip_multiple_files() {
    let mut writer = Writer::new();
    writer.add_file("test/hello.txt", b"Hello, World!".to_vec());
    writer.add_file("data/numbers.bin", vec![1, 2, 3, 4, 5, 6, 7, 8]);
    writer.add_file("empty.txt", vec![]);

    let data = writer.finalize().expect("Failed to write");
    let mut archive = Reader::from_bytes(&data).expect("Failed to read");

    assert_eq!(archive.len(), 4); // filename chunk + 3 files

    let content1 = archive.read_entry(1).expect("Failed to read entry 1");
    assert_eq!(content1, b"Hello, World!");

    let content2 = archive.read_entry(2).expect("Failed to read entry 2");
    assert_eq!(content2, vec![1, 2, 3, 4, 5, 6, 7, 8]);

    let content3 = archive.read_entry(3).expect("Failed to read entry 3");
    assert_eq!(content3, vec![]);
}

#[test]
fn roundtrip_identical_bytes() {
    let mut writer = Writer::new();
    writer.add_file("test/hello.txt", b"Hello, World!".to_vec());
    writer.add_file("data/numbers.bin", vec![1, 2, 3, 4, 5, 6, 7, 8]);

    let data1 = writer.finalize().expect("Failed to write");

    let mut archive = Reader::from_bytes(&data1).expect("Failed to read");
    let hashes1: Vec<_> = archive.iter().map(|e| e.extra.comp_tiger128).collect();

    let mut writer2 = Writer::new();
    for i in 1..archive.len() {
        let entry = archive.entry(i).unwrap();
        let filename = entry.filename.as_ref().unwrap().clone();
        let content = archive.read_entry(i).expect("Failed to read entry");
        writer2.add_file(filename, content);
    }

    let data2 = writer2.finalize().expect("Failed to write second time");

    let archive2 = Reader::from_bytes(&data2).expect("Failed to read second");
    let hashes2: Vec<_> = archive2.iter().map(|e| e.extra.comp_tiger128).collect();

    assert_eq!(data1, data2, "Round-trip should produce identical bytes");
    assert_eq!(
        hashes1, hashes2,
        "Tiger128 hashes should match after roundtrip"
    );
}

#[test]
fn large_file() {
    let large_data: Vec<u8> = (0..1000).map(|i| (i % 256) as u8).collect();

    let mut writer = Writer::new();
    writer.add_file("large.bin", large_data.clone());

    let data = writer.finalize().expect("Failed to write");
    let mut archive = Reader::from_bytes(&data).expect("Failed to read");

    let content = archive.read_entry(1).expect("Failed to read entry");
    assert_eq!(content, large_data);
}

#[test]
fn tea_encrypt_decrypt_roundtrip() {
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
fn parallel_encryption_roundtrip() {
    use era::crypto::tea::{
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
fn precompressed_file() {
    let mut writer = Writer::new();
    writer.add_file("regular.txt", b"Regular file".to_vec());

    let compressed = compress_file_data(b"Pre-compressed data");
    writer.add_compressed_file(
        "precomp.txt",
        compressed.data.clone(),
        compressed.decompressed_size,
        compressed.tiger128,
    );

    let data = writer.finalize().expect("Failed to write");
    let mut archive = Reader::from_bytes(&data).expect("Failed to read");

    assert_eq!(archive.len(), 3);

    let content1 = archive.read_entry(1).expect("Failed to read entry 1");
    assert_eq!(content1, b"Regular file");

    let content2 = archive.read_entry(2).expect("Failed to read entry 2");
    assert_eq!(content2, b"Pre-compressed data");
}

#[test]
fn read_entry_compressed() {
    let mut writer = Writer::new();
    writer.add_file("test.txt", b"Test content for compression".to_vec());

    let data = writer.finalize().expect("Failed to write");
    let mut archive = Reader::from_bytes(&data).expect("Failed to read");

    let (compressed, decomp_size, tiger128) =
        archive.read_entry_compressed(1).expect("Failed to read");

    assert_eq!(decomp_size, 28);
    assert!(!compressed.is_empty());
    assert_ne!(tiger128, [0u8; 16]);

    let decompressed = archive.read_entry(1).expect("Failed to read");
    assert_eq!(decompressed, b"Test content for compression");
}

#[test]
fn write_with_progress() {
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

    let mut archive = Reader::from_bytes(&data).expect("Failed to read");
    assert_eq!(archive.len(), 4);
    let content = archive.read_entry(1).expect("Failed to read entry");
    assert_eq!(content, b"Hello, World!");
}

#[test]
fn write_with_progress_cancellation() {
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

#[test]
fn write_to_matches_finalize() {
    let mut writer = Writer::new();
    writer.add_file("test/hello.txt", b"Hello, World!".to_vec());
    writer.add_file("data/numbers.bin", vec![1, 2, 3, 4, 5, 6, 7, 8]);

    let finalized = writer.finalize().expect("finalize failed");

    let mut streamed = Vec::new();
    writer.write_to(&mut streamed).expect("write_to failed");

    assert_eq!(
        finalized, streamed,
        "write_to must produce identical bytes to finalize"
    );

    let mut archive = Reader::from_bytes(&streamed).expect("Failed to read streamed");
    assert_eq!(archive.len(), 3);
    assert_eq!(archive.read_entry(1).unwrap(), b"Hello, World!");
    assert_eq!(archive.read_entry(2).unwrap(), vec![1, 2, 3, 4, 5, 6, 7, 8]);
}

#[test]
fn write_to_with_precompressed() {
    let mut writer = Writer::new();
    writer.add_file("regular.txt", b"Regular file".to_vec());

    let compressed = compress_file_data(b"Pre-compressed data");
    writer.add_compressed_file(
        "precomp.txt",
        compressed.data.clone(),
        compressed.decompressed_size,
        compressed.tiger128,
    );

    let finalized = writer.finalize().expect("finalize failed");
    let mut streamed = Vec::new();
    writer.write_to(&mut streamed).expect("write_to failed");

    assert_eq!(finalized, streamed);
}
