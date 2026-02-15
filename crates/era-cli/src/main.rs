//! ERA CLI - Tool for working with ERA archives from Halo Wars.

use std::fs;
use std::path::Path;

use clap::{Parser, Subcommand};
use era::{EraArchive, EraWriter};
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
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Format a byte size as a human-readable string
fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
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

fn list_archive(path: &str, json: bool) -> i32 {
    let archive = match EraArchive::open(path) {
        Ok(a) => a,
        Err(e) => {
            if json {
                eprintln!(r#"{{"error": "{}"}}"#, e);
            } else {
                eprintln!("Error opening archive: {}", e);
            }
            return exit_code::FILE_NOT_FOUND;
        }
    };

    let entries: Vec<ListEntry> = archive
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let comp = entry.compressed_size();
            let decomp = entry.decompressed_size();
            let ratio = if decomp > 0 {
                (comp as f64 / decomp as f64) * 100.0
            } else {
                100.0
            };
            // Index 0 is always the filename table (no filename of its own)
            let filename = if i == 0 {
                Some("<filename table>".to_string())
            } else {
                entry.filename.clone()
            };
            ListEntry {
                index: i,
                filename,
                compressed_size: comp,
                decompressed_size: decomp,
                compression_ratio: ratio,
                tiger128: bytes_to_hex(&entry.extra.comp_tiger128),
            }
        })
        .collect();

    if json {
        let output = ListOutput {
            archive: path.to_string(),
            entries,
            total_entries: archive.len(),
        };
        println!("{}", serde_json::to_string(&output).unwrap());
    } else {
        println!("Files in {}:", path);
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
                format_size(entry.compressed_size as u64),
                format_size(entry.decompressed_size as u64),
                entry.compression_ratio,
                name,
                entry.tiger128,
                width = max_name_len
            );
        }

        // Calculate totals
        let total_compressed: u64 = entries.iter().map(|e| e.compressed_size as u64).sum();
        let total_decompressed: u64 = entries.iter().map(|e| e.decompressed_size as u64).sum();
        let total_ratio = if total_decompressed > 0 {
            (total_compressed as f64 / total_decompressed as f64) * 100.0
        } else {
            100.0
        };

        println!();
        println!(
            "Total: {} entries, {} -> {} ({:.1}%)",
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
    ecf_header: EcfHeaderInfo,
    archive_header: ArchiveHeaderInfo,
    total_entries: usize,
}

