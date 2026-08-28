//! Exercises XTD vertex decoding with a real file.

use std::env;
use xtd::Reader;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    let path = args.get(1).map_or(
        "test_extract/scenario/skirmish/design/blood_gulch/blood_gulch.xtd",
        std::string::String::as_str,
    );

    println!("Loading XTD file: {path}");
    let data = std::fs::read(path)?;
    println!("File size: {} bytes", data.len());

    // Debug: print first 64 bytes of atlas data in hex
    let file = Reader::read(&data)?;
    println!("\n=== XTD Header ===");
    println!("  Version: 0x{:04X}", file.header.version);
    println!("  NumXVerts: {}", file.header.num_x_verts);
    println!("  NumXChunks: {}", file.header.num_x_chunks);
    println!("  TileScale: {}", file.header.tile_scale);
    println!("  WorldMin: {:?}", file.header.world_min);
    println!("  WorldMax: {:?}", file.header.world_max);
    println!("\n  Visual chunks: {}", file.visual_chunks.len());
    println!("  Atlas data: {} bytes", file.atlas_data.len());

    println!("\n=== Decoding Vertices ===");
    let vertices = file.decode_vertices()?;

    println!("  Atlas Header:");
    println!("    Mid: {:?}", vertices.header.mid);
    println!("    Range: {:?}", vertices.header.range);

    print_atlas_debug(&file);
    println!(
        "\n  Terrain grid: {}x{}",
        vertices.num_verts_per_axis, vertices.num_verts_per_axis
    );
    println!("  Total vertices: {}", vertices.positions.len());

    // Print first few vertices
    println!("\n  First 10 vertices:");
    for i in 0..10.min(vertices.positions.len()) {
        let pos = vertices.positions[i];
        let norm = vertices.normals[i];
        let norm_len = (norm[0] * norm[0] + norm[1] * norm[1] + norm[2] * norm[2]).sqrt();
        println!(
            "    [{:5}] pos=({:8.2}, {:8.2}, {:8.2}) norm=({:6.3}, {:6.3}, {:6.3}) |n|={:.3}",
            i, pos[0], pos[1], pos[2], norm[0], norm[1], norm[2], norm_len
        );
    }

    // Print vertices at specific grid positions to check layout
    let n = vertices.num_verts_per_axis;
    println!("\n  Corner vertices (expecting X~gridX, Z~gridZ):");
    let corners = [(0, 0), (0, n - 1), (n - 1, 0), (n - 1, n - 1), (512, 512)];
    for (x, z) in corners {
        let i = z * n + x;
        let pos = vertices.positions[i];
        let norm = vertices.normals[i];
        let norm_len = (norm[0] * norm[0] + norm[1] * norm[1] + norm[2] * norm[2]).sqrt();
        println!(
            "    Grid ({:4},{:4}) -> pos=({:8.2}, {:8.2}, {:8.2}) expected~({:4},{:4}) |n|={:.3}",
            x, z, pos[0], pos[1], pos[2], x, z, norm_len
        );
    }

    // Position bounds
    let mut min_pos = [f32::MAX; 3];
    let mut max_pos = [f32::MIN; 3];
    for pos in &vertices.positions {
        for j in 0..3 {
            min_pos[j] = min_pos[j].min(pos[j]);
            max_pos[j] = max_pos[j].max(pos[j]);
        }
    }
    println!("\n  Position bounds:");
    println!(
        "    Min: ({:.2}, {:.2}, {:.2})",
        min_pos[0], min_pos[1], min_pos[2]
    );
    println!(
        "    Max: ({:.2}, {:.2}, {:.2})",
        max_pos[0], max_pos[1], max_pos[2]
    );

    // Generate indices
    let indices = vertices.generate_indices()?;
    println!(
        "\n  Generated {} indices ({} triangles)",
        indices.len(),
        indices.len() / 3
    );

    print_summary(&vertices, &indices);

    Ok(())
}

fn print_summary(vertices: &xtd::TerrainVertices, indices: &[u32]) {
    let mut bad_normals = 0;
    for (index, normal) in vertices.normals.iter().enumerate() {
        let length = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
        if (length - 1.0).abs() > 0.15 {
            bad_normals += 1;
            if bad_normals <= 5 {
                println!("  WARNING: Normal {index} not normalized: {normal:?} (len={length})");
            }
        }
    }
    if bad_normals > 5 {
        println!("  ... and {} more bad normals", bad_normals - 5);
    }

    println!("\n=== Summary ===");
    println!("  ✓ XTD file parsed successfully");
    println!("  ✓ {} vertices decoded", vertices.positions.len());
    println!("  ✓ {} indices generated", indices.len());
    if bad_normals == 0 {
        println!("  ✓ All normals are normalized");
    } else {
        println!(
            "  ⚠ {}/{} normals have unusual length",
            bad_normals,
            vertices.normals.len()
        );
    }
}

fn print_atlas_debug(file: &xtd::XtdFile) {
    println!("\n  Raw atlas data (first 64 bytes after header):");
    let atlas_data = &file.atlas_data;
    print!("    Header bytes: ");
    for byte in atlas_data.iter().take(32) {
        print!("{byte:02x} ");
    }
    println!();

    println!("\n  First 8 packed positions (at offset 32):");
    for index in 0..8 {
        let offset = 32 + index * 4;
        if let Some(bytes) = atlas_data.get(offset..offset + 4) {
            let packed = u32::from_be_bytes(bytes.try_into().expect("four-byte packed position"));
            let x = (packed >> 22) & 0x3FF;
            let y = (packed >> 11) & 0x3FF;
            let z = packed & 0x3FF;
            println!("    [{index}] 0x{packed:08x} -> X={x:4} Y={y:4} Z={z:4}");
        }
    }

    println!("\n  Checking if data is row-major (first 8 vs positions 1024-1031):");
    for index in 0..4 {
        let first_offset = 32 + index * 4;
        let second_offset = 32 + (index + 1024) * 4;
        let first = atlas_data.get(first_offset..first_offset + 4);
        let second = atlas_data.get(second_offset..second_offset + 4);
        if let (Some(first), Some(second)) = (first, second) {
            let first = u32::from_be_bytes(first.try_into().expect("four-byte packed position"));
            let second = u32::from_be_bytes(second.try_into().expect("four-byte packed position"));
            println!("    Row 0 col {index}: 0x{first:08x}   Row 1 col {index}: 0x{second:08x}");
        }
    }
}
