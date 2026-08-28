//! ERA CLI - Tool for working with ERA archives from Halo Wars.

use std::fs;
use std::path::Path;

use clap::{Parser, Subcommand};
use era::{Reader, TeaKeys, Writer};
use serde::Serialize;

/// Exit codes for scripting
pub mod exit_code {
    /// Success
    pub const SUCCESS: i32 = 0;
    /// General error
    pub const ERROR: i32 = 1;
    /// File not found
    pub const FILE_NOT_FOUND: i32 = 2;
    /// Invalid archive format
    pub const INVALID_FORMAT: i32 = 3;
    /// I/O error
    pub const IO_ERROR: i32 = 4;
    /// Partial failure (some files failed)
    pub const PARTIAL_FAILURE: i32 = 5;
}

/// Convert a byte slice to a lowercase hex string
fn bytes_to_hex(bytes: &[u8]) -> String {
    hex::encode(bytes)
}

/// Format a byte size as a human-readable string
fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format_scaled_size(bytes, GB, "GB")
    } else if bytes >= MB {
        format_scaled_size(bytes, MB, "MB")
    } else if bytes >= KB {
        format_scaled_size(bytes, KB, "KB")
    } else {
        format!("{bytes} B")
    }
}

fn format_scaled_size(bytes: u64, unit: u64, suffix: &str) -> String {
    let whole = bytes / unit;
    let tenths = (bytes % unit) * 10 / unit;
    format!("{whole}.{tenths} {suffix}")
}

fn format_ratio(numerator: u64, denominator: u64) -> String {
    if denominator == 0 {
        return "100.0".to_string();
    }
    let tenths = u128::from(numerator) * 1000 / u128::from(denominator);
    format!("{}.{}", tenths / 10, tenths % 10)
}

#[derive(Parser)]
#[command(name = "era")]
#[command(author, version, about = "ERA archive tool for Halo Wars", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Output in JSON format for scripting
    #[arg(long, global = true)]
    json: bool,

    /// Suppress non-essential output
    #[arg(short, long, global = true)]
    quiet: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// List files in an ERA archive
    List {
        /// Path to the ERA archive
        file: String,
    },
    /// Show archive information
    Info {
        /// Path to the ERA archive
        file: String,
    },
    /// Extract files from an ERA archive
    Extract {
        /// Path to the ERA archive
        file: String,
        /// Output directory (defaults to archive name without extension)
        #[arg(short, long)]
        output: Option<String>,
        /// Unix shell style glob pattern to filter files (e.g., "*.ugx", "data/**/*.xmb")
        #[arg(short, long)]
        filter: Option<String>,
        /// Specific files to extract
        #[arg(trailing_var_arg = true)]
        files: Vec<String>,
    },
    /// Create an ERA archive from a directory
    Create {
        /// Output ERA file path
        output: String,
        /// Input directory containing files to archive
        input: String,
    },
    /// Verify the Merkle signature of an ERA archive
    Verify {
        /// Path to the ERA archive
        file: String,
        /// Public key as 40-char hex string (20 bytes)
        #[arg(short, long)]
        key: String,
    },
}

fn main() {
    let cli = Cli::parse();

    let exit_code = match &cli.command {
        Commands::List { file } => list_archive(file, cli.json),
        Commands::Info { file } => info_archive(file, cli.json),
        Commands::Extract {
            file,
            output,
            filter,
            files,
        } => extract_archive(
            file,
            output.as_deref(),
            filter.as_deref(),
            files,
            cli.json,
            cli.quiet,
        ),
        Commands::Create { output, input } => create_archive(output, input, cli.json, cli.quiet),
        Commands::Verify { file, key } => verify_archive(file, key, cli.json),
    };

    std::process::exit(exit_code);
}

/// JSON output for list command
#[derive(Serialize)]
struct ListOutput {
    archive: String,
    entries: Vec<ListEntry>,
    total_entries: usize,
}

#[derive(Serialize)]
struct ListEntry {
    index: usize,
    filename: Option<String>,
    compressed_size: u32,
    decompressed_size: u32,
    compression_ratio: f64,
    /// Tiger128 hash of compressed data (hex string)
    tiger128: String,
}

