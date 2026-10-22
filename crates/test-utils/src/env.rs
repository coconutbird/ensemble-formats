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

/// Read a game directory from an environment variable.
///
/// Use this function only in tests that have `#[ignore = "requires ..."]`. These tests run only
/// when you request them, so a missing game directory is a configuration error.
///
/// # Panics
///
/// Panics if the variable is unset, empty or does not name a directory.
#[must_use]
pub fn require_game_dir(env_name: &str) -> PathBuf {
    let Some(val) = env_var(env_name) else {
        panic!(
            "{env_name} is not set: set it in the environment or in the workspace `.env` file \
             to the game directory"
        );
    };
    let path = Path::new(&val).to_path_buf();
    assert!(path.is_dir(), "{env_name}={val} is not a directory");
    path
}
