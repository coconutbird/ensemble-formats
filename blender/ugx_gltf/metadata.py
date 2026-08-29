"""Blender-independent helpers for editable UGX glTF metadata."""

from __future__ import annotations

import ast
from collections.abc import Iterable
import json
import math
import re


FLOAT32_MAX = 3.4028234663852886e38

MAP_TYPES = (
    ("diffuse", "Diffuse", "Base color / albedo texture"),
    ("normal", "Normal", "Tangent-space normal texture"),
    ("gloss", "Gloss", "Gloss or specular-power texture"),
    ("opacity", "Opacity", "Opacity or alpha-mask texture"),
    ("xform", "Transform", "UV transform or detail texture"),
    ("emissive", "Emissive", "Self-illumination texture"),
    ("ao", "Ambient Occlusion", "Ambient-occlusion texture"),
    ("env", "Environment", "Environment or reflection texture"),
    ("envmask", "Environment Mask", "Reflection-intensity mask"),
    ("emxform", "Emissive Transform", "Emissive UV-transform texture"),
    ("distortion", "Distortion", "Distortion or refraction texture"),
    ("highlight", "Highlight", "Highlight or rim-light texture"),
    ("modulate", "Modulate", "Modulation or blend texture"),
)
MAP_TYPE_IDS = tuple(item[0] for item in MAP_TYPES)
MAP_TYPE_LABELS = {item[0]: item[1] for item in MAP_TYPES}

LEGACY_FLAGS = (
    ("color_gloss", "Color Gloss", 0, "Take specular color from the gloss map"),
    ("opacity_valid", "Use Opacity", 1, "Apply the material opacity value"),
    ("two_sided", "Two Sided", 2, "Render front and back faces"),
    ("disable_shadows", "Disable Shadow Casting", 3, "Do not cast shadows"),
    ("global_env", "Global Environment", 4, "Force global environment mapping"),
    ("terrain_conform", "Terrain Conform", 5, "Enable terrain-conforming rendering"),
    ("local_reflection", "Local Reflection", 6, "Enable local reflections"),
    (
        "disable_shadow_reception",
        "Disable Shadow Reception",
        7,
        "Do not receive shadows from other objects",
    ),
)
LEGACY_KNOWN_MASK = sum(1 << item[2] for item in LEGACY_FLAGS)

HOGAN_FLAGS = (
    ("height_blend", "Height Blend", 23, "Height-blend terrain parameters"),
    ("extra_texture_layer", "Extra Texture Layer", 27, "Additional texture layer"),
    ("roughness_channel", "Roughness Channel", 28, "Roughness channel override"),
    ("emissive_sub_a", "Emissive Variant A", 29, "Emissive sub-feature A"),
    ("emissive_sub_b", "Emissive Variant B", 30, "Emissive sub-feature B"),
    ("emissive", "Emissive", 32, "Emissive map and intensity"),
    ("scroll_anim", "Scroll Animation", 35, "Animated emissive UV scrolling"),
    ("per_channel_uv", "Per-channel UV", 38, "Independent T5 UV scale"),
    ("vertex_anim_a", "Vertex Animation A", 41, "Vertex animation variant A"),
    ("vertex_anim_b", "Vertex Animation B", 44, "Vertex animation variant B"),
    (
        "simplified_texturing",
        "Simplified Texturing",
        46,
        "Simplified texturing shader path",
    ),
    (
        "material_override",
        "Material Override",
        49,
        "Material override constant-buffer block",
    ),
    ("reduced_texturing", "Reduced Texturing", 53, "Reduced texturing shader path"),
)
HOGAN_KNOWN_MASK = sum(1 << item[2] for item in HOGAN_FLAGS)

BLEND_TYPES = (
    ("0", "Alpha to Coverage", "Default coverage-based transparency"),
    ("1", "Additive", "Add source color to the framebuffer"),
    ("2", "Alpha Blend", "Standard over/alpha blending"),
    ("3", "Alpha Test", "Discard pixels below the alpha cutoff"),
)

PARAMETER_STAGES = (
    ("VS", "Vertex", "Vertex-shader constant buffer"),
    ("PS", "Pixel", "Pixel-shader constant buffer"),
    ("HS", "Hull", "Hull-shader constant buffer"),
    ("DS", "Domain", "Domain-shader constant buffer"),
    ("GS", "Geometry", "Geometry-shader constant buffer"),
)

_FLAGS_SUFFIX = re.compile(r"(?i)([0-9a-f]{16})(?=\.ufx$|$)")


class MetadataError(ValueError):
    """Raised when an editable metadata value cannot fit the UGX field."""


def plain_value(value):
    """Recursively convert Blender ID properties into JSON-compatible values."""
    if isinstance(value, str):
        stripped = value.strip()
        if stripped.startswith(("{", "[")):
            for decoder in (json.loads, ast.literal_eval):
                try:
                    return plain_value(decoder(stripped))
                except (ValueError, SyntaxError, json.JSONDecodeError):
                    continue
        return value
    if value is None or isinstance(value, (bool, int, float)):
        return value
    items = getattr(value, "items", None)
    if callable(items):
        return {str(key): plain_value(item) for key, item in items()}
    if isinstance(value, (bytes, bytearray, memoryview)):
        return list(value)
    if isinstance(value, Iterable):
        return [plain_value(item) for item in value]
    return value


