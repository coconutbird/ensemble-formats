//! Command-line inspection and conversion tools for FXB shader files.

use clap::{Parser, Subcommand};
use cli_support::shader::{ShaderInfoOptions, write_shader_info};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(name = "fxb", about = "Inspect and extract HWDE FXB shader bundles")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List all shader entries in an FXB file.
    List {
        /// FXB file(s) to inspect.
        files: Vec<PathBuf>,
    },
    /// Show detailed info (bindings, signatures, constant buffers) for each shader.
    Info {
        /// FXB file(s) to inspect.
        files: Vec<PathBuf>,
        /// Show full disassembly for each shader.
        #[arg(short, long)]
        disasm: bool,
    },
    /// Extract individual DXBC blobs to a directory.
    Extract {
        /// FXB file to extract from.
        file: PathBuf,
        /// Output directory (default: current directory).
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::List { files } => cmd_list(&files),
        Command::Info { files, disasm } => cmd_info(&files, disasm),
        Command::Extract { file, output } => cmd_extract(&file, output.as_deref()),
    }
}

/// Reads and parses one FXB file, then calls `f` with the parsed file.
///
/// The input buffer lives only for the call. If the read or the parse fails,
/// the function prints the error and returns `None`.
fn with_fxb<T>(path: &Path, f: impl FnOnce(&fxb::FxbFile<'_>) -> T) -> Option<T> {
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{}: {e}", path.display());
            return None;
        }
    };
    match fxb::parse(&data) {
        Ok(parsed) => Some(f(&parsed)),
        Err(e) => {
            eprintln!("{}: {e}", path.display());
            None
        }
    }
}

fn cmd_list(files: &[PathBuf]) {
    if files.is_empty() {
        eprintln!("No files specified.");
        std::process::exit(1);
    }
    for path in files {
        with_fxb(path, |fxb| print_list(path, fxb));
    }
}

fn print_list(path: &Path, fxb: &fxb::FxbFile<'_>) {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    println!("=== {name} ===");
    println!(
        "  version: {}  name_len: {}  entries: {}",
        fxb.version,
        fxb.name_len,
        fxb.entries.len()
    );
    for (i, e) in fxb.entries.iter().enumerate() {
        let sm = e
            .shader
            .as_ref()
            .and_then(|s| s.program())
            .map(|p| format!("SM {}.{}", p.major_version, p.minor_version));
        println!(
            "  [{i:3}] {:<14} {:>6} bytes @ 0x{:06X}  ann={:<5} {}",
            e.name,
            e.dxbc_size,
            e.dxbc_offset,
            e.annotation.len(),
            sm.unwrap_or_default()
        );
    }
    println!();
}

fn cmd_info(files: &[PathBuf], disasm: bool) {
    if files.is_empty() {
        eprintln!("No files specified.");
        std::process::exit(1);
    }
    let options = ShaderInfoOptions {
        variable_types: false,
        constant_buffers_only: false,
        disassembly: disasm,
    };
    for path in files {
        with_fxb(path, |fxb| print_info(path, fxb, options));
    }
}

fn print_info(path: &Path, fxb: &fxb::FxbFile<'_>, options: ShaderInfoOptions) {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    println!("=== {name} ===");

    for (i, e) in fxb.entries.iter().enumerate() {
        println!(
            "\n-- [{i}] {} (offset 0x{:06X}, {} bytes) --",
            e.name, e.dxbc_offset, e.dxbc_size
        );
        if let Some(shader) = &e.shader {
            write_shader_info(&mut std::io::stdout().lock(), shader, options)
                .expect("failed printing to stdout");
        }
    }
    println!();
}

fn cmd_extract(file: &Path, output: Option<&Path>) {
    if with_fxb(file, |fxb| extract_entries(file, fxb, output)).is_none() {
        std::process::exit(1);
    }
}

fn extract_entries(file: &Path, fxb: &fxb::FxbFile<'_>, output: Option<&Path>) {
    let out_dir = output.unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(out_dir).unwrap();

    let stem = file.file_stem().unwrap_or_default().to_string_lossy();
    for (i, e) in fxb.entries.iter().enumerate() {
        // Sanitize name: strip NUL bytes and spaces that would break file paths.
        let safe_name: String = e.name.replace([' ', '\0'], "");
        let safe_name = if safe_name.is_empty() {
            format!("entry{i}")
        } else {
            safe_name
        };
        let fname = format!("{stem}_{i:03}_{safe_name}.dxbc");
        let out_path = out_dir.join(&fname);
        std::fs::write(&out_path, e.dxbc_data).unwrap();
        println!("  {fname} ({} bytes)", e.dxbc_data.len());
    }
}
