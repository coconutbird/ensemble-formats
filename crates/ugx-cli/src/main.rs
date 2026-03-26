//! UGX CLI - Command-line tool for UGX model files.

use clap::{Parser, Subcommand};
use ecf::Reader as EcfReader;
use std::fs;
use std::path::PathBuf;
use ugx::{Reader as UgxReader, UgxVersion, Writer as UgxWriter};
use ugx_gltf::{
    GltfExportOptions, GltfImportOptions, export_to_gltf_with_buffer_name, import_from_gltf,
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
    /// Convert glTF/GLB to UGX format
    FromGltf {
        /// Input glTF or GLB file
        #[arg(short, long)]
        input: PathBuf,
        /// Output UGX file
        #[arg(short, long)]
        output: PathBuf,
        /// Exclude skeleton/bones from the import
        #[arg(long)]
        no_skeleton: bool,
        /// Target game version: "hw1" for Halo Wars DE (v4) or "hw2" for Halo Wars 2 (v6)
        #[arg(long, default_value = "hw2")]
        version: String,
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
            version,
        } => {
            let ugx_version = match version.to_lowercase().as_str() {
                "hw1" | "de" | "v4" => UgxVersion::Hw1,
                "hw2" | "v6" => UgxVersion::Hw2,
                other => {
                    eprintln!("Unknown version '{}', expected 'hw1' or 'hw2'", other);
                    std::process::exit(1);
                }
            };
            cmd_from_gltf(&input, &output, no_skeleton, ugx_version)?
        }
        Commands::Dump { input } => cmd_dump(&input)?,
    }

    Ok(())
}

fn cmd_info(input: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let data = fs::read(input)?;
    let geom = UgxReader::read(&data)?;

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
        if let Some(ref packer) = section.base_vert_packer {
            println!("       PackOrder: {}", packer.pack_order);
            println!("       PosType: {:?}", packer.pos_type);
            println!("       NormType: {:?}", packer.normal_type);
            println!("       TangentType: {:?}", packer.tangent_type);
            println!("       UV[0]Type: {:?}", packer.uv_types[0]);
            println!("       IndicesType: {:?}", packer.indices_type);
            println!("       WeightsType: {:?}", packer.weights_type);
        } else {
            println!("       (HW2 format — no UnivertPacker)");
        }

        // Print first 3 unpacked vertices for debugging
        if section.num_verts > 0
            && let Ok(verts) = geom.unpack_section_vertices(i)
        {
            println!("       First 3 vertices:");
            for (vi, v) in verts.iter().take(3).enumerate() {
                println!(
                    "         [{}] pos=[{:.3}, {:.3}, {:.3}] bones=[{},{},{},{}] weights=[{:.3},{:.3},{:.3},{:.3}]",
                    vi,
                    v.position[0],
                    v.position[1],
                    v.position[2],
                    v.bone_indices[0],
                    v.bone_indices[1],
                    v.bone_indices[2],
                    v.bone_indices[3],
                    v.bone_weights[0],
                    v.bone_weights[1],
                    v.bone_weights[2],
                    v.bone_weights[3]
                );
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
    let geom = UgxReader::read(&data)?;

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

/// Parse a GLB file and return the JSON string and binary buffer.
#[allow(clippy::type_complexity)]
fn parse_glb(data: &[u8]) -> Result<(String, Option<Vec<u8>>), Box<dyn std::error::Error>> {
    // GLB Header: magic (4) + version (4) + length (4) = 12 bytes
    if data.len() < 12 {
        return Err("GLB file too small".into());
    }

    let magic = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    if magic != 0x46546C67 {
        // "glTF" in little-endian
        return Err(format!("Invalid GLB magic: 0x{:08X}", magic).into());
    }

    let version = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    if version != 2 {
        return Err(format!("Unsupported GLB version: {}", version).into());
    }

    let _total_length = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);

    // Parse chunks
    let mut offset = 12usize;
    let mut json_str = String::new();
    let mut bin_data: Option<Vec<u8>> = None;

    while offset + 8 <= data.len() {
        let chunk_length = u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]) as usize;
        let chunk_type = u32::from_le_bytes([
            data[offset + 4],
            data[offset + 5],
            data[offset + 6],
            data[offset + 7],
        ]);
        offset += 8;

        if offset + chunk_length > data.len() {
            return Err("GLB chunk extends beyond file".into());
        }

        match chunk_type {
            0x4E4F534A => {
                // "JSON" in little-endian
                json_str = String::from_utf8(data[offset..offset + chunk_length].to_vec())?;
            }
            0x004E4942 => {
                // "BIN\0" in little-endian
                bin_data = Some(data[offset..offset + chunk_length].to_vec());
            }
            _ => {
                // Unknown chunk type, skip
            }
        }

        offset += chunk_length;
    }

    if json_str.is_empty() {
        return Err("GLB file missing JSON chunk".into());
    }

    Ok((json_str, bin_data))
}

