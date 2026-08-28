//! UGX command-line entry point.

mod cli;
mod info;
mod scan;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    cli::run()
}
