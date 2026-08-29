"""Minimal GLB reader/writer used to preserve scene-level UGX extras."""

from __future__ import annotations

from dataclasses import dataclass
import json
from pathlib import Path
import struct
from typing import Any, Mapping


GLB_MAGIC = b"glTF"
GLB_VERSION = 2
JSON_CHUNK = 0x4E4F534A
BIN_CHUNK = 0x004E4942


class GlbError(ValueError):
    """Raised for a malformed or unsupported GLB container."""


@dataclass
class GlbDocument:
    """Decoded GLB JSON plus all non-JSON chunks in original order."""

    json_document: dict[str, Any]
    other_chunks: list[tuple[int, bytes]]


def decode_glb(data: bytes) -> GlbDocument:
    """Decode a GLB 2.0 container.

    Raises:
        GlbError: If the header, chunks, UTF-8, or JSON are malformed.
    """
    if len(data) < 12:
        raise GlbError("GLB header is truncated")
    magic, version, declared_length = struct.unpack_from("<4sII", data)
    if magic != GLB_MAGIC:
        raise GlbError("Invalid GLB magic")
    if version != GLB_VERSION:
        raise GlbError(f"Unsupported GLB version: {version}")
    if declared_length != len(data):
        raise GlbError(
            f"GLB length mismatch: header says {declared_length}, file has {len(data)}"
        )

    offset = 12
    json_document: dict[str, Any] | None = None
    other_chunks: list[tuple[int, bytes]] = []
    while offset < len(data):
        if len(data) - offset < 8:
            raise GlbError("GLB chunk header is truncated")
        chunk_length, chunk_type = struct.unpack_from("<II", data, offset)
        chunk_offset = offset
        offset += 8
        if chunk_length % 4:
            raise GlbError(
                f"GLB chunk at offset {chunk_offset} has unaligned length {chunk_length}"
            )
        if chunk_offset == 12 and chunk_type != JSON_CHUNK:
            raise GlbError("The first GLB chunk must contain JSON")
        end = offset + chunk_length
        if end > len(data):
            raise GlbError("GLB chunk extends beyond the declared file length")
        payload = data[offset:end]
        offset = end
        if chunk_type == JSON_CHUNK:
            if json_document is not None:
                raise GlbError("GLB contains more than one JSON chunk")
            try:
                decoded = json.loads(payload.rstrip(b" \t\r\n\0").decode("utf-8"))
            except (UnicodeDecodeError, json.JSONDecodeError) as error:
                raise GlbError(f"Invalid GLB JSON chunk: {error}") from error
            if not isinstance(decoded, dict):
                raise GlbError("GLB JSON root must be an object")
            json_document = decoded
        else:
            other_chunks.append((chunk_type, payload))

    if json_document is None:
        raise GlbError("GLB is missing its JSON chunk")
    return GlbDocument(json_document, other_chunks)


def encode_glb(document: GlbDocument) -> bytes:
    """Encode a GLB document with checked 32-bit container lengths.

    Raises:
        GlbError: If the encoded container exceeds GLB's 32-bit length fields.
    """
    if any(chunk_type == JSON_CHUNK for chunk_type, _ in document.other_chunks):
        raise GlbError("GLB contains more than one JSON chunk")
    if sum(chunk_type == BIN_CHUNK for chunk_type, _ in document.other_chunks) > 1:
        raise GlbError("GLB contains more than one binary chunk")

    json_payload = json.dumps(
        document.json_document,
        ensure_ascii=False,
        separators=(",", ":"),
    ).encode("utf-8")
    json_payload += b" " * ((-len(json_payload)) % 4)

    chunks = [(JSON_CHUNK, json_payload), *document.other_chunks]
    encoded_chunks: list[bytes] = []
    total_length = 12
    for chunk_type, raw_payload in chunks:
        payload = raw_payload + b"\0" * ((-len(raw_payload)) % 4)
        if len(payload) > 0xFFFF_FFFF:
            raise GlbError("GLB chunk is too large")
        encoded = struct.pack("<II", len(payload), chunk_type) + payload
        encoded_chunks.append(encoded)
        total_length += len(encoded)
        if total_length > 0xFFFF_FFFF:
            raise GlbError("GLB container is too large")

    return struct.pack("<4sII", GLB_MAGIC, GLB_VERSION, total_length) + b"".join(
        encoded_chunks
    )


def update_scene_extras(path: Path, extras: Mapping[str, object]) -> None:
    """Merge UGX metadata into the active GLB scene's ``extras`` object.

    Raises:
        GlbError: If the GLB or scene structure is malformed.
        OSError: If the file cannot be read or replaced.
    """
    document = decode_glb(path.read_bytes())
    root = document.json_document
    scenes = root.get("scenes")
    if not isinstance(scenes, list) or not scenes:
        raise GlbError("Exported GLB contains no scenes")
    scene_index = root.get("scene", 0)
    if not isinstance(scene_index, int) or not 0 <= scene_index < len(scenes):
        raise GlbError("Exported GLB has an invalid active scene index")
    scene = scenes[scene_index]
    if not isinstance(scene, dict):
        raise GlbError("Exported GLB scene is not an object")
    current_extras = scene.get("extras")
    if current_extras is None:
        current_extras = {}
        scene["extras"] = current_extras
    if not isinstance(current_extras, dict):
        raise GlbError("Exported GLB scene extras are not an object")
    current_extras.update(extras)
    path.write_bytes(encode_glb(document))


def update_ugx_extras(
    path: Path,
    scene_extras: Mapping[str, object],
    material_extras: Mapping[str, Mapping[str, object]],
) -> None:
    """Write exact scene and named-material UGX metadata into an exported GLB.

    Blender ID properties use signed 32-bit integers, while UGX has unsigned
    32-bit hashes and indices. This final JSON merge restores their exact types
    after Blender's glTF exporter has finished.

    Raises:
        GlbError: If the GLB scene/material structure or name mapping is invalid.
        OSError: If the file cannot be read or replaced.
    """
    document = decode_glb(path.read_bytes())
    root = document.json_document

    scenes = root.get("scenes")
    if not isinstance(scenes, list) or not scenes:
        raise GlbError("Exported GLB contains no scenes")
    scene_index = root.get("scene", 0)
    if not isinstance(scene_index, int) or not 0 <= scene_index < len(scenes):
        raise GlbError("Exported GLB has an invalid active scene index")
    scene = scenes[scene_index]
    if not isinstance(scene, dict):
        raise GlbError("Exported GLB scene is not an object")
    current_scene_extras = scene.setdefault("extras", {})
    if not isinstance(current_scene_extras, dict):
        raise GlbError("Exported GLB scene extras are not an object")
    current_scene_extras.update(scene_extras)

    materials = root.get("materials", [])
    if not isinstance(materials, list):
        raise GlbError("Exported GLB materials are not an array")
    exported_by_name = {}
    for material in materials:
        if not isinstance(material, dict):
            raise GlbError("Exported GLB material is not an object")
        name = material.get("name")
        if isinstance(name, str):
            exported_by_name[name] = material

    missing = sorted(set(material_extras) - set(exported_by_name))
    if missing:
        raise GlbError(
            "Exported GLB is missing UGX materials: " + ", ".join(missing)
        )
    for name, exact_extras in material_extras.items():
        material = exported_by_name[name]
        current = material.setdefault("extras", {})
        if not isinstance(current, dict):
            raise GlbError(f"Exported material '{name}' extras are not an object")
        for key in tuple(current):
            if key.startswith("ugx_"):
                del current[key]
        current.update(exact_extras)

    path.write_bytes(encode_glb(document))
