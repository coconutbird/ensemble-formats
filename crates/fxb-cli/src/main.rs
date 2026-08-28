//! Command-line inspection and conversion tools for FXB shader files.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

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

fn read_fxb(path: &std::path::Path) -> Option<(&'static [u8], fxb::FxbFile<'static>)> {
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{}: {e}", path.display());
            return None;
        }
    };
    // Leak the data so the FxbFile can borrow it for 'static.
    let leaked: &'static [u8] = Vec::leak(data);
    match fxb::parse(leaked) {
        Ok(f) => Some((leaked, f)),
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
        let Some((_, fxb)) = read_fxb(path) else {
            continue;
        };
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
}

fn cmd_info(files: &[PathBuf], disasm: bool) {
    if files.is_empty() {
        eprintln!("No files specified.");
        std::process::exit(1);
    }
    for path in files {
        let Some((_, fxb)) = read_fxb(path) else {
            continue;
        };
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        println!("=== {name} ===");

        for (i, e) in fxb.entries.iter().enumerate() {
            println!(
                "\n-- [{i}] {} (offset 0x{:06X}, {} bytes) --",
                e.name, e.dxbc_offset, e.dxbc_size
            );
            if let Some(ref shader) = e.shader {
                print_shader_info(shader, disasm);
            }
        }
        println!();
    }
}

fn cmd_extract(file: &std::path::Path, output: Option<&std::path::Path>) {
    let Some((_, fxb)) = read_fxb(file) else {
        std::process::exit(1);
    };
    let out_dir = output.unwrap_or_else(|| std::path::Path::new("."));
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

fn print_shader_info(shader: &d3dasm::Shader<'_>, disasm: bool) {
    if let Some(prog) = shader.program() {
        println!("  SM {}.{}", prog.major_version, prog.minor_version);
    }
    if let Some(rd) = shader.resource_def() {
        if !rd.creator.is_empty() {
            println!("  compiler: {}", rd.creator);
        }
        for cb in &rd.constant_buffers {
            println!("\n  cbuffer {} ({} bytes):", cb.name, cb.size);
            for v in &cb.variables {
                println!("    +{:<4} {:<32} ({} bytes)", v.offset, v.name, v.size);
            }
        }
        if !rd.bindings.is_empty() {
            println!("\n  bindings:");
            for b in &rd.bindings {
                println!("    {b}");
            }
        }
    }
    if let Some(sig) = shader.input_signature() {
        println!("\n  inputs:");
        for elem in &sig.elements {
            println!("    {elem}");
        }
    }
    if let Some(sig) = shader.output_signature() {
        println!("\n  outputs:");
        for elem in &sig.elements {
            println!("    {elem}");
        }
    }
    if disasm {
        println!("\n  --- disassembly ---");
        print!("{shader}");
    }
}
