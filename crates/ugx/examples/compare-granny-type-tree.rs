//! Compares the Granny `FileInfo` type tree across multiple UGX files.
//!
//! Reads each UGX's 0x703 chunk, locates the `FileInfo` type definition pointer,
//! and extracts the entire type tree blob for comparison.
//!
//! Usage: cargo run -p ugx --example `compare-granny-type-tree` -- <`dir_or_file`...>

use std::collections::HashMap;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("Usage: compare-granny-type-tree <ugx_files_or_dirs...>");
        std::process::exit(1);
    }

    // Collect all .ugx paths
    let mut paths = Vec::new();
    for arg in &args {
        let meta = std::fs::metadata(arg)?;
        if meta.is_dir() {
            collect_ugx_files(arg, &mut paths);
        } else {
            paths.push(arg.clone());
        }
    }
    paths.sort();

    println!("Found {} UGX files\n", paths.len());

    // For each file, extract the type tree region from the granny chunk
    let mut signatures: HashMap<String, Vec<String>> = HashMap::new();
    let mut files_with_granny = 0;
    let mut files_without_granny = 0;

    for path in &paths {
        let data = std::fs::read(path)?;
        let ecf = match ecf::Reader::new(&data) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("  SKIP {}: ECF error: {}", short_name(path), e);
                continue;
            }
        };

        let Ok(granny) = ecf.chunk_data_by_id(0x703) else {
            files_without_granny += 1;
            continue;
        };
        files_with_granny += 1;

        // FileInfo header at offset 0. The type def pointer is at +0x88 (u64).
        // Actually, in the Granny layout: the type_def_ptr for FileInfo itself
        // is stored separately. Let's scan for it.
        //
        // The FileInfo struct occupies [0x00..0x94). The engine stores
        // a pointer to the FileInfo type definition somewhere in the chunk.
        // Let's find it by looking at what the writer doesn't write —
        // everything after the model/binding data and before the string table.
        //
        // Alternative approach: just hash the entire granny chunk region that
        // contains type definitions. We know type defs are 44-byte entries
        // with recognizable member_type values (2..21). Let's scan for the
        // largest contiguous block of valid type def entries.

        let type_tree = extract_type_tree_region(&granny);
        let fname = short_name(path);

        match type_tree {
            Some((offset, blob)) => {
                let hash = format!("{:016x}", simple_hash(&blob));
                println!(
                    "  {} — granny={} bytes, type_tree at 0x{:04X}, {} bytes, hash={}",
                    fname,
                    granny.len(),
                    offset,
                    blob.len(),
                    &hash[..12]
                );
                signatures.entry(hash).or_default().push(fname);
            }
            None => {
                println!(
                    "  {} — granny={} bytes, NO type tree found",
                    fname,
                    granny.len()
                );
            }
        }
    }

    println!("\n=== Summary ===");
    println!("Files with granny chunk: {files_with_granny}");
    println!("Files without granny chunk: {files_without_granny}");
    println!("Distinct type tree signatures: {}", signatures.len());
    for (hash, files) in &signatures {
        println!("\n  Hash {} — {} files:", &hash[..12], files.len());
        for f in files.iter().take(5) {
            println!("    {f}");
        }
        if files.len() > 5 {
            println!("    ... and {} more", files.len() - 5);
        }
    }

    Ok(())
}

fn collect_ugx_files(dir: &str, out: &mut Vec<String>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                collect_ugx_files(p.to_str().unwrap_or(""), out);
            } else if p.extension().is_some_and(|e| e == "ugx")
                && let Some(s) = p.to_str()
            {
                out.push(s.to_string());
            }
        }
    }
}

fn short_name(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

fn simple_hash(data: &[u8]) -> u64 {
    // FNV-1a 64-bit
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// Extract the `FileInfo` type definition tree from a granny chunk.
///
/// The type tree is a contiguous array of 44-byte `GrannyDataTypeDefinition`
/// entries. We scan the chunk for the longest contiguous run of valid entries
/// (`member_type` in 0..=21). Multiple terminated arrays may be adjacent
/// (nested struct definitions follow each other).
fn extract_type_tree_region(granny: &[u8]) -> Option<(usize, Vec<u8>)> {
    let stride = 44usize;
    if granny.len() < stride {
        return None;
    }

    let mut best_start = 0usize;
    let mut best_len = 0usize;

    let mut pos = 0usize;
    while pos + stride <= granny.len() {
        let mt = u32::from_le_bytes(granny[pos..pos + 4].try_into().ok()?);
        if mt <= 21 {
            let start = pos;
            let mut end = pos;
            while end + stride <= granny.len() {
                let mt2 = u32::from_le_bytes(granny[end..end + 4].try_into().unwrap());
                if mt2 > 21 {
                    break;
                }
                end += stride;
                if mt2 == 0 {
                    // Terminator — check if another type def array follows
                    if end + stride <= granny.len() {
                        let mt3 = u32::from_le_bytes(granny[end..end + 4].try_into().unwrap());
                        if mt3 > 0 && mt3 <= 21 {
                            continue;
                        }
                    }
                    break;
                }
            }
            let run_len = end - start;
            if run_len > best_len {
                best_len = run_len;
                best_start = start;
            }
            pos = end;
        } else {
            pos += 4;
        }
    }

    if best_len >= stride * 3 {
        Some((
            best_start,
            granny[best_start..best_start + best_len].to_vec(),
        ))
    } else {
        None
    }
}
