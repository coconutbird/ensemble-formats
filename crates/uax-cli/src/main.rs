//! UAX CLI — Inspect and dump Halo Wars animation files.

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use uax::types::{CurveData, CurvePayload};

#[derive(Parser)]
#[command(
    name = "uax",
    about = "Inspect UAX animation files (Halo Wars DE / HW2)"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Show high-level animation info (name, duration, track groups).
    Info {
        /// Input UAX file(s).
        #[arg(short, long, required = true, num_args = 1..)]
        input: Vec<PathBuf>,
    },

    /// Dump full animation hierarchy (track groups, bones, curves).
    Dump {
        /// Input UAX file.
        #[arg(short, long)]
        input: PathBuf,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Info { input } => {
            for path in &input {
                if let Err(e) = print_info(path) {
                    eprintln!("{}: {e}", path.display());
                }
            }
        }
        Commands::Dump { input } => {
            print_dump(&input)?;
        }
    }

    Ok(())
}

fn print_info(path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let data = std::fs::read(path)?;
    let anim = uax::Reader::read(&data)?;

    let name = path.file_name().unwrap_or_default().to_string_lossy();
    println!("=== {name} ===");
    println!(
        "  name:         {:?}",
        anim.name.as_deref().unwrap_or("(none)")
    );
    println!("  duration:     {:.4}s", anim.duration);
    println!("  time_step:    {:.6}", anim.time_step);
    println!("  oversampling: {:.1}", anim.oversampling);
    println!("  track_groups: {}", anim.track_groups.len());

    for (i, tg) in anim.track_groups.iter().enumerate() {
        let tg_name = tg.name.as_deref().unwrap_or("?");
        println!(
            "    [{}] '{}' — {} xform tracks, flags=0x{:X}",
            i,
            tg_name,
            tg.transform_tracks.len(),
            tg.flags,
        );
    }

    // ECF layer info
    let ecf = ecf::Reader::new(&data)?;
    let hdr = ecf.header();
    println!("  ecf_id:       0x{:08X}", hdr.id);
    println!("  file_size:    {} bytes", data.len());
    for (ci, ch) in ecf.chunks().iter().enumerate() {
        println!(
            "    chunk[{}] id=0x{:04X} size={} adler32=0x{:08X}",
            ci, ch.id, ch.size, ch.adler32,
        );
    }
    println!();

    Ok(())
}

fn print_dump(path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let data = std::fs::read(path)?;
    let anim = uax::Reader::read(&data)?;

    let name = path.file_name().unwrap_or_default().to_string_lossy();
    println!("=== {name} ===");
    println!(
        "Animation: {:?}  duration={:.4}s  time_step={:.6}  oversampling={:.1} loops={} flags=0x{:X}",
        anim.name.as_deref().unwrap_or("(none)"),
        anim.duration,
        anim.time_step,
        anim.oversampling,
        anim.default_loop_count,
        anim.flags,
    );
    println!();

    for (gi, tg) in anim.track_groups.iter().enumerate() {
        let tg_name = tg.name.as_deref().unwrap_or("?");
        println!("TrackGroup[{gi}] '{tg_name}'");
        println!("  flags: 0x{:X}", tg.flags);
        println!("  loop_translation: {:?}", tg.loop_translation);
        let p = &tg.initial_placement;
        println!(
            "  initial_placement: pos=[{:.3},{:.3},{:.3}] ori=[{:.3},{:.3},{:.3},{:.3}]",
            p.position[0],
            p.position[1],
            p.position[2],
            p.orientation[0],
            p.orientation[1],
            p.orientation[2],
            p.orientation[3],
        );
        if !tg.transform_lod_errors.is_empty() {
            println!("  lod_errors: {} entries", tg.transform_lod_errors.len());
        }
        println!("  vector_tracks: {}", tg.vector_tracks.len());
        for (track_index, track) in tg.vector_tracks.iter().enumerate() {
            println!(
                "    V[{track_index:3}] '{}' key={} dimension={}",
                track.name.as_deref().unwrap_or("?"),
                track.track_key,
                track.dimension
            );
            print_curve_detail("      V", &track.value);
        }
        println!("  transform_tracks: {}", tg.transform_tracks.len());

        for (ti, tt) in tg.transform_tracks.iter().enumerate() {
            let bone = tt.name.as_deref().unwrap_or("?");
            println!("    [{:3}] '{}' flags={}", ti, bone, tt.flags);
            print_curve_detail("      O", &tt.orientation);
            print_curve_detail("      P", &tt.position);
            print_curve_detail("      S", &tt.scale_shear);
        }
        println!("  text_tracks: {}", tg.text_tracks.len());
        println!();
    }

    Ok(())
}

fn print_curve_detail(prefix: &str, cd: &CurveData) {
    match cd.format {
        0..=9 => print_low_curve_detail(prefix, cd),
        10..=18 => print_high_curve_detail(prefix, cd),
        _ => print_unknown_curve(prefix, cd),
    }
}

