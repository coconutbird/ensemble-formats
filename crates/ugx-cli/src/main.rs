//! UGX CLI - Command-line tool for UGX model files.

use clap::{Parser, Subcommand};
use std::fs;
use std::path::PathBuf;
use ugx::{export_to_gltf, GltfExportOptions, UgxGeom};

#[derive(Parser)]
#[command(name = "ugx")]
#[command(about = "UGX (Unit Graphics) model file tool for Halo Wars")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Show information about a UGX file
    Info {
        /// Input UGX file
        #[arg(short, long)]
        input: PathBuf,
    },
    /// Convert UGX to glTF format
    ToGltf {
        /// Input UGX file
        #[arg(short, long)]
        input: PathBuf,
        /// Output glTF file
        #[arg(short, long)]
        output: PathBuf,
        /// Create separate .bin file instead of embedding data
        #[arg(long)]
        external_buffer: bool,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Info { input } => cmd_info(&input)?,
        Commands::ToGltf {
            input,
            output,
            external_buffer,
        } => cmd_to_gltf(&input, &output, external_buffer)?,
    }

    Ok(())
}

fn cmd_info(input: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let data = fs::read(input)?;
    let geom = UgxGeom::read(&data)?;

    println!("UGX File: {}", input.display());
    println!();

    println!("Bounding Sphere:");
    println!(
        "  Center: [{:.3}, {:.3}, {:.3}]",
        geom.bounding_sphere.center[0],
        geom.bounding_sphere.center[1],
        geom.bounding_sphere.center[2]
    );
    println!("  Radius: {:.3}", geom.bounding_sphere.radius);
    println!();

    println!("Bounding Box:");
    println!(
        "  Min: [{:.3}, {:.3}, {:.3}]",
        geom.bounds.min[0], geom.bounds.min[1], geom.bounds.min[2]
    );
    println!(
        "  Max: [{:.3}, {:.3}, {:.3}]",
        geom.bounds.max[0], geom.bounds.max[1], geom.bounds.max[2]
    );
    println!();

    println!("Materials: {}", geom.materials.len());
    for (i, mat) in geom.materials.iter().enumerate() {
        println!("  [{}] {}", i, mat.name);
        // Show diffuse texture if present
        if !mat.maps[0].maps.is_empty() {
            println!("      Diffuse: {}", mat.maps[0].maps[0].name);
        }
    }
    println!();

    println!("Bones: {}", geom.bones.len());
    for (i, bone) in geom.bones.iter().enumerate() {
        let parent = if bone.parent_index >= 0 {
            format!("parent={}", bone.parent_index)
        } else {
            "root".to_string()
        };
        println!("  [{}] {} ({})", i, bone.name, parent);
    }
    println!();

    println!("Sections: {}", geom.sections.len());
    for (i, section) in geom.sections.iter().enumerate() {
        println!(
            "  [{}] Material={}, Verts={}, Tris={}, VB={}bytes",
            i, section.material_index, section.num_verts, section.num_tris, section.vb_bytes
        );
        println!("       PackOrder: {}", section.base_vert_packer.pack_order);
    }
    println!();

    println!("Summary:");
    println!("  Total Vertices: {}", geom.total_vertices());
    println!("  Total Triangles: {}", geom.total_triangles());
    println!("  Vertex Buffer: {} bytes", geom.vertex_buffer.len());
    println!("  Index Buffer: {} indices", geom.index_buffer.len());
    println!("  Shadow Geometry: {}", geom.shadow_geom);
    println!("  Rigid Only: {}", geom.rigid_only);

    Ok(())
}

fn cmd_to_gltf(
    input: &PathBuf,
    output: &PathBuf,
    external_buffer: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let data = fs::read(input)?;
    let geom = UgxGeom::read(&data)?;

    let options = GltfExportOptions {
        embed_buffers: !external_buffer,
        include_materials: true,
        include_skeleton: true,
    };

    let export = export_to_gltf(&geom, &options)?;

    // Write the glTF JSON
    fs::write(output, &export.json)?;
    println!("Wrote {}", output.display());

    // Write external buffer if present
    if let Some(buffer_data) = export.buffer {
        let bin_path = output.with_extension("bin");
        fs::write(&bin_path, &buffer_data)?;
        println!("Wrote {}", bin_path.display());
    }

    println!();
    println!("Exported {} sections, {} materials, {} bones",
        geom.sections.len(),
        geom.materials.len(),
        geom.bones.len()
    );
    println!("Total: {} vertices, {} triangles",
        geom.total_vertices(),
        geom.total_triangles()
    );

    Ok(())
}
