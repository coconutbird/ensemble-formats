//! Command-line inspection and constant-buffer inference for UFX shader files.

use clap::{Args, Parser};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "ufx", about = "Inspect UFX compiled shader files")]
struct Cli {
    /// UFX file(s) to inspect.
    files: Vec<PathBuf>,

    #[command(flatten)]
    display: DisplayOptions,

    #[command(flatten)]
    analysis: AnalysisOptions,
}

#[derive(Args)]
struct DisplayOptions {
    /// Show full disassembly for each shader stage.
    #[arg(short, long)]
    disasm: bool,
    /// Only show constant buffer layouts (skip bindings/signatures).
    #[arg(short = 'c', long)]
    cb_only: bool,
    /// Decode bitflags from the filename and show enabled features.
    #[arg(short = 'f', long)]
    flags: bool,
}

#[derive(Args)]
struct AnalysisOptions {
    /// Infer semantic names for material CB parameters (cb8 PS, cb7 VS).
    #[arg(short = 'i', long)]
    infer_params: bool,
    /// Build a control-flow graph and print it as Graphviz DOT.
    #[arg(long)]
    cfg: bool,
    /// Batch inference: output one TSV line per file with hex flags and params.
    /// Format: `hex_flags`, a tab, then `slot.reg.comp=semantic,...`.
    #[arg(long)]
    batch_infer: bool,
}

fn print_batch_inference(name: &str, file: &ufx::UfxFile<'_>) {
    use ufx::cb_infer::{HOGAN_PS_SLOT, HOGAN_VS_SLOT};

    let flags = name
        .strip_suffix(".ufx")
        .and_then(|stem| stem.rsplit('_').next())
        .unwrap_or("?");
    let mut parameters = Vec::new();
    if let Some(program) = file.pixel_shaders.first().and_then(ufx::Shader::program) {
        parameters.extend(ufx::cb_infer::infer_cb_params(
            program,
            HOGAN_PS_SLOT,
            HOGAN_VS_SLOT,
        ));
    }
    if let Some(program) = file.vertex_shader.as_ref().and_then(ufx::Shader::program) {
        parameters.extend(
            ufx::cb_infer::infer_cb_params(program, HOGAN_PS_SLOT, HOGAN_VS_SLOT)
                .into_iter()
                .filter(|parameter| parameter.cb_slot == HOGAN_VS_SLOT),
        );
    }

    let rendered: Vec<String> = parameters
        .iter()
        .map(|parameter| {
            format!(
                "cb{}[{}].{}={}[{}]",
                parameter.cb_slot,
                parameter.reg_index,
                parameter.components,
                parameter.semantic,
                parameter.confidence
            )
        })
        .collect();
    println!("{}\t{}", flags, rendered.join(","));
}

fn print_flags(name: &str) {
    use ufx::cb_infer::bitflags::HoganFlags;

    let Some(flags) = HoganFlags::from_filename(name) else {
        println!("  (could not parse flags from filename)");
        return;
    };
    println!(
        "\n  -- Bitflags: {} --",
        ufx::cb_infer::bitflags::flags_summary(flags)
    );
    let features = flags.features();
    if features.is_empty() {
        println!("    (no known features decoded)");
        return;
    }
    for feature in &features {
        let requirements = if feature.requires_bits.is_empty() {
            String::new()
        } else {
            format!(" (requires bits {:?})", feature.requires_bits)
        };
        println!(
            "    bit {:2} | cb{} | {:<22} — {}{}",
            feature.bit, feature.cb_slot, feature.name, feature.description, requirements
        );
    }
}

fn print_parameters(file: &ufx::UfxFile<'_>) {
    use ufx::cb_infer::{HOGAN_PS_SLOT, HOGAN_VS_SLOT};

    if let Some(program) = file.pixel_shaders.first().and_then(ufx::Shader::program) {
        let parameters = ufx::cb_infer::infer_cb_params(program, HOGAN_PS_SLOT, HOGAN_VS_SLOT);
        if parameters.is_empty() {
            println!("  (no cb8/cb7 params detected)");
        } else {
            println!("\n  -- Inferred CB Parameters --");
            print_parameter_rows(parameters.iter());
        }
    }

    if let Some(program) = file.vertex_shader.as_ref().and_then(ufx::Shader::program) {
        let parameters = ufx::cb_infer::infer_cb_params(program, HOGAN_PS_SLOT, HOGAN_VS_SLOT);
        let vertex_parameters: Vec<_> = parameters
            .iter()
            .filter(|parameter| parameter.cb_slot == HOGAN_VS_SLOT)
            .collect();
        if !vertex_parameters.is_empty() {
            println!("\n  -- Inferred VS CB Parameters --");
            print_parameter_rows(vertex_parameters);
        }
    }
}

fn print_parameter_rows<'a>(parameters: impl IntoIterator<Item = &'a ufx::cb_infer::CbParam>) {
    for parameter in parameters {
        println!(
            "    cb{}[{}].{:<6} => {:<24} [{}] (insn #{})",
            parameter.cb_slot,
            parameter.reg_index,
            parameter.components,
            parameter.semantic,
            parameter.confidence,
            parameter.insn_index,
        );
    }
}

