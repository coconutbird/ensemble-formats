//! UGX CLI - Command-line tool for UGX model files.

use clap::{Parser, Subcommand};
use ecf::EcfReader;
use std::fs;
use std::fs::File;
use std::path::PathBuf;
use ugx::{
    export_to_gltf_with_buffer_name, import_from_gltf, write_ugx, GltfExportOptions,
    GltfImportOptions, UgxGeom,
};

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
        /// Exclude skeleton/bones from the export
        #[arg(long)]
        no_skeleton: bool,
    },
    /// Convert glTF to UGX format
    FromGltf {
        /// Input glTF file
        #[arg(short, long)]
        input: PathBuf,
        /// Output UGX file
        #[arg(short, long)]
        output: PathBuf,
        /// Exclude skeleton/bones from the import
        #[arg(long)]
        no_skeleton: bool,
    },
    /// Dump ECF structure for debugging
    Dump {
        /// Input UGX file
        #[arg(short, long)]
        input: PathBuf,
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
            no_skeleton,
        } => cmd_to_gltf(&input, &output, external_buffer, no_skeleton)?,
        Commands::FromGltf {
            input,
            output,
            no_skeleton,
        } => cmd_from_gltf(&input, &output, no_skeleton)?,
        Commands::Dump { input } => cmd_dump(&input)?,
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
        if !mat.maps[0].is_empty() {
            println!("      Diffuse: {}", mat.maps[0][0].name);
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
        // Print matrix for first 3 bones
        if i < 3 {
            let m = &bone.model_to_bone.rows;
            println!("      Matrix (model_to_bone):");
            println!(
                "        [{:8.4}, {:8.4}, {:8.4}, {:8.4}]",
                m[0][0], m[0][1], m[0][2], m[0][3]
            );
            println!(
                "        [{:8.4}, {:8.4}, {:8.4}, {:8.4}]",
                m[1][0], m[1][1], m[1][2], m[1][3]
            );
            println!(
                "        [{:8.4}, {:8.4}, {:8.4}, {:8.4}]",
                m[2][0], m[2][1], m[2][2], m[2][3]
            );
            println!(
                "        [{:8.4}, {:8.4}, {:8.4}, {:8.4}]",
                m[3][0], m[3][1], m[3][2], m[3][3]
            );
        }
    }
    println!();

    println!("Sections: {}", geom.sections.len());
    for (i, section) in geom.sections.iter().enumerate() {
        println!(
            "  [{}] Material={}, Verts={}, Tris={}, VB={}bytes, VertSize={}",
            i,
            section.material_index,
            section.num_verts,
            section.num_tris,
            section.vb_bytes,
            section.vert_size
        );
        println!(
            "       MaxBones={}, RigidBoneIdx={}, RigidOnly={}, GlobalBones={}",
            section.max_bones, section.rigid_bone_index, section.rigid_only, section.global_bones
        );
        println!("       PackOrder: {}", section.base_vert_packer.pack_order);
        println!("       PosType: {:?}", section.base_vert_packer.pos_type);
        println!(
            "       NormType: {:?}",
            section.base_vert_packer.normal_type
        );
        println!(
            "       TangentType: {:?}",
            section.base_vert_packer.tangent_type
        );
        println!(
            "       UV[0]Type: {:?}",
            section.base_vert_packer.uv_types[0]
        );
        println!(
            "       IndicesType: {:?}",
            section.base_vert_packer.indices_type
        );
        println!(
            "       WeightsType: {:?}",
            section.base_vert_packer.weights_type
        );

        // Print first 3 unpacked vertices for debugging
        if section.num_verts > 0 && !section.base_vert_packer.pack_order.is_empty() {
            if let Ok(verts) = geom.unpack_section_vertices(i) {
                println!("       First 3 vertices:");
                for (vi, v) in verts.iter().take(3).enumerate() {
                    println!("         [{}] pos=[{:.3}, {:.3}, {:.3}] bones=[{},{},{},{}] weights=[{:.3},{:.3},{:.3},{:.3}]",
                        vi, v.position[0], v.position[1], v.position[2],
                        v.bone_indices[0], v.bone_indices[1], v.bone_indices[2], v.bone_indices[3],
                        v.bone_weights[0], v.bone_weights[1], v.bone_weights[2], v.bone_weights[3]);
                }
            }
        }
    }
    println!();

    println!("Summary:");
    println!("  Total Vertices: {}", geom.total_vertices());
    println!("  Total Triangles: {}", geom.total_triangles());
    println!("  Vertex Buffer: {} bytes", geom.vertex_buffer.len());
    println!("  Index Buffer: {} indices", geom.index_buffer.len());
    println!("  Rigid Only: {}", geom.rigid_only);
    println!("  All Sections Rigid: {}", geom.all_sections_rigid);
    println!("  All Sections Skinned: {}", geom.all_sections_skinned);

    Ok(())
}

