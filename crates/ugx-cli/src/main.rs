//! UGX command-line entry point.

mod cli;
mod gltf_io;
mod info;
mod scan;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    cli::run()
}