type ArchiveReader = Reader<era::crypto::decrypt::Reader<std::io::BufReader<std::fs::File>>>;

fn build_list_entries(archive: &ArchiveReader) -> Vec<ListEntry> {
    archive
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let compressed_size = entry.compressed_size();
            let decompressed_size = entry.decompressed_size();
            let compression_ratio = if decompressed_size > 0 {
                (f64::from(compressed_size) / f64::from(decompressed_size)) * 100.0
            } else {
                100.0
            };
            ListEntry {
                index,
                filename: if index == 0 {
                    Some("<filename table>".to_string())
                } else {
                    entry.filename.clone()
                },
                compressed_size,
                decompressed_size,
                compression_ratio,
                tiger128: bytes_to_hex(&entry.extra.comp_tiger128),
            }
        })
        .collect()
}

/// Read and decrypt an ERA file into a byte buffer.
fn open_archive(path: &str) -> Result<ArchiveReader, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("Failed to open {path}: {e}"))?;
    let buf = std::io::BufReader::new(file);
    Reader::from_encrypted(buf, TeaKeys::default_archive_keys())
        .map_err(|e| format!("Failed to parse archive: {e}"))
}

fn list_archive(path: &str, json: bool) -> i32 {
    let archive = match open_archive(path) {
        Ok(a) => a,
        Err(e) => {
            if json {
                eprintln!(r#"{{"error": "{e}"}}"#);
            } else {
                eprintln!("Error opening archive: {e}");
            }
            return exit_code::FILE_NOT_FOUND;
        }
    };

    let entries = build_list_entries(&archive);

    if json {
        let output = ListOutput {
            archive: path.to_string(),
            entries,
            total_entries: archive.len(),
        };
        println!("{}", serde_json::to_string(&output).unwrap());
    } else {
        println!("Files in {path}:");
        println!();

        // Find the longest filename for alignment
        let max_name_len = entries
            .iter()
            .map(|e| e.filename.as_deref().unwrap_or("<unnamed>").len())
            .max()
            .unwrap_or(10)
            .max(8); // minimum width for "Filename" header

        // Print header
        println!(
            "{:>5}  {:>10}  {:>12}  {:>6}  {:<width$}  {:>32}",
            "Index",
            "Compressed",
            "Decompressed",
            "Ratio",
            "Filename",
            "Tiger128",
            width = max_name_len
        );
        println!(
            "{:->5}  {:->10}  {:->12}  {:->6}  {:-<width$}  {:->32}",
            "",
            "",
            "",
            "",
            "",
            "",
            width = max_name_len
        );

        for entry in &entries {
            let name = entry.filename.as_deref().unwrap_or("<unnamed>");
            println!(
                "{:>5}  {:>10}  {:>12}  {:>5.1}%  {:<width$}  {}",
                entry.index,
                format_size(u64::from(entry.compressed_size)),
                format_size(u64::from(entry.decompressed_size)),
                entry.compression_ratio,
                name,
                entry.tiger128,
                width = max_name_len
            );
        }

        // Calculate totals
        let total_compressed: u64 = entries.iter().map(|e| u64::from(e.compressed_size)).sum();
        let total_decompressed: u64 = entries.iter().map(|e| u64::from(e.decompressed_size)).sum();
        let total_ratio = format_ratio(total_compressed, total_decompressed);

        println!();
        println!(
            "Total: {} entries, {} -> {} ({}%)",
            archive.len(),
            format_size(total_compressed),
            format_size(total_decompressed),
            total_ratio
        );
    }

    exit_code::SUCCESS
}

/// JSON output for info command
#[derive(Serialize)]
struct InfoOutput {
    archive: String,
    ecf_header: HeaderInfo,
    archive_header: ArchiveHeaderInfo,
    total_entries: usize,
}

#[derive(Serialize)]
struct HeaderInfo {
    magic: String,
    header_size: u32,
    file_size: u32,
    num_chunks: u16,
    chunk_extra_data_size: u16,
}

#[derive(Serialize)]
struct ArchiveHeaderInfo {
    magic: String,
    signature_size: u32,
}

fn info_archive(path: &str, json: bool) -> i32 {
    let archive = match open_archive(path) {
        Ok(a) => a,
        Err(e) => {
            if json {
                eprintln!(r#"{{"error": "{e}"}}"#);
            } else {
                eprintln!("Error opening archive: {e}");
            }
            return exit_code::FILE_NOT_FOUND;
        }
    };

    if json {
        let output = InfoOutput {
            archive: path.to_string(),
            ecf_header: HeaderInfo {
                magic: format!("0x{:08X}", archive.ecf_header.magic),
                header_size: archive.ecf_header.header_size,
                file_size: archive.ecf_header.file_size,
                num_chunks: archive.ecf_header.num_chunks,
                chunk_extra_data_size: archive.ecf_header.chunk_extra_data_size,
            },
            archive_header: ArchiveHeaderInfo {
                magic: format!("0x{:08X}", archive.archive_header.archive_magic),
                signature_size: archive.archive_header.signature_size,
            },
            total_entries: archive.len(),
        };
        println!("{}", serde_json::to_string(&output).unwrap());
    } else {
        // Calculate summary statistics (skip index 0 which is filename table)
        let mut total_compressed: u64 = 0;
        let mut total_decompressed: u64 = 0;
        for entry in archive.iter().skip(1) {
            total_compressed += u64::from(entry.compressed_size());
            total_decompressed += u64::from(entry.decompressed_size());
        }
        let file_count = archive.len().saturating_sub(1); // exclude filename table
        let ratio = format_ratio(total_compressed, total_decompressed);

        println!("Archive: {path}");
        println!();
        println!("ECF Header");
        println!("  Magic              0x{:08X}", archive.ecf_header.magic);
        println!(
            "  Header Size        {}",
            format_size(u64::from(archive.ecf_header.header_size))
        );
        println!(
            "  File Size          {}",
            format_size(u64::from(archive.ecf_header.file_size))
        );
        println!("  Chunks             {}", archive.ecf_header.num_chunks);
        println!();
        println!("ERA Header");
        println!(
            "  Magic              0x{:08X}",
            archive.archive_header.archive_magic
        );
        println!(
            "  Signature Size     {}",
            format_size(u64::from(archive.archive_header.signature_size))
        );
        println!();
        println!("Summary");
        println!("  Files              {file_count}");
        println!("  Compressed         {}", format_size(total_compressed));
        println!("  Decompressed       {}", format_size(total_decompressed));
        println!("  Ratio              {ratio}%");
    }

    exit_code::SUCCESS
}

/// JSON output for extract command
#[derive(Serialize)]
struct ExtractOutput {
    archive: String,
    output_dir: String,
    extracted: usize,
    errors: usize,
    files: Vec<ExtractedFile>,
}

#[derive(Serialize)]
struct ExtractedFile {
    filename: String,
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

fn matches_filter(
    filename: &str,
    pattern: Option<&glob::Pattern>,
    specific_files: &[String],
) -> bool {
    let normalized = filename.replace('\\', "/");
    if specific_files.is_empty() {
        pattern.is_none_or(|candidate| candidate.matches(&normalized))
    } else {
        specific_files.iter().any(|candidate| {
            let candidate = candidate.replace('\\', "/");
            normalized == candidate || normalized.ends_with(&format!("/{candidate}"))
        })
    }
}

fn extract_entry(
    archive: &mut ArchiveReader,
    index: usize,
    filename: &str,
    outdir: &Path,
    json: bool,
    quiet: bool,
    files: &mut Vec<ExtractedFile>,
) -> bool {
    let data = match archive.read_entry(index) {
        Ok(data) => data,
        Err(error) => {
            if json {
                files.push(ExtractedFile {
                    filename: filename.to_string(),
                    success: false,
                    error: Some(format!("Read error: {error}")),
                });
            } else if !quiet {
                eprintln!("  Error reading {filename}: {error}");
            }
            return false;
        }
    };

    let file_path = outdir.join(filename.replace('\\', "/"));
    if let Some(parent) = file_path.parent()
        && let Err(error) = fs::create_dir_all(parent)
    {
        if json {
            files.push(ExtractedFile {
                filename: filename.to_string(),
                success: false,
                error: Some(format!("Directory creation error: {error}")),
            });
        } else if !quiet {
            eprintln!("  Error creating directory for {filename}: {error}");
        }
        return false;
    }

    if let Err(error) = fs::write(&file_path, data) {
        if json {
            files.push(ExtractedFile {
                filename: filename.to_string(),
                success: false,
                error: Some(format!("Write error: {error}")),
            });
        } else if !quiet {
            eprintln!("  Error writing {filename}: {error}");
        }
        return false;
    }

    if json {
        files.push(ExtractedFile {
            filename: filename.to_string(),
            success: true,
            error: None,
        });
    } else if !quiet {
        println!("  {filename}");
    }
    true
}

fn extract_archive(
    path: &str,
    outdir: Option<&str>,
    filter: Option<&str>,
    specific_files: &[String],
    json: bool,
    quiet: bool,
) -> i32 {
    let mut archive = match open_archive(path) {
        Ok(a) => a,
        Err(e) => {
            if json {
                eprintln!(r#"{{"error": "{e}"}}"#);
            } else {
                eprintln!("Error opening archive: {e}");
            }
            return exit_code::FILE_NOT_FOUND;
        }
    };

    // Compile glob pattern if provided
    let pattern = filter.map(|f| {
        glob::Pattern::new(f).unwrap_or_else(|e| {
            eprintln!("Invalid glob pattern '{f}': {e}");
            std::process::exit(exit_code::ERROR);
        })
    });

    // Default output directory is the archive name without extension
    let default_outdir = Path::new(path)
        .file_stem()
        .map_or_else(|| "output".to_string(), |s| s.to_string_lossy().to_string());
    let outdir_str = outdir.unwrap_or(&default_outdir);
    let outdir = Path::new(outdir_str);

    // Create output directory
    if let Err(e) = fs::create_dir_all(outdir) {
        if json {
            eprintln!(r#"{{"error": "Failed to create output directory: {e}"}}"#);
        } else {
            eprintln!("Error creating output directory: {e}");
        }
        return exit_code::IO_ERROR;
    }

    if !quiet && !json {
        println!("Extracting {} to {}...", path, outdir.display());
    }

    let mut success = 0;
    let mut errors = 0;
    let mut files = Vec::new();

    // Skip entry 0 (filename table)
    for i in 1..archive.len() {
        let Some(entry) = archive.entry(i) else {
            continue;
        };
        let filename = entry.filename.as_deref().unwrap_or("<unnamed>").to_string();

        if !matches_filter(&filename, pattern.as_ref(), specific_files) {
            continue;
        }

        if extract_entry(&mut archive, i, &filename, outdir, json, quiet, &mut files) {
            success += 1;
        } else {
            errors += 1;
        }
    }

    if json {
        let output = ExtractOutput {
            archive: path.to_string(),
            output_dir: outdir_str.to_string(),
            extracted: success,
            errors,
            files,
        };
        println!("{}", serde_json::to_string(&output).unwrap());
    } else if !quiet {
        println!();
        println!("Extracted {success} files ({errors} errors)");
    }

    if errors > 0 {
        exit_code::PARTIAL_FAILURE
    } else {
        exit_code::SUCCESS
    }
}

/// JSON output for create command
#[derive(Serialize)]
struct CreateOutput {
    archive: String,
    input_dir: String,
    files_added: usize,
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

fn create_archive(output_path: &str, input_dir: &str, json: bool, quiet: bool) -> i32 {
    let input_path = Path::new(input_dir);

    if !input_path.is_dir() {
        if json {
            let output = CreateOutput {
                archive: output_path.to_string(),
                input_dir: input_dir.to_string(),
                files_added: 0,
                success: false,
                error: Some(format!("{input_dir} is not a directory")),
            };
            println!("{}", serde_json::to_string(&output).unwrap());
        } else {
            eprintln!("Error: {input_dir} is not a directory");
        }
        return exit_code::FILE_NOT_FOUND;
    }

    if !quiet && !json {
        println!("Creating {output_path} from {input_dir}...");
    }

    let mut writer = Writer::new();

    // Recursively collect files
    let mut file_count = 0;
    if let Err(e) = collect_files(input_path, input_path, &mut writer, &mut file_count) {
        if json {
            let output = CreateOutput {
                archive: output_path.to_string(),
                input_dir: input_dir.to_string(),
                files_added: file_count,
                success: false,
                error: Some(format!("Error collecting files: {e}")),
            };
            println!("{}", serde_json::to_string(&output).unwrap());
        } else {
            eprintln!("Error collecting files: {e}");
        }
        return exit_code::IO_ERROR;
    }

    if !quiet && !json {
        println!("  Collected {file_count} files");
    }

    // Build, encrypt, and stream directly to file
    let write_result: Result<(), String> = (|| {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(output_path)
            .map_err(|e| format!("Failed to create {output_path}: {e}"))?;
        let keys = TeaKeys::default_archive_keys();
        writer
            .write_to_encrypted(file, keys)
            .map_err(|e| format!("Error writing archive: {e}"))?;
        Ok(())
    })();

    if let Err(e) = write_result {
        if json {
            let output = CreateOutput {
                archive: output_path.to_string(),
                input_dir: input_dir.to_string(),
                files_added: file_count,
                success: false,
                error: Some(e.clone()),
            };
            println!("{}", serde_json::to_string(&output).unwrap());
        } else {
            eprintln!("{e}");
        }
        return exit_code::IO_ERROR;
    }

    if json {
        let output = CreateOutput {
            archive: output_path.to_string(),
            input_dir: input_dir.to_string(),
            files_added: file_count,
            success: true,
            error: None,
        };
        println!("{}", serde_json::to_string(&output).unwrap());
    } else if !quiet {
        println!("Done!");
    }

    exit_code::SUCCESS
}

fn collect_files(
    base: &Path,
    dir: &Path,
    writer: &mut Writer,
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

fn verify_archive(path: &str, key_hex: &str, json: bool) -> i32 {
    // Parse hex key
    let key_bytes = match hex::decode(key_hex) {
        Ok(b) if b.len() == 20 => {
            let mut key = [0u8; 20];
            key.copy_from_slice(&b);
            key
        }
        Ok(b) => {
            eprintln!(
                "Error: key must be 20 bytes (40 hex chars), got {} bytes",
                b.len()
            );
            return exit_code::ERROR;
        }
        Err(e) => {
            eprintln!("Error: invalid hex key: {e}");
            return exit_code::ERROR;
        }
    };

    let archive = match open_archive(path) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Error: {e}");
            return exit_code::FILE_NOT_FOUND;
        }
    };

    if !archive.has_signature() {
        if json {
            println!(r#"{{"file": "{path}", "signed": false}}"#);
        } else {
            println!("Archive is not signed.");
        }
        return exit_code::ERROR;
    }

    let hash = archive.header_hash();

    match archive.verify_signature_with_key(&key_bytes) {
        Ok(true) => {
            if json {
                println!(
                    r#"{{"file": "{path}", "signed": true, "valid": true, "header_hash": "{}"}}"#,
                    hex::encode(hash)
                );
            } else {
                println!("Signature VALID");
                println!("  Header hash: {}", hex::encode(hash));
                println!("  Signature:   {} bytes", archive.signature().len());
            }
            exit_code::SUCCESS
        }
        Ok(false) => {
            if json {
                println!(
                    r#"{{"file": "{path}", "signed": true, "valid": false, "header_hash": "{}"}}"#,
                    hex::encode(hash)
                );
            } else {
                println!("Signature INVALID");
                println!("  Header hash: {}", hex::encode(hash));
            }
            exit_code::ERROR
        }
        Err(e) => {
            eprintln!("Error verifying signature: {e}");
            exit_code::ERROR
        }
    }
}
