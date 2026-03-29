use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "ufx", about = "Inspect UFX compiled shader files")]
struct Cli {
    /// UFX file(s) to inspect.
    files: Vec<PathBuf>,

    /// Show full disassembly for each shader stage.
    #[arg(short, long)]
    disasm: bool,

    /// Only show constant buffer layouts (skip bindings/signatures).
    #[arg(short = 'c', long)]
    cb_only: bool,

    /// Infer semantic names for material CB parameters (cb8 PS, cb7 VS).
    #[arg(short = 'i', long)]
    infer_params: bool,

    /// Build a control-flow graph and print it as Graphviz DOT.
    #[arg(long)]
    cfg: bool,

    /// Batch inference: output one TSV line per file with hex flags and params.
    /// Format: hex_flags<TAB>slot.reg.comp=semantic,...
    #[arg(long)]
    batch_infer: bool,

    /// Decode bitflags from the filename and show enabled features.
    #[arg(short = 'f', long)]
    flags: bool,
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

        // Batch inference mode: one compact TSV line per file, no other output.
        if cli.batch_infer {
            use ufx::cb_infer::{HOGAN_PS_SLOT, HOGAN_VS_SLOT};

            // Extract hex flags from filename: hogan_<type>_<hex>.ufx
            let hex_part = name
                .strip_suffix(".ufx")
                .and_then(|n| n.rsplit('_').next())
                .unwrap_or("?");

            let mut all_params = Vec::new();

            // PS inference
            if let Some(ps) = ufx.pixel_shaders.first()
                && let Some(prog) = ps.program()
            {
                all_params.extend(ufx::cb_infer::infer_cb_params(
                    prog,
                    HOGAN_PS_SLOT,
                    HOGAN_VS_SLOT,
                ));
            }

            // VS inference
            if let Some(vs) = &ufx.vertex_shader
                && let Some(prog) = vs.program()
            {
                let vs_params = ufx::cb_infer::infer_cb_params(prog, HOGAN_PS_SLOT, HOGAN_VS_SLOT);
                for p in vs_params {
                    if p.cb_slot == HOGAN_VS_SLOT {
                        all_params.push(p);
                    }
                }
            }

            // Output: hex<TAB>cb8[0].xy=uv_scale[high],cb8[1].x=normal_intensity[high],...
            let param_strs: Vec<String> = all_params
                .iter()
                .map(|p| {
                    format!(
                        "cb{}[{}].{}={}[{}]",
                        p.cb_slot, p.reg_index, p.components, p.semantic, p.confidence
                    )
                })
                .collect();
            println!("{}\t{}", hex_part, param_strs.join(","));
            continue;
        }

        println!("=== {name} ===");
        println!("  version: {}  hash: 0x{:08X}", ufx.version, ufx.hash);
        println!(
            "  PS slots: [{:#X}, {:#X}, {:#X}, {:#X}]",
            ufx.ps_offsets[0], ufx.ps_offsets[1], ufx.ps_offsets[2], ufx.ps_offsets[3]
        );

        if cli.flags {
            use ufx::cb_infer::bitflags::HoganFlags;
            if let Some(flags) = HoganFlags::from_filename(&name) {
                println!(
                    "\n  -- Bitflags: {} --",
                    ufx::cb_infer::bitflags::flags_summary(flags)
                );
                let features = flags.features();
                if features.is_empty() {
                    println!("    (no known features decoded)");
                } else {
                    for f in &features {
                        let req = if f.requires_bits.is_empty() {
                            String::new()
                        } else {
                            format!(" (requires bits {:?})", f.requires_bits)
                        };
                        println!(
                            "    bit {:2} | cb{} | {:<22} — {}{}",
                            f.bit, f.cb_slot, f.name, f.description, req
                        );
                    }
                }
            } else {
                println!("  (could not parse flags from filename)");
            }
        }