fn print_low_curve_detail(prefix: &str, cd: &CurveData) {
    match &cd.payload {
        CurvePayload::DaKeyframes32f {
            dimension,
            controls,
        } => {
            println!(
                "{prefix}: DaKeyframes32f(dim={dimension}, controls={})",
                controls.len()
            );
        }
        CurvePayload::Identity { dimension } => {
            println!("{prefix}: Identity(dim={dimension})");
        }
        CurvePayload::DaConstant32f { controls, .. } => {
            println!(
                "{prefix}: DaConstant32f(deg={}, vals={:?})",
                cd.degree, controls
            );
        }
        CurvePayload::D3Constant32f { controls, .. } => {
            println!(
                "{prefix}: D3Constant32f(deg={}, [{:.4},{:.4},{:.4}])",
                cd.degree, controls[0], controls[1], controls[2]
            );
        }
        CurvePayload::D4Constant32f { controls, .. } => {
            println!(
                "{prefix}: D4Constant32f(deg={}, [{:.4},{:.4},{:.4},{:.4}])",
                cd.degree, controls[0], controls[1], controls[2], controls[3]
            );
        }
        CurvePayload::DaK32fC32f {
            knots, controls, ..
        } => {
            println!(
                "{prefix}: DaK32fC32f(deg={}, knots={}, ctrls={})",
                cd.degree,
                knots.len(),
                controls.len()
            );
        }
        CurvePayload::DaK16uC16u {
            control_scale_offsets,
            knots_controls,
            ..
        } => {
            println!(
                "{prefix}: DaK16uC16u(deg={}, scales={}, data={})",
                cd.degree,
                control_scale_offsets.len(),
                knots_controls.len()
            );
        }
        CurvePayload::DaK8uC8u {
            control_scale_offsets,
            knots_controls,
            ..
        } => {
            println!(
                "{prefix}: DaK8uC8u(deg={}, scales={}, data={})",
                cd.degree,
                control_scale_offsets.len(),
                knots_controls.len()
            );
        }
        CurvePayload::D4nK16uC15u {
            one_over_knot_scale,
            knots_controls,
            ..
        } => {
            println!(
                "{prefix}: D4nK16uC15u(deg={}, ooks={:.4}, data={})",
                cd.degree,
                one_over_knot_scale,
                knots_controls.len()
            );
        }
        CurvePayload::D4nK8uC7u {
            one_over_knot_scale,
            knots_controls,
            ..
        } => {
            println!(
                "{prefix}: D4nK8uC7u(deg={}, ooks={:.4}, data={})",
                cd.degree,
                one_over_knot_scale,
                knots_controls.len()
            );
        }
        _ => print_unknown_curve(prefix, cd),
    }
}

fn print_high_curve_detail(prefix: &str, cd: &CurveData) {
    match &cd.payload {
        CurvePayload::D3K16uC16u {
            control_scales,
            control_offsets,
            knots_controls,
            ..
        }
        | CurvePayload::D9I3K16uC16u {
            control_scales,
            control_offsets,
            knots_controls,
            ..
        }
        | CurvePayload::D3I1K16uC16u {
            control_scales,
            control_offsets,
            knots_controls,
            ..
        } => {
            println!(
                "{prefix}: {}(deg={}, data={}, scales={:?}, offsets={:?})",
                cd.payload.name(),
                cd.degree,
                knots_controls.len(),
                control_scales,
                control_offsets
            );
        }
        CurvePayload::D3K8uC8u {
            control_scales,
            control_offsets,
            knots_controls,
            ..
        }
        | CurvePayload::D9I3K8uC8u {
            control_scales,
            control_offsets,
            knots_controls,
            ..
        }
        | CurvePayload::D3I1K8uC8u {
            control_scales,
            control_offsets,
            knots_controls,
            ..
        } => {
            println!(
                "{prefix}: {}(deg={}, data={}, scales={:?}, offsets={:?})",
                cd.payload.name(),
                cd.degree,
                knots_controls.len(),
                control_scales,
                control_offsets
            );
        }
        CurvePayload::D9I1K16uC16u {
            control_scale,
            control_offset,
            knots_controls,
            ..
        } => {
            println!(
                "{prefix}: D9I1K16uC16u(deg={}, data={}, scale={control_scale}, offset={control_offset})",
                cd.degree,
                knots_controls.len()
            );
        }
        CurvePayload::D9I1K8uC8u {
            control_scale,
            control_offset,
            knots_controls,
            ..
        } => {
            println!(
                "{prefix}: D9I1K8uC8u(deg={}, data={}, scale={control_scale}, offset={control_offset})",
                cd.degree,
                knots_controls.len()
            );
        }
        CurvePayload::D3I1K32fC32f {
            control_scales,
            control_offsets,
            knots_controls,
            ..
        } => {
            println!(
                "{prefix}: D3I1K32fC32f(deg={}, data={}, scales={:?}, offsets={:?})",
                cd.degree,
                knots_controls.len(),
                control_scales,
                control_offsets
            );
        }
        _ => print_unknown_curve(prefix, cd),
    }
}

fn print_unknown_curve(prefix: &str, cd: &CurveData) {
    let raw_size = match &cd.payload {
        CurvePayload::Unknown { raw } => raw.len(),
        _ => 0,
    };
    println!(
        "{prefix}: Unknown(fmt={}, deg={}, {raw_size}B)",
        cd.format, cd.degree
    );
}
