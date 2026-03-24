//! PKG CLI — Tool for working with Halo Wars 2 PKG archives.

use std::fs;
use std::io::BufReader;
use std::path::Path;

use clap::{Parser, Subcommand};
use serde::Serialize;

/// Exit codes for scripting.
pub mod exit_code {
    pub const SUCCESS: i32 = 0;
    pub const ERROR: i32 = 1;
    pub const FILE_NOT_FOUND: i32 = 2;
    pub const INVALID_FORMAT: i32 = 3;
    pub const IO_ERROR: i32 = 4;
    pub const PARTIAL_FAILURE: i32 = 5;
}

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
#[command(name = "pkg")]
#[command(author, version, about = "PKG archive tool for Halo Wars 2", long_about = None)]
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
    /// List files in a PKG archive
    List {
        /// Path to the PKG archive
        file: String,
    },
    /// Show archive information
    Info {
        /// Path to the PKG archive
        file: String,
    },
    /// Extract files from a PKG archive
    Extract {
        /// Path to the PKG archive
        file: String,
        /// Output directory (defaults to archive name without extension)
        #[arg(short, long)]
        output: Option<String>,
        /// Unix shell style glob pattern to filter files (e.g., "*.fnt", "data/**/*.xml")
        #[arg(short, long)]
        filter: Option<String>,
        /// Specific files to extract
        #[arg(trailing_var_arg = true)]
        files: Vec<String>,
    },
    /// Create a PKG archive from a directory
    Create {
        /// Output PKG file path
        output: String,
        /// Input directory containing files to archive
        input: String,
        /// PKG format version (1 or 2)
        #[arg(long, default_value = "2")]
        version: u64,
        /// Data alignment in bytes (version 2+ only)
        #[arg(long, default_value = "4096")]
        alignment: u64,
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
        Commands::Create {
            output,
            input,
            version,
            alignment,
        } => create_archive(output, input, *version, *alignment, cli.json, cli.quiet),
    };
    std::process::exit(exit_code);
}

fn open_archive(path: &str) -> Result<pkg::Reader<BufReader<fs::File>>, String> {
    let file = fs::File::open(path).map_err(|e| format!("Failed to open {}: {}", path, e))?;
    let reader = BufReader::new(file);
    pkg::Reader::new(reader).map_err(|e| format!("Failed to parse PKG: {}", e))
}

// --- JSON output types ---

#[derive(Serialize)]
struct ListOutput {
    archive: String,
    entries: Vec<ListEntry>,
    total_entries: usize,
}

#[derive(Serialize)]
struct ListEntry {
    index: usize,
    filename: String,
    data_offset: u64,
    data_size: u64,
    name_hash: String,
}

#[derive(Serialize)]
struct InfoOutput {
    archive: String,
    version: u64,
    alignment: u64,
    data_section_offset: u64,
    total_entries: usize,
    total_data_size: u64,
}

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

