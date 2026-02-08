//! XMB CLI - Convert between XMB and XML formats.

use clap::{Parser, Subcommand, ValueEnum};
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::PathBuf;
use xmb::{Node, XmbData, XmbFormat, XmbReader, XmbWriter};

#[derive(Parser)]
#[command(name = "xmb")]
#[command(author, version, about = "XMB binary XML format converter for Halo Wars", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Files to convert (drag-and-drop mode). Auto-detects direction by extension.
    #[arg(global = true)]
    files: Vec<PathBuf>,

    /// Output format for XML to XMB conversion (drag-and-drop mode)
    #[arg(short, long, value_enum, default_value = "pc", global = true)]
    format: FormatArg,

    /// Overwrite existing files instead of adding _1, _2, etc.
    #[arg(short = 'w', long, global = true)]
    overwrite: bool,

    /// Disable compression when writing XMB files (compressed by default)
    #[arg(short = 'u', long = "no-compress", global = true)]
    no_compress: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Convert XMB to XML
    ToXml {
        /// Input XMB file
        #[arg(short, long)]
        input: PathBuf,

        /// Output XML file (defaults to input with .xml extension)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Convert XML to XMB
    ToXmb {
        /// Input XML file
        #[arg(short, long)]
        input: PathBuf,

        /// Output XMB file (defaults to input with .xmb extension)
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Output format
        #[arg(short, long, value_enum, default_value = "pc")]
        format: FormatArg,
    },

    /// Show information about an XMB file
    Info {
        /// Input XMB file
        #[arg(short, long)]
        input: PathBuf,
    },
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueEnum)]
enum FormatArg {
    /// PC/HWDE format (little-endian, 48-byte nodes)
    Pc,
    /// Xbox 360 format (big-endian, 28-byte nodes)
    Xbox360,
}

impl From<FormatArg> for XmbFormat {
    fn from(arg: FormatArg) -> Self {
        match arg {
            FormatArg::Pc => XmbFormat::PC,
            FormatArg::Xbox360 => XmbFormat::Xbox360,
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // If files are provided without a subcommand, use drag-and-drop mode
    if cli.command.is_none() && !cli.files.is_empty() {
        return process_files(&cli.files, cli.format, cli.overwrite, !cli.no_compress);
    }

    match cli.command {
        Some(Commands::ToXml { input, output }) => {
            let output = output.unwrap_or_else(|| {
                let mut p = input.clone();
                p.set_extension("xml");
                p
            });

            println!("Converting {} -> {}", input.display(), output.display());

            let file = File::open(&input)?;
            let reader = BufReader::new(file);
            let xmb = XmbReader::read(reader)?;

            let xml = xmb.to_xml();
            std::fs::write(&output, xml)?;

            println!("Done!");
        }

        Some(Commands::ToXmb {
            input,
            output,
            format,
        }) => {
            let output = output.unwrap_or_else(|| {
                let mut p = input.clone();
                p.set_extension("xmb");
                p
            });

            let compress = !cli.no_compress;
            println!(
                "Converting {} -> {} ({:?} format{})",
                input.display(),
                output.display(),
                format,
                if compress { ", compressed" } else { "" }
            );

            let xml = std::fs::read_to_string(&input)?;
            let xmb = XmbData::from_xml(&xml)?;

            let file = File::create(&output)?;
            let writer = BufWriter::new(file);
            XmbWriter::write_with_options(&xmb, writer, format.into(), compress)?;

            println!("Done!");
        }

        Some(Commands::Info { input }) => {
            let file = File::open(&input)?;
            let reader = BufReader::new(file);
            let xmb = XmbReader::read(reader)?;

            println!("File: {}", input.display());
            println!("Format: {:?}", xmb.format());

            if let Some(root) = xmb.root() {
                println!("Root element: <{}>", root.name);
                println!("Attributes: {}", root.attributes.len());
                println!("Children: {}", root.children.len());

                fn count_nodes(node: &Node) -> usize {
                    1 + node.children.iter().map(count_nodes).sum::<usize>()
                }
                println!("Total nodes: {}", count_nodes(root));
            } else {
                println!("(empty document)");
            }
        }

        None => {
            // No command and no files - show help
            eprintln!("No files provided. Use --help for usage information.");
            eprintln!();
            eprintln!("Drag and drop files onto the executable to convert them:");
            eprintln!("  .xml files -> .xmb (PC format by default)");
            eprintln!("  .xmb files -> .xml");
            std::process::exit(1);
        }
    }

    Ok(())
}

/// Generate an output path by appending the new extension.
/// If `overwrite` is false and the file exists, adds "_1", "_2", etc.
fn output_path(base: &PathBuf, new_ext: &str, overwrite: bool) -> PathBuf {
    let base_name = base.as_os_str().to_string_lossy();
    let output_name = format!("{}.{}", base_name, new_ext);
    let output = PathBuf::from(&output_name);

    if overwrite || !output.exists() {
        return output;
    }

    for i in 1..1000 {
        let candidate = PathBuf::from(format!("{}_{}.{}", base_name, i, new_ext));
        if !candidate.exists() {
            return candidate;
        }
    }

    output
}

/// Process files in drag-and-drop mode.
fn process_files(
    files: &[PathBuf],
    format: FormatArg,
    overwrite: bool,
    compress: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut success_count = 0;
    let mut error_count = 0;

    for file in files {
        let ext = file
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();

        let result = match ext.as_str() {
            "xml" => convert_xml_to_xmb(file, format, overwrite, compress),
            "xmb" => convert_xmb_to_xml(file, overwrite),
            _ => {
                eprintln!("Skipping {}: unknown extension", file.display());
                continue;
            }
        };

        match result {
            Ok(output) => {
                println!("Converted: {} -> {}", file.display(), output.display());
                success_count += 1;
            }
            Err(e) => {
                eprintln!("Error converting {}: {}", file.display(), e);
                error_count += 1;
            }
        }
    }

    println!();
    println!("Done! {} converted, {} errors", success_count, error_count);

    if error_count > 0 {
        std::process::exit(1);
    }

    Ok(())
}

/// Convert an XML file to XMB.
fn convert_xml_to_xmb(
    input: &PathBuf,
    format: FormatArg,
    overwrite: bool,
    compress: bool,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let output = output_path(input, "xmb", overwrite);

    let xml = std::fs::read_to_string(input)?;
    let xmb = XmbData::from_xml(&xml)?;

    let file = File::create(&output)?;
    let writer = BufWriter::new(file);
    XmbWriter::write_with_options(&xmb, writer, format.into(), compress)?;

    Ok(output)
}

/// Convert an XMB file to XML.
fn convert_xmb_to_xml(
    input: &PathBuf,
    overwrite: bool,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let output = output_path(input, "xml", overwrite);

    let file = File::open(input)?;
    let reader = BufReader::new(file);
    let xmb = XmbReader::read(reader)?;

    let xml = xmb.to_xml();
    std::fs::write(&output, xml)?;

    Ok(output)
}