def finite_float(value, default: float) -> float:
    """Return a finite float, falling back when imported metadata is malformed."""
    try:
        result = float(value)
    except (TypeError, ValueError, OverflowError):
        return default
    return result if math.isfinite(result) else default


def integer(value, default: int) -> int:
    """Return an exact integer, falling back for booleans and fractional values."""
    if isinstance(value, bool):
        return default
    try:
        result = int(value)
    except (TypeError, ValueError, OverflowError):
        return default
    try:
        return result if result == value else default
    except TypeError:
        return default


def vector(value, size: int, default: tuple[float, ...]) -> tuple[float, ...]:
    """Normalize an imported numeric vector to a fixed component count."""
    raw = plain_value(value)
    if not isinstance(raw, list) or len(raw) < size:
        return default
    return tuple(finite_float(raw[index], default[index]) for index in range(size))


def parse_uint(value, bits: int, label: str) -> int:
    """Parse decimal or hexadecimal text as an unsigned integer field."""
    if isinstance(value, bool):
        raise MetadataError(f"{label} must be an unsigned integer")
    if isinstance(value, int):
        parsed = value
    else:
        text = str(value).strip().replace("_", "")
        if not text:
            raise MetadataError(f"{label} cannot be empty")
        try:
            parsed = int(text, 0 if text.lower().startswith("0x") else 10)
        except ValueError as error:
            raise MetadataError(
                f"{label} must be decimal or start with 0x for hexadecimal"
            ) from error
    maximum = (1 << bits) - 1
    if not 0 <= parsed <= maximum:
        raise MetadataError(f"{label} must be between 0 and {maximum}")
    return parsed


def format_hex(value, bits: int) -> str:
    """Format an imported unsigned value as fixed-width hexadecimal text."""
    parsed = integer(value, 0) & ((1 << bits) - 1)
    return f"0x{parsed:0{bits // 4}X}"


def normalize_maps(value) -> dict[str, list[dict[str, object]]]:
    """Normalize the nested ``ugx_maps`` custom property."""
    raw = plain_value(value)
    if not isinstance(raw, dict):
        return {}
    result: dict[str, list[dict[str, object]]] = {}
    for map_type in MAP_TYPE_IDS:
        entries = raw.get(map_type)
        if not isinstance(entries, list):
            continue
        normalized = []
        for entry in entries:
            if not isinstance(entry, dict):
                continue
            normalized.append(
                {
                    "name": str(entry.get("name", "")),
                    "channel": max(0, min(32_767, integer(entry.get("channel"), 0))),
                    "flags": max(0, min(65_535, integer(entry.get("flags"), 0))),
                }
            )
        if normalized:
            result[map_type] = normalized
    return result


def normalize_uvw(value) -> list[tuple[float, float, float]]:
    """Normalize all thirteen legacy UVW velocity slots."""
    raw = plain_value(value)
    raw = raw if isinstance(raw, list) else []
    return [
        vector(raw[index], 3, (0.0, 0.0, 0.0))
        if index < len(raw)
        else (0.0, 0.0, 0.0)
        for index in range(len(MAP_TYPES))
    ]


def shader_flags_from_name(name: str) -> int | None:
    """Extract the 64-bit feature-mask suffix from a Hogan permutation name."""
    match = _FLAGS_SUFFIX.search(name.strip())
    return int(match.group(1), 16) if match else None


def replace_shader_flags(name: str, flags: int) -> str:
    """Replace a Hogan permutation's 64-bit suffix while retaining its family."""
    replacement = f"{flags & 0xFFFF_FFFF_FFFF_FFFF:016X}"
    normalized_name = name.strip()
    match = _FLAGS_SUFFIX.search(normalized_name)
    if not match:
        raise MetadataError("Permutation name does not end in a 16-digit flag mask")
    return (
        f"{normalized_name[:match.start()]}{replacement}"
        f"{normalized_name[match.end():]}"
    )


def normalize_hogan(value) -> dict[str, object]:
    """Normalize the nested ``ugx_hogan`` custom property."""
    raw = plain_value(value)
    raw = raw if isinstance(raw, dict) else {}
    permutations = []
    for entry in raw.get("shader_permutations", []):
        if isinstance(entry, dict):
            permutations.append(
                {
                    "name": str(entry.get("name", "")),
                    "hash": integer(entry.get("hash"), 0) & 0xFFFF_FFFF,
                }
            )

    result: dict[str, object] = {
        "shader_permutations": permutations,
        "ufx_version": integer(raw.get("ufx_version"), 9) & 0xFFFF_FFFF,
        "blend_mode": integer(raw.get("blend_mode"), 0) & 0xFFFF_FFFF,
        "shadow_requires_consts": bool(raw.get("shadow_requires_consts", False)),
        "skinned": bool(raw.get("skinned", False)),
        "terrain_blending": bool(raw.get("terrain_blending", False)),
        "shader_flags": str(raw.get("shader_flags", "")),
        "textures": str(raw.get("textures", "")),
    }
    for key in ("vs_cb", "ps_cb"):
        parameters = plain_value(raw.get(key))
        result[key] = parameters if isinstance(parameters, dict) else {}
    for key in ("vs_params", "ps_params", "hs_params", "ds_params", "gs_params"):
        registers = plain_value(raw.get(key))
        result[key] = registers if isinstance(registers, list) else []
    return result
