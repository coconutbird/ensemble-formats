//! HW1 game database CLI — validate database files and resolve the full asset
//! pipeline from ERA archives.
//!
//! # Subcommands
//!
//! - `validate` — parse all database XMBs and report success/failure
//! - `resolve`  — walk the full asset resolution pipeline:
//!   objects → visuals → model/anim refs, objects → tactics, objects → physics chain
//!
//! # ERA loading
//!
//! Uses [`AssetSource`] to load multiple ERA archives in priority order,
//! matching the game engine's `BArchiveManager` behaviour (confirmed via IDA).

pub mod assets;
mod resolve;
mod validate;

use clap::{Parser, Subcommand};

use assets::AssetSource;

#[derive(Parser)]
#[command(name = "database")]
#[command(about = "HW1 game database tool — validate and resolve game assets")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Validate all database XMB files parse correctly
    Validate {
        /// Path to root.era
        era_path: String,
    },
    /// Resolve the full asset pipeline: objects → visuals → models/anims
    Resolve {
        /// Path to the game directory containing ERA files
        #[arg(long)]
        game_dir: Option<String>,
        /// Explicit ERA paths to load (in priority order, last = highest)
        #[arg(long = "era")]
        era_paths: Vec<String>,
        /// Print every resolved asset path (verbose)
        #[arg(short, long)]
        verbose: bool,
    },
}

/// Build an [`AssetSource`] from a game directory, loading ERAs in the
/// engine's confirmed load order.
fn load_game_dir(dir: &str) -> AssetSource {
    let mut src = AssetSource::new();
    let era_order = [
        "root.era",
        "root_update.era",
        "locale.era",
        "locale_update.era",
        "scenarioshared.era",
    ];
    for name in &era_order {
        let path = format!("{dir}/{name}");
        if std::path::Path::new(&path).exists() {
            match src.add_era(&path) {
                Ok(n) => println!("  Loaded {name:<24} ({n} entries)"),
                Err(e) => eprintln!("  WARN  {name}: {e}"),
            }
        }
    }
    // Auto-discover DLC ERAs
    for i in 1..=10 {
        let name = format!("dlc{i:02}.era");
        let path = format!("{dir}/{name}");
        if std::path::Path::new(&path).exists() {
            match src.add_era(&path) {
                Ok(n) => println!("  Loaded {name:<24} ({n} entries)"),
                Err(e) => eprintln!("  WARN  {name}: {e}"),
            }
        }
    }
    src
}

/// Build an [`AssetSource`] from explicit ERA paths.
fn load_era_list(paths: &[String]) -> AssetSource {
    let mut src = AssetSource::new();
    for path in paths {
        match src.add_era(path) {
            Ok(n) => println!("  Loaded {path} ({n} entries)"),
            Err(e) => {
                eprintln!("Failed to load {path}: {e}");
                std::process::exit(1);
            }
        }
    }
    src
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Validate { era_path } => validate::run(&era_path),
        Commands::Resolve {
            game_dir,
            era_paths,
            verbose,
        } => {
            let mut src = if let Some(dir) = &game_dir {
                println!("Loading ERAs from {dir}:");
                load_game_dir(dir)
            } else if !era_paths.is_empty() {
                println!("Loading ERAs:");
                load_era_list(&era_paths)
            } else {
                eprintln!("Error: provide --game-dir or --era paths");
                std::process::exit(1);
            };
            println!();
            for (label, count) in src.summary() {
                println!("  {label:<24} {count} files");
            }
            println!();
            resolve::run(&mut src, verbose);
        }
    }
}