        if let Some(ref vs) = ufx.vertex_shader {
            println!(
                "\n-- Vertex Shader (offset {:#X}, {} bytes) --",
                vs.offset(),
                vs.size()
            );
            print_shader_info(vs, cli.cb_only, cli.disasm);
        }

        for (i, ps) in ufx.pixel_shaders.iter().enumerate() {
            println!(
                "\n-- Pixel Shader {i} (offset {:#X}, {} bytes) --",
                ps.offset(),
                ps.size()
            );
            print_shader_info(ps, cli.cb_only, cli.disasm);
        }

        if cli.infer_params {
            // Infer PS params from first pixel shader
            use ufx::cb_infer::{HOGAN_PS_SLOT, HOGAN_VS_SLOT};

            if let Some(ps) = ufx.pixel_shaders.first()
                && let Some(prog) = ps.program()
            {
                let params = ufx::cb_infer::infer_cb_params(prog, HOGAN_PS_SLOT, HOGAN_VS_SLOT);
                if params.is_empty() {
                    println!("  (no cb8/cb7 params detected)");
                } else {
                    println!("\n  -- Inferred CB Parameters --");
                    for p in &params {
                        println!(
                            "    cb{}[{}].{:<6} => {:<24} [{}] (insn #{})",
                            p.cb_slot,
                            p.reg_index,
                            p.components,
                            p.semantic,
                            p.confidence,
                            p.insn_index,
                        );
                    }
                }
            }

            // Infer VS params
            if let Some(vs) = &ufx.vertex_shader
                && let Some(prog) = vs.program()
            {
                let params = ufx::cb_infer::infer_cb_params(prog, HOGAN_PS_SLOT, HOGAN_VS_SLOT);
                let vs_params: Vec<_> = params
                    .iter()
                    .filter(|p| p.cb_slot == HOGAN_VS_SLOT)
                    .collect();
                if !vs_params.is_empty() {
                    println!("\n  -- Inferred VS CB Parameters --");
                    for p in &vs_params {
                        println!(
                            "    cb{}[{}].{:<6} => {:<24} [{}] (insn #{})",
                            p.cb_slot,
                            p.reg_index,
                            p.components,
                            p.semantic,
                            p.confidence,
                            p.insn_index,
                        );
                    }
                }
            }
        }

        if cli.cfg {
            if let Some(vs) = &ufx.vertex_shader
                && let Some(prog) = vs.program()
            {
                match cfglib_dxbc::build_cfg(prog) {
                    Ok(cfg) => {
                        println!(
                            "\n  -- Vertex Shader CFG ({} blocks, {} edges) --",
                            cfg.blocks().len(),
                            cfg.edges().count(),
                        );
                        println!("{}", cfg.to_dot());
                    }
                    Err(e) => eprintln!("  VS CFG error: {e}"),
                }
            }

            for (i, ps) in ufx.pixel_shaders.iter().enumerate() {
                if let Some(prog) = ps.program() {
                    match cfglib_dxbc::build_cfg(prog) {
                        Ok(cfg) => {
                            println!(
                                "\n  -- Pixel Shader {i} CFG ({} blocks, {} edges) --",
                                cfg.blocks().len(),
                                cfg.edges().count(),
                            );
                            println!("{}", cfg.to_dot());
                        }
                        Err(e) => eprintln!("  PS {i} CFG error: {e}"),
                    }
                }
            }
        }

        println!();
    }
}

fn print_shader_info(shader: &d3dasm::Shader, cb_only: bool, disasm: bool) {
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

fn format_type(t: &d3dasm::dxbc::chunks::rdef::TypeDesc) -> String {
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
        0 => "",    // scalar
        1 => "vec", // vector
        2 => "mat", // matrix row-major
        3 => "mat", // matrix col-major
        _ => "",
    };
    if t.rows == 1 && t.columns == 1 {
        base.to_string()
    } else if t.rows == 1 {
        format!("{base}{}{columns}", class, columns = t.columns)
    } else {
        format!("{base}{class}{r}x{c}", r = t.rows, c = t.columns)
    }
}