fn err_exit(msg: &str, json: bool, code: i32) -> i32 {
    if json {
        eprintln!(r#"{{"error": "{}"}}"#, msg);
    } else {
        eprintln!("Error: {}", msg);
    }
    code
}

fn list_archive(path: &str, json: bool) -> i32 {
    let archive = match open_archive(path) {
        Ok(a) => a,
        Err(e) => return err_exit(&e, json, exit_code::FILE_NOT_FOUND),
    };

    let entries: Vec<ListEntry> = archive
        .entries()
        .iter()
        .enumerate()
        .map(|(i, e)| ListEntry {
            index: i,
            filename: e.filename.clone(),
            data_offset: e.data_offset,
            data_size: e.data_size,
            name_hash: format!("{:016X}", e.name_hash),
        })
        .collect();

    if json {
        let output = ListOutput {
            archive: path.to_string(),
            entries,
            total_entries: archive.entry_count(),
        };
        println!("{}", serde_json::to_string(&output).unwrap());
    } else {
        println!("Files in {}:", path);
        println!();

        let max_name_len = entries
            .iter()
            .map(|e| e.filename.len())
            .max()
            .unwrap_or(8)
            .max(8);

        println!(
            "{:>5}  {:>10}  {:>10}  {:<width$}  {:>16}",
            "Index",
            "Offset",
            "Size",
            "Filename",
            "FNV-1a Hash",
            width = max_name_len
        );
        println!(
            "{:->5}  {:->10}  {:->10}  {:-<width$}  {:->16}",
            "",
            "",
            "",
            "",
            "",
            width = max_name_len
        );

        for entry in &entries {
            println!(
                "{:>5}  {:>10}  {:>10}  {:<width$}  {}",
                entry.index,
                format!("0x{:X}", entry.data_offset),
                format_size(entry.data_size),
                entry.filename,
                entry.name_hash,
                width = max_name_len
            );
        }

        let total_size: u64 = entries.iter().map(|e| e.data_size).sum();
        println!();
        println!(
            "Total: {} entries, {}",
            archive.entry_count(),
            format_size(total_size)
        );
    }

    exit_code::SUCCESS
}

fn info_archive(path: &str, json: bool) -> i32 {
    let archive = match open_archive(path) {
        Ok(a) => a,
        Err(e) => return err_exit(&e, json, exit_code::FILE_NOT_FOUND),
    };

    let total_data_size: u64 = archive.entries().iter().map(|e| e.data_size).sum();

    if json {
        let output = InfoOutput {
            archive: path.to_string(),
            version: archive.version(),
            alignment: archive.alignment(),
            data_section_offset: archive.data_section_offset(),
            total_entries: archive.entry_count(),
            total_data_size,
        };
        println!("{}", serde_json::to_string(&output).unwrap());
    } else {
        println!("Archive: {}", path);
        println!();
        println!("  Magic              capack");
        println!("  Version            {}", archive.version());
        println!("  Alignment          {}", archive.alignment());
        println!("  Data offset        0x{:X}", archive.data_section_offset());
        println!("  Entries            {}", archive.entry_count());
        println!("  Total data         {}", format_size(total_data_size));
    }

    exit_code::SUCCESS
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
        Err(e) => return err_exit(&e, json, exit_code::FILE_NOT_FOUND),
    };

    let pattern = filter.map(|f| {
        glob::Pattern::new(f).unwrap_or_else(|e| {
            eprintln!("Invalid glob pattern '{}': {}", f, e);
            std::process::exit(exit_code::ERROR);
        })
    });

    let default_outdir = Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "output".to_string());
    let outdir_str = outdir.unwrap_or(&default_outdir);
    let outdir = Path::new(outdir_str);

    if let Err(e) = fs::create_dir_all(outdir) {
        return err_exit(
            &format!("Failed to create output directory: {}", e),
            json,
            exit_code::IO_ERROR,
        );
    }

    if !quiet && !json {
        println!("Extracting {} to {}...", path, outdir.display());
    }

    let mut success = 0;
    let mut errors = 0;
    let mut files = Vec::new();

    // Collect indices and filenames first (entries() borrows immutably).
    let to_extract: Vec<(usize, String)> = archive
        .entries()
        .iter()
        .enumerate()
        .filter_map(|(i, entry)| {
            let normalized = entry.filename.replace('\\', "/");
            let matches = if !specific_files.is_empty() {
                specific_files.iter().any(|f| {
                    let f_normalized = f.replace('\\', "/");
                    normalized == f_normalized
                        || normalized.ends_with(&format!("/{}", f_normalized))
                })
            } else if let Some(ref pat) = pattern {
                pat.matches(&normalized)
            } else {
                true
            };
            if matches { Some((i, normalized)) } else { None }
        })
        .collect();

    for (index, normalized) in &to_extract {
        let filename = archive.entries()[*index].filename.clone();

        let data = match archive.read_entry(*index) {
            Ok(d) => d,
            Err(e) => {
                if json {
                    files.push(ExtractedFile {
                        filename,
                        success: false,
                        error: Some(format!("{}", e)),
                    });
                } else if !quiet {
                    eprintln!("  Error reading {}: {}", normalized, e);
                }
                errors += 1;
                continue;
            }
        };

        let file_path = outdir.join(normalized);

        if let Some(parent) = file_path.parent()
            && let Err(e) = fs::create_dir_all(parent)
        {
            if json {
                files.push(ExtractedFile {
                    filename,
                    success: false,
                    error: Some(format!("mkdir: {}", e)),
                });
            } else if !quiet {
                eprintln!("  Error creating directory for {}: {}", normalized, e);
            }
            errors += 1;
            continue;
        }

        if let Err(e) = fs::write(&file_path, &data) {
            if json {
                files.push(ExtractedFile {
                    filename,
                    success: false,
                    error: Some(format!("write: {}", e)),
                });
            } else if !quiet {
                eprintln!("  Error writing {}: {}", normalized, e);
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
            println!("  {}", normalized);
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

#[derive(Serialize)]
struct CreateOutput {
    archive: String,
    input_dir: String,
    files_added: usize,
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

fn create_archive(
    output_path: &str,
    input_dir: &str,
    version: u64,
    alignment: u64,
    json: bool,
    quiet: bool,
) -> i32 {
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

    let mut writer = pkg::Writer::new();
    writer.set_version(version);
    if version >= 2 {
        writer.set_alignment(alignment);
    }

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

    let archive_bytes = match writer.finalize() {
        Ok(b) => b,
        Err(e) => {
            if json {
                let output = CreateOutput {
                    archive: output_path.to_string(),
                    input_dir: input_dir.to_string(),
                    files_added: file_count,
                    success: false,
                    error: Some(format!("Error building archive: {}", e)),
                };
                println!("{}", serde_json::to_string(&output).unwrap());
            } else {
                eprintln!("Error building archive: {}", e);
            }
            return exit_code::ERROR;
        }
    };

    if let Err(e) = fs::write(output_path, &archive_bytes) {
        if json {
            let output = CreateOutput {
                archive: output_path.to_string(),
                input_dir: input_dir.to_string(),
                files_added: file_count,
                success: false,
                error: Some(format!("Error writing file: {}", e)),
            };
            println!("{}", serde_json::to_string(&output).unwrap());
        } else {
            eprintln!("Error writing {}: {}", output_path, e);
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
        println!("Done! Wrote {}", format_size(archive_bytes.len() as u64));
    }

    exit_code::SUCCESS
}

fn collect_files(
    base: &Path,
    dir: &Path,
    writer: &mut pkg::Writer,
    count: &mut usize,
) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            collect_files(base, &path, writer, count)?;
        } else if path.is_file() {
            // Get relative path with backslashes (PKG convention).
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
