//! End-to-end coverage for the direct cross-version CLI command.

use std::path::{Path, PathBuf};
use std::process::Command;

use ugx::{Reader, UgxVersion};

fn workspace_file(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn temporary_output(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("ugx-cli-{name}-{}.ugx", std::process::id()))
}

fn run_conversion(source: &Path, target: UgxVersion, no_verify: bool) {
    let output = temporary_output(match target {
        UgxVersion::Hw1 => "hw1",
        UgxVersion::Hw2 => "hw2",
    });
    let version = match target {
        UgxVersion::Hw1 => "hw1",
        UgxVersion::Hw2 => "hw2",
    };
    let mut command = Command::new(env!("CARGO_BIN_EXE_ugx"));
    command.args([
        "convert",
        "--input",
        source.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
        "--version",
        version,
    ]);
    if no_verify {
        command.arg("--no-verify");
    }

    let result = command.output().unwrap();
    assert!(
        result.status.success(),
        "conversion failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let bytes = std::fs::read(&output).unwrap();
    assert_eq!(ugx::detect_version(&bytes).unwrap(), target);
    Reader::read(&bytes).unwrap();
    std::fs::remove_file(output).unwrap();
}

#[test]
fn direct_command_converts_both_fixture_versions() {
    run_conversion(&workspace_file("launcher_01.ugx"), UgxVersion::Hw2, true);
    run_conversion(
        &workspace_file("input/mesh_magnum_01.ugx"),
        UgxVersion::Hw1,
        false,
    );
}