#[derive(Serialize)]
struct EcfHeaderInfo {
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
    let archive = match EraArchive::open(path) {
        Ok(a) => a,
        Err(e) => {
            if json {
                eprintln!(r#"{{"error": "{}"}}"#, e);
            } else {
                eprintln!("Error opening archive: {}", e);
            }
            return exit_code::FILE_NOT_FOUND;
        }
    };

    if json {
        let output = InfoOutput {
            archive: path.to_string(),
            ecf_header: EcfHeaderInfo {
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
            total_compressed += entry.compressed_size() as u64;
            total_decompressed += entry.decompressed_size() as u64;
        }
        let file_count = archive.len().saturating_sub(1); // exclude filename table
        let ratio = if total_decompressed > 0 {
            (total_compressed as f64 / total_decompressed as f64) * 100.0
        } else {
            100.0
        };

        println!("Archive: {}", path);
        println!();
        println!("ECF Header");
        println!("  Magic              0x{:08X}", archive.ecf_header.magic);
        println!(
            "  Header Size        {}",
            format_size(archive.ecf_header.header_size as u64)
        );
        println!(
            "  File Size          {}",
            format_size(archive.ecf_header.file_size as u64)
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
            format_size(archive.archive_header.signature_size as u64)
        );
        println!();
        println!("Summary");
        println!("  Files              {}", file_count);
        println!("  Compressed         {}", format_size(total_compressed));
        println!("  Decompressed       {}", format_size(total_decompressed));
        println!("  Ratio              {:.1}%", ratio);
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

fn extract_archive(
    path: &str,
    outdir: Option<&str>,
    filter: Option<&str>,
    specific_files: &[String],
    json: bool,
    quiet: bool,
) -> i32 {
    let mut archive = match EraArchive::open(path) {
        Ok(a) => a,
        Err(e) => {
            if json {
                eprintln!(r#"{{"error": "{}"}}"#, e);
            } else {
                eprintln!("Error opening archive: {}", e);
            }
            return exit_code::FILE_NOT_FOUND;
        }
    };

    // Compile glob pattern if provided
    let pattern = filter.map(|f| {
        glob::Pattern::new(f).unwrap_or_else(|e| {
            eprintln!("Invalid glob pattern '{}': {}", f, e);
            std::process::exit(exit_code::ERROR);
        })
    });

    // Default output directory is the archive name without extension
    let default_outdir = Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "output".to_string());
    let outdir_str = outdir.unwrap_or(&default_outdir);
    let outdir = Path::new(outdir_str);

    // Create output directory
    if let Err(e) = fs::create_dir_all(outdir) {
        if json {
            eprintln!(r#"{{"error": "Failed to create output directory: {}"}}"#, e);
        } else {
            eprintln!("Error creating output directory: {}", e);
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
        let entry = archive.entry(i).unwrap().clone();
        let filename = entry.filename.as_deref().unwrap_or("<unnamed>").to_string();

        // Normalize filename for matching (use forward slashes)
        let normalized = filename.replace('\\', "/");

        // Check if file matches filter criteria
        let matches = if !specific_files.is_empty() {
            // Check against specific file list
            specific_files.iter().any(|f| {
                let f_normalized = f.replace('\\', "/");
                normalized == f_normalized || normalized.ends_with(&format!("/{}", f_normalized))
            })
        } else if let Some(ref pat) = pattern {
            // Check against glob pattern
            pat.matches(&normalized)
        } else {
            // No filter, extract all
            true
        };

        if !matches {
            continue;
        }

        // Read entry data
        let data = match archive.read_entry(i) {
            Ok(d) => d,
            Err(e) => {
                if json {
                    files.push(ExtractedFile {
                        filename,
                        success: false,
                        error: Some(format!("Read error: {}", e)),
                    });
                } else if !quiet {
                    eprintln!("  Error reading {}: {}", filename, e);
                }
                errors += 1;
                continue;
            }
        };

        // Build output path
        let file_path = outdir.join(filename.replace('\\', "/"));

        // Create parent directories
        if let Some(parent) = file_path.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                if json {
                    files.push(ExtractedFile {
                        filename,
                        success: false,
                        error: Some(format!("Directory creation error: {}", e)),
                    });
                } else if !quiet {
                    eprintln!("  Error creating directory for {}: {}", filename, e);
                }
                errors += 1;
                continue;
            }
        }

        // Write file
        if let Err(e) = fs::write(&file_path, &data) {
            if json {
                files.push(ExtractedFile {
                    filename,
                    success: false,
                    error: Some(format!("Write error: {}", e)),
                });
            } else if !quiet {
                eprintln!("  Error writing {}: {}", filename, e);
            }
            errors += 1;
            continue;
        }

        if json {
            files.push(ExtractedFile {
                filename,
                success: true,
                error: None,
            });
        } else if !quiet {
            println!("  {}", filename);
        }
        success += 1;
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
        println!("Extracted {} files ({} errors)", success, errors);
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
                error: Some(format!("{} is not a directory", input_dir)),
            };
            println!("{}", serde_json::to_string(&output).unwrap());
        } else {
            eprintln!("Error: {} is not a directory", input_dir);
        }
        return exit_code::FILE_NOT_FOUND;
    }

    if !quiet && !json {
        println!("Creating {} from {}...", output_path, input_dir);
    }

    let mut writer = EraWriter::new();

    // Recursively collect files
    let mut file_count = 0;
    if let Err(e) = collect_files(input_path, input_path, &mut writer, &mut file_count) {
        if json {
            let output = CreateOutput {
                archive: output_path.to_string(),
                input_dir: input_dir.to_string(),
                files_added: file_count,
                success: false,
                error: Some(format!("Error collecting files: {}", e)),
            };
            println!("{}", serde_json::to_string(&output).unwrap());
        } else {
            eprintln!("Error collecting files: {}", e);
        }
        return exit_code::IO_ERROR;
    }

    if !quiet && !json {
        println!("  Collected {} files", file_count);
    }

    // Write archive
    if let Err(e) = writer.write_to_file(output_path) {
        if json {
            let output = CreateOutput {
                archive: output_path.to_string(),
                input_dir: input_dir.to_string(),
                files_added: file_count,
                success: false,
                error: Some(format!("Error writing archive: {}", e)),
            };
            println!("{}", serde_json::to_string(&output).unwrap());
        } else {
            eprintln!("Error writing archive: {}", e);
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
