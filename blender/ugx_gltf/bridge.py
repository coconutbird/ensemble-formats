"""Safe subprocess bridge to the Rust ``ugx`` converter.

This module deliberately has no Blender dependency so its path discovery,
command construction, and error handling can be tested with normal Python.
"""

from __future__ import annotations

from dataclasses import dataclass
import json
import math
import os
from pathlib import Path
import shutil
import subprocess
from typing import Callable, Mapping, Sequence


CONVERTER_ENV = "UGX_CLI"


class BridgeError(RuntimeError):
    """Raised when the converter cannot be found or a command fails."""


class ConverterNotFoundError(BridgeError):
    """Raised when no usable Rust converter executable can be found."""


@dataclass(frozen=True)
class ConversionResult:
    """Captured output from a successful converter command."""

    command: tuple[str, ...]
    stdout: str
    stderr: str


def converter_filename(os_name: str | None = None) -> str:
    """Return the platform-specific converter filename."""
    return "ugx.exe" if (os_name or os.name) == "nt" else "ugx"


def resolve_converter(
    configured_path: str = "",
    *,
    package_dir: Path | None = None,
    environ: Mapping[str, str] | None = None,
    path_lookup: Callable[[str], str | None] = shutil.which,
) -> Path:
    """Resolve the Rust converter from preferences, bundle, environment, or PATH.

    Resolution order is the explicit Blender preference, a binary bundled at
    ``bin/ugx``, the ``UGX_CLI`` environment variable, then ``PATH``.

    Raises:
        ConverterNotFoundError: If none of the candidates names an existing file.
    """
    environment = os.environ if environ is None else environ
    root = Path(__file__).resolve().parent if package_dir is None else Path(package_dir)
    candidates: list[Path] = []

    if configured_path.strip():
        candidates.append(Path(configured_path).expanduser())
    candidates.append(root / "bin" / converter_filename())
    if environment.get(CONVERTER_ENV, "").strip():
        candidates.append(Path(environment[CONVERTER_ENV]).expanduser())
    if located := path_lookup("ugx"):
        candidates.append(Path(located))

    for candidate in candidates:
        if candidate.is_file():
            return candidate.resolve()

    searched = ", ".join(str(path) for path in candidates) or "no candidate paths"
    raise ConverterNotFoundError(
        "Rust UGX converter not found. Set it in the extension preferences, "
        f"set {CONVERTER_ENV}, or add 'ugx' to PATH. Searched: {searched}"
    )


def converter_version(executable: Path, *, timeout_seconds: int = 30) -> str:
    """Return the converter's version banner.

    Raises:
        BridgeError: If the executable cannot be started, times out, or exits unsuccessfully.
    """
    result = run_converter(executable, ["--version"], timeout_seconds=timeout_seconds)
    return result.stdout or result.stderr


def inspect_ugx(
    executable: Path,
    source: Path,
    *,
    verify_checksums: bool = True,
    timeout_seconds: int = 300,
) -> dict[str, object]:
    """Return the Rust converter's machine-readable UGX summary.

    Raises:
        BridgeError: If inspection fails or the converter returns malformed JSON.
    """
    arguments = ["info", "--input", str(source), "--json"]
    if not verify_checksums:
        arguments.append("--no-verify")
    result = run_converter(executable, arguments, timeout_seconds=timeout_seconds)
    try:
        summary = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise BridgeError(f"Converter returned invalid info JSON: {error}") from error
    if not isinstance(summary, dict) or summary.get("format") != "ugx":
        raise BridgeError("Converter returned an unexpected UGX info document")
    return summary


def convert_ugx_to_gltf(
    executable: Path,
    source: Path,
    destination: Path,
    *,
    include_skeleton: bool = True,
    verify_checksums: bool = True,
    timeout_seconds: int = 300,
) -> ConversionResult:
    """Convert a UGX file to an embedded glTF document using Rust.

    Raises:
        BridgeError: If conversion fails.
    """
    arguments = ["to-gltf", "--input", str(source), "--output", str(destination)]
    if not include_skeleton:
        arguments.append("--no-skeleton")
    if not verify_checksums:
        arguments.append("--no-verify")
    return run_converter(executable, arguments, timeout_seconds=timeout_seconds)


def convert_gltf_to_ugx(
    executable: Path,
    source: Path,
    destination: Path,
    *,
    version: str,
    include_skeleton: bool = True,
    model_scale: float = 1.0,
    mirror_x: bool = False,
    timeout_seconds: int = 300,
) -> ConversionResult:
    """Convert a glTF or GLB file to UGX using Rust.

    Raises:
        BridgeError: If ``version`` is unsupported or conversion fails.
    """
    normalized_version = version.lower()
    if normalized_version not in {"hw1", "hw2"}:
        raise BridgeError(f"Unsupported UGX target version: {version}")
    if not math.isfinite(model_scale) or model_scale <= 0.0:
        raise BridgeError("UGX model scale must be finite and greater than zero")
    arguments = [
        "from-gltf",
        "--input",
        str(source),
        "--output",
        str(destination),
        "--version",
        normalized_version,
    ]
    if not include_skeleton:
        arguments.append("--no-skeleton")
    if model_scale != 1.0:
        arguments.extend(("--scale", format(model_scale, ".17g")))
    if mirror_x:
        arguments.append("--mirror-x")
    return run_converter(executable, arguments, timeout_seconds=timeout_seconds)


def run_converter(
    executable: Path,
    arguments: Sequence[str],
    *,
    timeout_seconds: int,
) -> ConversionResult:
    """Run a converter command without a shell and capture its output.

    Raises:
        BridgeError: If the process cannot start, times out, or exits unsuccessfully.
    """
    command = (str(executable), *(str(argument) for argument in arguments))
    options: dict[str, object] = {
        "capture_output": True,
        "text": True,
        "encoding": "utf-8",
        "errors": "replace",
        "timeout": timeout_seconds,
        "check": False,
    }
    if os.name == "nt":
        options["creationflags"] = getattr(subprocess, "CREATE_NO_WINDOW", 0)

    try:
        completed = subprocess.run(command, **options)
    except OSError as error:
        raise BridgeError(f"Could not start Rust UGX converter: {error}") from error
    except subprocess.TimeoutExpired as error:
        raise BridgeError(
            f"Rust UGX converter timed out after {timeout_seconds} seconds"
        ) from error

    stdout = completed.stdout.strip()
    stderr = completed.stderr.strip()
    if completed.returncode != 0:
        detail = stderr or stdout or f"exit code {completed.returncode}"
        raise BridgeError(f"Rust UGX converter failed: {detail}")
    return ConversionResult(command, stdout, stderr)
