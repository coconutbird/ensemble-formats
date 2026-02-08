use std::env;
use std::fs;
use std::path::Path;

use era::{EraArchive, EraWriter};

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: era <command> [args...]");
        eprintln!();
        eprintln!("Commands:");
        eprintln!("  list <file.era>              List files in an ERA archive");
        eprintln!("  info <file.era>              Show archive information");
        eprintln!("  extract <file.era> [outdir]  Extract files from an ERA archive");
        eprintln!("  create <file.era> <indir>    Create an ERA archive from a directory");
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
        "extract" => {
            if args.len() < 3 {
                eprintln!("Usage: era extract <file.era> [outdir]");
                std::process::exit(1);
            }
            let outdir = args.get(3).map(|s| s.as_str());
            extract_archive(&args[2], outdir);
        }
        "create" => {
            if args.len() < 4 {
                eprintln!("Usage: era create <file.era> <indir>");
                std::process::exit(1);
            }
            create_archive(&args[2], &args[3]);
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

        println!(
            "{:5} {:>10} -> {:>10} ({:5.1}%) {}",
            i, comp, decomp, ratio, name
        );
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
    println!(
        "  Header Size:     {} bytes",
        archive.ecf_header.header_size
    );
    println!("  File Size:       {} bytes", archive.ecf_header.file_size);
    println!("  Num Chunks:      {}", archive.ecf_header.num_chunks);
    println!(
        "  Chunk Extra:     {} bytes",
        archive.ecf_header.chunk_extra_data_size
    );
    println!();
    println!("Archive Header:");
    println!(
        "  Archive Magic:   0x{:08X}",
        archive.archive_header.archive_magic
    );
    println!(
        "  Signature Size:  {} bytes",
        archive.archive_header.signature_size
    );
    println!();
    println!("Entries: {}", archive.len());
}

fn extract_archive(path: &str, outdir: Option<&str>) {
    let mut archive = match EraArchive::open(path) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Error opening archive: {}", e);
            std::process::exit(1);
        }
    };

    // Default output directory is the archive name without extension
    let default_outdir = Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "output".to_string());
    let outdir = outdir.unwrap_or(&default_outdir);
    let outdir = Path::new(outdir);

    // Create output directory
    if let Err(e) = fs::create_dir_all(outdir) {
        eprintln!("Error creating output directory: {}", e);
        std::process::exit(1);
    }

    println!("Extracting {} to {}...", path, outdir.display());

    let mut success = 0;
    let mut errors = 0;

    // Skip entry 0 (filename table)
    for i in 1..archive.len() {
        let entry = archive.entry(i).unwrap().clone();
        let filename = entry.filename.as_deref().unwrap_or("<unnamed>");

        // Read entry data
        let data = match archive.read_entry(i) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("  Error reading {}: {}", filename, e);
                errors += 1;
                continue;
            }
        };

        // Build output path
        let file_path = outdir.join(filename.replace('\\', "/"));

        // Create parent directories
        if let Some(parent) = file_path.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                eprintln!("  Error creating directory for {}: {}", filename, e);
                errors += 1;
                continue;
            }
        }

        // Write file
        if let Err(e) = fs::write(&file_path, &data) {
            eprintln!("  Error writing {}: {}", filename, e);
            errors += 1;
            continue;
        }

        println!("  {}", filename);
        success += 1;
    }

    println!();
    println!("Extracted {} files ({} errors)", success, errors);
}

fn create_archive(output_path: &str, input_dir: &str) {
    let input_path = Path::new(input_dir);

    if !input_path.is_dir() {
        eprintln!("Error: {} is not a directory", input_dir);
        std::process::exit(1);
    }

    println!("Creating {} from {}...", output_path, input_dir);

    let mut writer = EraWriter::new();

    // Recursively collect files
    let mut file_count = 0;
    if let Err(e) = collect_files(input_path, input_path, &mut writer, &mut file_count) {
        eprintln!("Error collecting files: {}", e);
        std::process::exit(1);
    }

    println!("  Collected {} files", file_count);

    // Write archive
    if let Err(e) = writer.write_to_file(output_path) {
        eprintln!("Error writing archive: {}", e);
        std::process::exit(1);
    }

    println!("Done!");
}

fn collect_files(
    base: &Path,
    dir: &Path,
    writer: &mut EraWriter,
    count: &mut usize,
) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            collect_files(base, &path, writer, count)?;
        } else if path.is_file() {
            // Get relative path with backslashes (ERA convention)
            let rel_path = path
                .strip_prefix(base)
                .unwrap()
                .to_string_lossy()
                .replace('/', "\\");

            let data = fs::read(&path)?;
            writer.add_file(rel_path, data);
            *count += 1;
        }
    }
    Ok(())
}
