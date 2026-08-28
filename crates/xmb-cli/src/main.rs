//! XMB CLI - Convert between XMB and XML formats.

use clap::{Parser, Subcommand, ValueEnum};
use nostdio::{Cursor, Endian, ReadEndian, ReadLe, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use xmb::{Document, Format, Node, Reader, Writer};

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

impl From<FormatArg> for Format {
    fn from(arg: FormatArg) -> Self {
        match arg {
            FormatArg::Pc => Format::PC,
            FormatArg::Xbox360 => Format::Xbox360,
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // If files are provided without a subcommand, use drag-and-drop mode
    if cli.command.is_none() && !cli.files.is_empty() {
        process_files(&cli.files, cli.format, cli.overwrite, !cli.no_compress);
        return Ok(());
    }

    match cli.command {
        Some(Commands::ToXml { input, output }) => {
            let output = output.unwrap_or_else(|| {
                let mut p = input.clone();
                p.set_extension("xml");
                p
            });

            println!("Converting {} -> {}", input.display(), output.display());

            let data = std::fs::read(&input)?;
            let doc = Reader::read(&data)?;

            let xml = doc.to_xml();
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
            let doc = Document::from_xml(&xml)?;

            let bytes = Writer::write_with_options(&doc, format.into(), compress)?;
            std::fs::write(&output, bytes)?;

            println!("Done!");
        }

        Some(Commands::Info { input }) => {
            let data = std::fs::read(&input)?;
            print_info(&input, &data)?;
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

fn print_deflate_stream(raw: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    if raw.len() < ecf::deflate_stream::HEADER_SIZE {
        return Ok(());
    }

    let mut cursor = Cursor::new(raw);
    let signature = cursor.read_u32_le()?;
    let endian = if signature == ecf::deflate_stream::SIGNATURE_INVERTED {
        Endian::Big
    } else {
        Endian::Little
    };
    cursor.seek(SeekFrom::Start(12))?;
    let source_bytes = cursor.read_u64(endian)?;
    let compressed_bytes = cursor.read_u64(endian)?;
    let source_adler = cursor.read_u32(endian)?;
    let compressed_adler = cursor.read_u32(endian)?;

    println!();
    println!("  --- BDeflateStream ---");
    println!(
        "  signature:      0x{signature:08X} ({})",
        if endian == Endian::Big {
            "big-endian"
        } else {
            "little-endian"
        }
    );
    println!("  src_bytes:      {source_bytes} (decompressed)");
    println!("  dst_bytes:      {compressed_bytes} (compressed)");
    println!("  src_adler32:    0x{source_adler:08X}");
    println!("  dst_adler32:    0x{compressed_adler:08X}");
    if source_bytes > 0 {
        let ratio_tenths = u128::from(compressed_bytes) * 1000 / u128::from(source_bytes);
        println!(
            "  ratio:          {}.{}%",
            ratio_tenths / 10,
            ratio_tenths % 10
        );
    }
    Ok(())
}

fn count_nodes(node: &Node) -> usize {
    1 + node.children.iter().map(count_nodes).sum::<usize>()
}

fn print_decompressed_payload(file_data: &[u8], decompressed: &[u8]) {
    println!();
    println!("  --- Decompressed payload ---");
    println!("  size: {} bytes", decompressed.len());
    if decompressed.len() >= 4 {
        let mut cursor = Cursor::new(decompressed);
        let signature = cursor.read_u32_le().expect("length checked above");
        println!("  xmb_signature:  0x{signature:08X} (LE)");
        if signature == xmb::SIGNATURE {
            println!("  format:         PC (little-endian BDT, 48-byte nodes)");
        } else if signature.swap_bytes() == xmb::SIGNATURE {
            println!("  format:         Xbox 360 (big-endian BDT, 28-byte nodes)");
        }
    }
    if let Ok(doc) = Reader::read(file_data) {
        println!("  bdt_format:     {:?}", doc.format());
        if let Some(root) = doc.root() {
            println!("  root_element:   <{}>", root.name);
            println!("  root_attrs:     {}", root.attributes.len());
            println!("  root_children:  {}", root.children.len());
            println!("  total_nodes:    {}", count_nodes(root));
        }
    }
}

/// Print detailed info about an XMB file: ECF header, chunk metadata,
/// `BDeflateStream` compression details, and BDT format.
fn print_info(path: &Path, data: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    println!("File: {}", path.display());
    println!("Size: {} bytes", data.len());
    println!();

    // --- ECF layer ---
    let ecf = ecf::Reader::new(data)?;
    let hdr = ecf.header();
    println!("=== ECF Header ===");
    println!("  magic:       0x{:08X}", hdr.magic);
    println!("  header_size: {} bytes", hdr.header_size);
    println!("  file_size:   {} bytes", hdr.file_size);
    println!("  adler32:     0x{:08X}", hdr.adler32);
    println!("  num_chunks:  {}", hdr.num_chunks);
    println!("  flags:       0x{:04X}", hdr.flags);
    println!("  id:          0x{:08X}", hdr.id);
    println!("  chunk_extra: {} bytes", hdr.chunk_extra_data_size);
    println!();

    for (i, ch) in ecf.chunks().iter().enumerate() {
        println!("=== Chunk {i} ===");
        println!("  id:             0x{:016X}", ch.id);
        println!("  offset:         {}", ch.offset);
        println!("  size:           {} bytes", ch.size);
        println!("  adler32:        0x{:08X}", ch.adler32);
        println!("  flags:          0x{:02X}", ch.flags);
        println!(
            "  alignment:      {} (log2={})",
            ch.alignment(),
            ch.alignment_log2
        );
        println!("  resource_flags: 0x{:04X}", ch.resource_flags);

        let is_compressed = (ch.resource_flags & ecf::resource_flags::IS_DEFLATE_STREAM) != 0;
        println!("  compressed:     {is_compressed}");

        if is_compressed {
            let raw = ecf.raw_chunk_data(i)?;
            print_deflate_stream(raw)?;
        }

        // Decompress and inspect XMB/BDT payload
        match ecf.chunk_data(i) {
            Ok(decompressed) => {
                print_decompressed_payload(data, &decompressed);
            }
            Err(e) => {
                println!("  decompress error: {e}");
            }
        }
        println!();
    }

    Ok(())
}

/// Generate an output path by appending the new extension.
/// If `overwrite` is false and the file exists, adds "_1", "_2", etc.
fn output_path(base: &Path, new_ext: &str, overwrite: bool) -> PathBuf {
    let base_name = base.as_os_str().to_string_lossy();
    let output_name = format!("{base_name}.{new_ext}");
    let output = PathBuf::from(&output_name);

    if overwrite || !output.exists() {
        return output;
    }

    for i in 1..1000 {
        let candidate = PathBuf::from(format!("{base_name}_{i}.{new_ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }

    output
}

/// Process files in drag-and-drop mode.
fn process_files(files: &[PathBuf], format: FormatArg, overwrite: bool, compress: bool) {
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
    println!("Done! {success_count} converted, {error_count} errors");

    if error_count > 0 {
        std::process::exit(1);
    }
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
    let doc = Document::from_xml(&xml)?;

    let bytes = Writer::write_with_options(&doc, format.into(), compress)?;
    std::fs::write(&output, bytes)?;

    Ok(output)
}

/// Convert an XMB file to XML.
fn convert_xmb_to_xml(
    input: &PathBuf,
    overwrite: bool,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let output = output_path(input, "xml", overwrite);

    let data = std::fs::read(input)?;
    let doc = Reader::read(&data)?;

    let xml = doc.to_xml();
    std::fs::write(&output, xml)?;

    Ok(output)
}