fn print_cfg(file: &ufx::UfxFile<'_>) {
    if let Some(program) = file.vertex_shader.as_ref().and_then(ufx::Shader::program) {
        match ufx::cb_infer::cfg_report(program) {
            Ok(report) => {
                println!(
                    "\n  -- Vertex Shader CFG ({} blocks, {} edges) --",
                    report.block_count, report.edge_count,
                );
                println!("{}", report.dot);
            }
            Err(error) => eprintln!("  VS CFG error: {error}"),
        }
    }

    for (index, shader) in file.pixel_shaders.iter().enumerate() {
        if let Some(program) = shader.program() {
            match ufx::cb_infer::cfg_report(program) {
                Ok(report) => {
                    println!(
                        "\n  -- Pixel Shader {index} CFG ({} blocks, {} edges) --",
                        report.block_count, report.edge_count,
                    );
                    println!("{}", report.dot);
                }
                Err(error) => eprintln!("  PS {index} CFG error: {error}"),
            }
        }
    }
}

fn main() {
    let cli = Cli::parse();
    if cli.files.is_empty() {
        eprintln!("No files specified. Usage: ufx <file.ufx> ...");
        std::process::exit(1);
    }

    for path in &cli.files {
        let data = match std::fs::read(path) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("{}: {e}", path.display());
                continue;
            }
        };
        let ufx = match ufx::parse(&data) {
            Ok(u) => u,
            Err(e) => {
                eprintln!("{}: {e}", path.display());
                continue;
            }
        };

        let name = path.file_name().unwrap_or_default().to_string_lossy();

        if cli.analysis.batch_infer {
            print_batch_inference(&name, &ufx);
            continue;
        }

        println!("=== {name} ===");
        println!("  version: {}  hash: 0x{:08X}", ufx.version, ufx.hash);
        println!(
            "  PS slots: [{:#X}, {:#X}, {:#X}, {:#X}]",
            ufx.ps_offsets[0], ufx.ps_offsets[1], ufx.ps_offsets[2], ufx.ps_offsets[3]
        );

        if cli.display.flags {
            print_flags(&name);
        }

        if let Some(ref vs) = ufx.vertex_shader {
            println!(
                "\n-- Vertex Shader (offset {:#X}, {} bytes) --",
                vs.offset(),
                vs.size()
            );
            print_shader_info(vs, cli.display.cb_only, cli.display.disasm);
        }

        for (i, ps) in ufx.pixel_shaders.iter().enumerate() {
            println!(
                "\n-- Pixel Shader {i} (offset {:#X}, {} bytes) --",
                ps.offset(),
                ps.size()
            );
            print_shader_info(ps, cli.display.cb_only, cli.display.disasm);
        }

        if cli.analysis.infer_params {
            print_parameters(&ufx);
        }

        if cli.analysis.cfg {
            print_cfg(&ufx);
        }

        println!();
    }
}

fn print_shader_info(shader: &ufx::Shader<'_>, cb_only: bool, disasm: bool) {
    if let Some(prog) = shader.program() {
        println!("  SM {}.{}", prog.major_version, prog.minor_version);
    }

    if let Some(rd) = shader.resource_def() {
        if !rd.creator.is_empty() {
            println!("  compiler: {}", rd.creator);
        }

        // Constant buffers
        for cb in &rd.constant_buffers {
            println!("\n  cbuffer {} ({} bytes):", cb.name, cb.size);
            for v in &cb.variables {
                let ty = format_type(&v.var_type);
                println!(
                    "    +{:<4} {:<32} {} ({} bytes)",
                    v.offset, v.name, ty, v.size
                );
            }
        }

        if !cb_only {
            // Resource bindings
            if !rd.bindings.is_empty() {
                println!("\n  bindings:");
                for b in &rd.bindings {
                    println!("    {b}");
                }
            }
        }
    }

    if !cb_only {
        if let Some(sig) = shader.input_signature() {
            println!("\n  inputs:");
            for e in &sig.elements {
                println!("    {e}");
            }
        }
        if let Some(sig) = shader.output_signature() {
            println!("\n  outputs:");
            for e in &sig.elements {
                println!("    {e}");
            }
        }
    }

    if disasm {
        println!("\n  --- disassembly ---");
        print!("{shader}");
    }
}

fn format_type(t: &ufx::dxbc::chunks::rdef::TypeDesc<'_>) -> String {
    let base = match t.var_type {
        0 => "void",
        1 => "bool",
        2 => "int",
        3 => "float",
        4 => "string",
        5 => "texture",
        6 => "texture1d",
        7 => "texture2d",
        8 => "texture3d",
        9 => "texturecube",
        10 => "sampler",
        19 => "uint",
        _ => "?",
    };
    let class = match t.class {
        1 => "vec",     // vector
        2 | 3 => "mat", // row-major or column-major matrix
        _ => "",        // scalar or unknown class
    };
    if t.rows == 1 && t.columns == 1 {
        base.to_string()
    } else if t.rows == 1 {
        format!("{base}{}{columns}", class, columns = t.columns)
    } else {
        format!("{base}{class}{r}x{c}", r = t.rows, c = t.columns)
    }
}