fn cmd_to_gltf(
    input: &PathBuf,
    output: &PathBuf,
    external_buffer: bool,
    no_skeleton: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let data = fs::read(input)?;
    let geom = UgxGeom::read(&data)?;

    let options = GltfExportOptions {
        embed_buffers: !external_buffer,
        include_materials: true,
        include_skeleton: !no_skeleton,
    };

    // Derive buffer filename from output path for external buffer mode
    let bin_name = output
        .with_extension("bin")
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let export = export_to_gltf_with_buffer_name(&geom, &options, &bin_name)?;

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
    println!(
        "Exported {} sections, {} materials, {} bones",
        geom.sections.len(),
        geom.materials.len(),
        geom.bones.len()
    );
    println!(
        "Total: {} vertices, {} triangles",
        geom.total_vertices(),
        geom.total_triangles()
    );

    Ok(())
}

fn cmd_from_gltf(
    input: &PathBuf,
    output: &PathBuf,
    no_skeleton: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    // Read glTF JSON
    let json_str = fs::read_to_string(input)?;

    // Check for external .bin file
    let bin_path = input.with_extension("bin");
    let buffer_data = if bin_path.exists() {
        Some(fs::read(&bin_path)?)
    } else {
        None
    };

    let options = GltfImportOptions {
        include_skeleton: !no_skeleton,
        include_materials: true,
    };

    let geom = import_from_gltf(&json_str, buffer_data.as_deref(), &options)?;

    // Write UGX
    let ugx_data = write_ugx(&geom)?;
    fs::write(output, &ugx_data)?;

    println!("Wrote {}", output.display());
    println!();
    println!(
        "Imported {} sections, {} materials, {} bones",
        geom.sections.len(),
        geom.materials.len(),
        geom.bones.len()
    );
    println!(
        "Total: {} vertices, {} triangles",
        geom.total_vertices(),
        geom.total_triangles()
    );

    Ok(())
}

fn cmd_dump(input: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let mut file = File::open(input)?;
    let mut ecf = EcfReader::new(&mut file)?;

    println!("=== ECF Header ===");
    let header = ecf.header();
    println!("  File ID: 0x{:08X}", header.id);
    println!("  Num chunks: {}", header.num_chunks);
    println!("  Flags: 0x{:04X}", header.flags);

    println!("\n=== Chunks ===");
    for (i, chunk) in ecf.chunks().iter().enumerate() {
        println!(
            "Chunk {}: ID=0x{:08X} offset=0x{:X} size=0x{:X} flags=0x{:02X}",
            i, chunk.id, chunk.offset, chunk.size, chunk.flags
        );
    }

    // Read and dump cached data chunk (0x700)
    if let Ok(cached_data) = ecf.read_chunk_data_by_id(0x700) {
        println!(
            "\n=== Cached Data (0x700) - {} bytes ===",
            cached_data.len()
        );
        hexdump(&cached_data, 768); // Dump more to see section data
    }

    // Read IB chunk (0x701)
    if let Ok(ib_data) = ecf.read_chunk_data_by_id(0x701) {
        println!("\n=== Index Buffer (0x701) - {} bytes ===", ib_data.len());
        println!("  {} indices", ib_data.len() / 2);
        hexdump(&ib_data, 64);
    }

    // Read VB chunk (0x702)
    if let Ok(vb_data) = ecf.read_chunk_data_by_id(0x702) {
        println!("\n=== Vertex Buffer (0x702) - {} bytes ===", vb_data.len());
        hexdump(&vb_data, 64);
    }

    // Read material chunk (0x704)
    if let Ok(mat_data) = ecf.read_chunk_data_by_id(0x704) {
        println!("\n=== Materials (0x704) - {} bytes ===", mat_data.len());
        hexdump(&mat_data, 512);
    }

    Ok(())
}

fn hexdump(data: &[u8], max: usize) {
    let show = data.len().min(max);
    for (i, chunk) in data[..show].chunks(16).enumerate() {
        print!("  {:04X}: ", i * 16);
        for byte in chunk {
            print!("{:02X} ", byte);
        }
        // Pad
        for _ in chunk.len()..16 {
            print!("   ");
        }
        print!(" |");
        for byte in chunk {
            let c = if *byte >= 0x20 && *byte < 0x7F {
                *byte as char
            } else {
                '.'
            };
            print!("{}", c);
        }
        println!("|");
    }
    if data.len() > max {
        println!("  ... ({} more bytes)", data.len() - max);
    }
}
