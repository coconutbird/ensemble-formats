//! HW1 game database library — shared logic for validation and asset loading.

pub mod assets;
pub mod validate;

use assets::AssetSource;

/// Build an [`AssetSource`] from a game directory, loading ERAs in the
/// engine's confirmed load order.
pub fn load_game_dir(dir: &str) -> AssetSource {
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
