use std::env;

use era::EraArchive;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: era <command> [args...]");
        eprintln!();
        eprintln!("Commands:");
        eprintln!("  list <file.era>              List files in an ERA archive");
        eprintln!("  info <file.era>              Show archive information");
        std::process::exit(1);
    }

    let command = &args[1];

    match command.as_str() {
        "list" => {
            if args.len() < 3 {
                eprintln!("Usage: era list <file.era>");
                std::process::exit(1);
            }
            list_archive(&args[2]);
        }
        "info" => {
            if args.len() < 3 {
                eprintln!("Usage: era info <file.era>");
                std::process::exit(1);
            }
            info_archive(&args[2]);
        }
        _ => {
            eprintln!("Unknown command: {}", command);
            std::process::exit(1);
        }
    }
}

fn list_archive(path: &str) {
    let archive = match EraArchive::open(path) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Error opening archive: {}", e);
            std::process::exit(1);
        }
    };

    println!("Files in {}:", path);
    println!();

    for (i, entry) in archive.iter().enumerate() {
        let name = entry.filename.as_deref().unwrap_or("<unnamed>");
        let comp = entry.compressed_size();
        let decomp = entry.decompressed_size();
        let ratio = if decomp > 0 {
            (comp as f64 / decomp as f64) * 100.0
        } else {
            100.0
        };

        println!("{:5} {:>10} -> {:>10} ({:5.1}%) {}", i, comp, decomp, ratio, name);
    }

    println!();
    println!("Total: {} entries", archive.len());
}

fn info_archive(path: &str) {
    let archive = match EraArchive::open(path) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Error opening archive: {}", e);
            std::process::exit(1);
        }
    };

    println!("Archive: {}", path);
    println!();
    println!("ECF Header:");
    println!("  Magic:           0x{:08X}", archive.ecf_header.magic);
    println!("  Header Size:     {} bytes", archive.ecf_header.header_size);
    println!("  File Size:       {} bytes", archive.ecf_header.file_size);
    println!("  Num Chunks:      {}", archive.ecf_header.num_chunks);
    println!("  Chunk Extra:     {} bytes", archive.ecf_header.chunk_extra_data_size);
    println!();
    println!("Archive Header:");
    println!("  Archive Magic:   0x{:08X}", archive.archive_header.archive_magic);
    println!("  Signature Size:  {} bytes", archive.archive_header.signature_size);
    println!();
    println!("Entries: {}", archive.len());
}

