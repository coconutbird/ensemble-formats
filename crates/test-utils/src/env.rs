//! Dotenv loading and environment variable helpers.

use std::path::{Path, PathBuf};
use std::sync::Once;

/// Load `.env` file from the workspace root (walks up from cwd).
/// Called automatically by [`env_var`]; safe to call multiple times.
pub fn load_dotenv() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let _ = dotenvy::dotenv();
    });
}

/// Read an environment variable, loading `.env` first if needed.
/// Returns `None` if the variable is unset or empty.
#[must_use]
pub fn env_var(name: &str) -> Option<String> {
    load_dotenv();
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// Load a game directory from an environment variable.
/// Returns `None` (and prints a skip message) if unset or not a directory.
#[must_use]
pub fn load_game_dir(env_name: &str) -> Option<PathBuf> {
    let Some(val) = env_var(env_name) else {
        eprintln!("{env_name} not set — skipping");
        return None;
    };
    let path = Path::new(&val).to_path_buf();
    if !path.is_dir() {
        eprintln!("{env_name}={val} is not a directory — skipping");
        return None;
    }
    Some(path)
}
