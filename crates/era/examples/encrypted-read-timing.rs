//! Compare streaming and bulk reads from an encrypted ERA archive.

use std::hint::black_box;
use std::time::Instant;

use era::{Reader, TeaKeys};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args().skip(1);
    let path = arguments
        .next()
        .ok_or("usage: encrypted-read-timing ERA [COUNT]")?;
    let count = arguments
        .next()
        .map(|value| value.parse::<usize>())
        .transpose()?
        .unwrap_or(16);
    let encrypted = std::fs::read(&path)?;
    let mut reader = Reader::from_encrypted_bytes(encrypted, TeaKeys::default_archive_keys())?;
    let mut entries = reader
        .entries()
        .iter()
        .enumerate()
        .skip(1)
        .map(|(index, entry)| (index, entry.compressed_size(), entry.decompressed_size()))
        .collect::<Vec<_>>();
    entries.sort_unstable_by_key(|&(_, compressed, _)| std::cmp::Reverse(compressed));
    entries.truncate(count);
    let count = entries.len();

    let compressed_bytes = entries
        .iter()
        .map(|&(_, size, _)| u64::from(size))
        .sum::<u64>();
    let decompressed_bytes = entries
        .iter()
        .map(|&(_, _, size)| u64::from(size))
        .sum::<u64>();

    let started = Instant::now();
    for &(index, _, _) in &entries {
        black_box(reader.read_entry(index)?);
    }
    let streaming = started.elapsed();

    let started = Instant::now();
    for &(index, _, _) in &entries {
        black_box(reader.read_entry_direct(index)?);
    }
    let direct = started.elapsed();

    println!("entries: {count}");
    println!("compressed: {compressed_bytes} bytes");
    println!("decompressed: {decompressed_bytes} bytes");
    println!("streaming: {:>9.3} ms", streaming.as_secs_f64() * 1_000.0);
    println!("direct:    {:>9.3} ms", direct.as_secs_f64() * 1_000.0);
    println!(
        "speedup:   {:>9.2}x",
        streaming.as_secs_f64() / direct.as_secs_f64()
    );

    Ok(())
}