fn cmd_from_gltf(
    input: &PathBuf,
    output: &PathBuf,
    no_skeleton: bool,
    version: UgxVersion,
) -> Result<(), Box<dyn std::error::Error>> {
    // Check if input is GLB or glTF based on extension
    let is_glb = input
        .extension()
        .map(|ext| ext.eq_ignore_ascii_case("glb"))
        .unwrap_or(false);

    let (json_str, buffer_data) = if is_glb {
        // Parse GLB container
        let data = fs::read(input)?;
        parse_glb(&data)?
    } else {
        // Read glTF JSON
        let json_str = fs::read_to_string(input)?;

        // Parse JSON to find the buffer URI
        let root: serde_json::Value = serde_json::from_str(&json_str)?;
        let buffer_data = if let Some(buffers) = root.get("buffers").and_then(|b| b.as_array()) {
            if let Some(first_buffer) = buffers.first() {
                if let Some(uri) = first_buffer.get("uri").and_then(|u| u.as_str()) {
                    // Check if it's a file path (not base64 embedded)
                    if !uri.starts_with("data:") {
                        // Resolve relative to the input file's directory
                        let base_dir = input.parent().unwrap_or(std::path::Path::new("."));
                        let bin_path = base_dir.join(uri);
                        if bin_path.exists() {
                            Some(fs::read(&bin_path)?)
                        } else {
                            return Err(format!(
                                "External buffer file not found: {} (expected at {})",
                                uri,
                                bin_path.display()
                            )
                            .into());
                        }
                    } else {
                        None // Base64 embedded, will be handled by import_from_gltf
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        (json_str, buffer_data)
    };

    let options = GltfImportOptions {
        include_skeleton: !no_skeleton,
        include_materials: true,
        version,
    };

    let geom = import_from_gltf(&json_str, buffer_data.as_deref(), &options)?;

    // Write UGX
    let ugx_data = UgxWriter::write(&geom, version)?;
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
    let data = std::fs::read(input)?;
    let ecf = EcfReader::new(&data)?;

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
    if let Ok(cached_data) = ecf.chunk_data_by_id(0x700) {
        println!(
            "\n=== Cached Data (0x700) - {} bytes ===",
            cached_data.len()
        );
        hexdump(&cached_data, 768); // Dump more to see section data
    }

    // Read IB chunk (0x701)
    if let Ok(ib_data) = ecf.chunk_data_by_id(0x701) {
        println!("\n=== Index Buffer (0x701) - {} bytes ===", ib_data.len());
        println!("  {} indices", ib_data.len() / 2);
        hexdump(&ib_data, 64);
    }

    // Read VB chunk (0x702)
    if let Ok(vb_data) = ecf.chunk_data_by_id(0x702) {
        println!("\n=== Vertex Buffer (0x702) - {} bytes ===", vb_data.len());
        hexdump(&vb_data, 64);
    }

    // Read material chunk (0x704)
    if let Ok(mat_data) = ecf.chunk_data_by_id(0x704) {
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
