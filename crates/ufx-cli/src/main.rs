//! Command-line inspection and constant-buffer inference for UFX shader files.

use clap::{Args, Parser};
use cli_support::shader::{ShaderInfoOptions, write_shader_info};
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
    if let Some(program) = file.pixel_shader().and_then(ufx::Shader::program) {
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

    if let Some(program) = file.pixel_shader().and_then(ufx::Shader::program) {
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
    for stage in ufx::ShaderStage::ALL {
        let Some(program) = file.shader(stage).and_then(ufx::Shader::program) else {
            continue;
        };
        match ufx::cb_infer::cfg_report(program) {
            Ok(report) => {
                println!(
                    "\n  -- {stage} CFG ({} blocks, {} edges) --",
                    report.block_count, report.edge_count,
                );
                println!("{}", report.dot);
            }
            Err(error) => eprintln!("  {stage} CFG error: {error}"),
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
        println!("  stages:");
        for stage in ufx::ShaderStage::ALL {
            let range = ufx.stage_ranges.get(stage);
            if range.is_present() {
                println!(
                    "    {stage}: offset {:#X}, {} bytes",
                    range.offset, range.size
                );
            }
        }
        if !ufx.vertex_inputs.is_empty() {
            println!("  vertex inputs:");
            for input in &ufx.vertex_inputs {
                let rate = if input.per_instance {
                    "per-instance"
                } else {
                    "per-vertex"
                };
                println!(
                    "    {}{}: {} at byte {} (slot {}, {}, {} components)",
                    input.semantic,
                    input.semantic_index,
                    input.format,
                    input.byte_offset,
                    input.input_slot,
                    rate,
                    input.component_count(),
                );
            }
            if let Some(stride) = ufx.vertex_stride(0) {
                println!("    slot 0 consumed stride: {stride} bytes");
            }
        }

        if cli.display.flags {
            print_flags(&name);
        }

        for stage in ufx::ShaderStage::ALL {
            let Some(shader) = ufx.shader(stage) else {
                continue;
            };
            println!(
                "\n-- {stage} (offset {:#X}, {} bytes) --",
                shader.offset(),
                shader.size()
            );
            write_shader_info(
                &mut std::io::stdout().lock(),
                shader,
                ShaderInfoOptions {
                    variable_types: true,
                    constant_buffers_only: cli.display.cb_only,
                    disassembly: cli.display.disasm,
                },
            )
            .expect("failed printing to stdout");
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
