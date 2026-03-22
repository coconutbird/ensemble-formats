use image::{ImageBuffer, Rgba};
use std::fs;
use xtt::Reader;

fn main() {
    let data =
        fs::read("test_extract/scenario/skirmish/design/blood_gulch/blood_gulch.xtt").unwrap();
    let xtt = Reader::read(&data).unwrap();

    println!("XTT Header:");
    println!("  version: 0x{:04X}", xtt.header.version);
    println!("  num_active_textures: {}", xtt.header.num_active_textures);
    println!("  num_active_decals: {}", xtt.header.num_active_decals);
    println!(
        "  num_active_decal_instances: {}",
        xtt.header.num_active_decal_instances
    );

    println!("\nLinkers: {} chunks", xtt.linkers.len());
    if !xtt.linkers.is_empty() {
        let l = &xtt.linkers[0];
        println!(
            "  First linker: grid({}, {}), splat_layers={}, decal_layers={}",
            l.grid_x, l.grid_z, l.num_splat_layers, l.num_decal_layers
        );
    }

    println!("\nAlbedo data size: {} bytes", xtt.albedo_data.len());

    // Decode the albedo atlas
    println!("\nDecoding albedo atlas...");
    match xtt.decode_albedo() {
        Ok(atlas) => {
            println!(
                "  Decoded: {}x{} pixels, {} mips",
                atlas.width, atlas.height, atlas.num_mips
            );
            println!("  Pixel data size: {} bytes", atlas.pixels.len());

            // Save as PNG
            let img: ImageBuffer<Rgba<u8>, _> =
                ImageBuffer::from_raw(atlas.width, atlas.height, atlas.pixels.clone())
                    .expect("Failed to create image buffer");
            let output_path = "test_extract/blood_gulch_albedo.png";
            img.save(output_path).expect("Failed to save PNG");
            println!("  Saved PNG to: {}", output_path);

            // Print first few pixels to verify decoding
            println!("\n  First 4 pixels (RGBA):");
            for i in 0..4 {
                let offset = i * 4;
                if offset + 3 < atlas.pixels.len() {
                    println!(
                        "    Pixel {}: R={} G={} B={} A={}",
                        i,
                        atlas.pixels[offset],
                        atlas.pixels[offset + 1],
                        atlas.pixels[offset + 2],
                        atlas.pixels[offset + 3]
                    );
                }
            }
        }
        Err(e) => {
            println!("  ERROR: {}", e);
        }
    }

    println!("\nRoad data size: {} bytes", xtt.road_data.len());
    println!("Foliage sets: {}", xtt.foliage.sets.len());
    for (i, set) in xtt.foliage.sets.iter().enumerate() {
        println!("  [{}] {}", i, set.filename);
    }
    println!("Foliage QN chunks: {}", xtt.foliage.qn_chunks.len());
}
