//! Example: Parse a DDX file and print its info.

use ddx::DdxTexture;
use std::env;
use std::fs;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <ddx_file>", args[0]);
        std::process::exit(1);
    }

    let path = &args[1];
    let data = match fs::read(path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Error reading file: {}", e);
            std::process::exit(1);
        }
    };

    match DdxTexture::from_bytes(&data) {
        Ok(texture) => {
            println!("DDX Texture Info:");
            println!("  Width: {}", texture.info.width);
            println!("  Height: {}", texture.info.height);
            println!("  Format: {:?}", texture.info.data_format);
            println!("  Resource Type: {:?}", texture.info.resource_type);
            println!("  Mip Levels: {}", texture.info.num_mip_levels);
            println!("  Has Alpha: {}", texture.info.has_alpha);
            println!("  Platform: {:?}", texture.info.platform);
            println!("  HDR Scale: {}", texture.info.hdr_scale);
            println!("  Data Size: {} bytes", texture.data.len());
        }
        Err(e) => {
            eprintln!("Error parsing DDX: {}", e);
            std::process::exit(1);
        }
    }
}
