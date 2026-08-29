"""Build a platform-specific Blender extension containing the Rust converter."""

from __future__ import annotations

import argparse
from pathlib import Path
import platform
import shutil
import stat
import subprocess
import sys
import tempfile
import tomllib
import zipfile


EXTENSION_FILES = (
    "__init__.py",
    "blender_manifest.toml",
    "bridge.py",
    "glb.py",
    "operators.py",
    "preferences.py",
    "properties.py",
    "README.md",
    "ui.py",
)


def blender_platform() -> str:
    """Return Blender's platform identifier for the current interpreter."""
    machine = platform.machine().lower()
    is_arm = machine in {"arm64", "aarch64"}
    if sys.platform == "win32":
        return "windows-arm64" if is_arm else "windows-x64"
    if sys.platform == "darwin":
        return "macos-arm64" if is_arm else "macos-x64"
    if sys.platform.startswith("linux") and not is_arm:
        return "linux-x64"
    raise RuntimeError(f"No Blender extension platform for {sys.platform}/{machine}")


def converter_filename() -> str:
    """Return the Rust binary filename for the host platform."""
    return "ugx.exe" if sys.platform == "win32" else "ugx"


def manifest_for_platform(source: str, platform_id: str) -> str:
    """Add a platform restriction to a source manifest."""
    marker = 'blender_version_min = "4.2.0"\n'
    if marker not in source:
        raise RuntimeError("Cannot find blender_version_min in extension manifest")
    if "\nplatforms = " in source:
        raise RuntimeError("Source manifest already declares platforms")
    return source.replace(marker, f'{marker}platforms = ["{platform_id}"]\n', 1)


def build_extension(workspace: Path, output_dir: Path, release: bool = True) -> Path:
    """Build the Rust CLI and package the extension for this platform."""
    extension_dir = workspace / "blender" / "ugx_gltf"
    manifest_path = extension_dir / "blender_manifest.toml"
    manifest = tomllib.loads(manifest_path.read_text(encoding="utf-8"))
    platform_id = blender_platform()
    profile = "release" if release else "debug"

    command = ["cargo", "build", "--locked", "-p", "ugx-cli"]
    if release:
        command.append("--release")
    subprocess.run(command, cwd=workspace, check=True)

    binary = workspace / "target" / profile / converter_filename()
    if not binary.is_file():
        raise RuntimeError(f"Cargo did not produce {binary}")

    output_dir.mkdir(parents=True, exist_ok=True)
    archive = output_dir / (
        f"{manifest['id']}-{manifest['version']}-{platform_id}.zip"
    )
    with tempfile.TemporaryDirectory(prefix="ugx_gltf_package_") as temporary:
        staging = Path(temporary)
        for relative in EXTENSION_FILES:
            source = extension_dir / relative
            if not source.is_file():
                raise RuntimeError(f"Missing extension file: {source}")
            shutil.copy2(source, staging / relative)
        staged_manifest = staging / "blender_manifest.toml"
        staged_manifest.write_text(
            manifest_for_platform(
                staged_manifest.read_text(encoding="utf-8"), platform_id
            ),
            encoding="utf-8",
        )
        staged_binary = staging / "bin" / binary.name
        staged_binary.parent.mkdir()
        shutil.copy2(binary, staged_binary)
        staged_binary.chmod(staged_binary.stat().st_mode | stat.S_IXUSR)

        with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as package:
            for path in sorted(staging.rglob("*")):
                if path.is_file():
                    package.write(path, path.relative_to(staging).as_posix())
    return archive


def parse_args() -> argparse.Namespace:
    """Parse command-line package options."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--debug",
        action="store_true",
        help="bundle a debug Rust binary instead of a release build",
    )
    parser.add_argument(
        "--output-dir",
        type=Path,
        help="archive directory (default: <workspace>/dist)",
    )
    return parser.parse_args()


def main() -> int:
    """Build an installable Blender extension archive."""
    args = parse_args()
    workspace = Path(__file__).resolve().parents[1]
    output_dir = args.output_dir or workspace / "dist"
    archive = build_extension(workspace, output_dir, release=not args.debug)
    print(archive)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
