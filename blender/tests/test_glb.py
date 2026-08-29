"""Tests for scene metadata preservation in temporary GLB files."""

from __future__ import annotations

import importlib.util
from pathlib import Path
import struct
import sys
import tempfile
import unittest


MODULE_PATH = Path(__file__).parents[1] / "ugx_gltf" / "glb.py"
SPEC = importlib.util.spec_from_file_location("ugx_gltf_glb", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
glb = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = glb
SPEC.loader.exec_module(glb)


class GlbTests(unittest.TestCase):
    """Check GLB validation, preservation, and extras merging."""

    def test_roundtrip_preserves_binary_chunk(self):
        binary_type = 0x004E4942
        document = glb.GlbDocument(
            {"asset": {"version": "2.0"}, "scene": 0, "scenes": [{}]},
            [(binary_type, b"\x01\x02\x03\x00")],
        )

        decoded = glb.decode_glb(glb.encode_glb(document))

        self.assertEqual(decoded.json_document, document.json_document)
        self.assertEqual(decoded.other_chunks, document.other_chunks)

    def test_update_scene_extras_merges_existing_values(self):
        document = glb.GlbDocument(
            {
                "asset": {"version": "2.0"},
                "scene": 0,
                "scenes": [{"extras": {"keep": True}}],
            },
            [],
        )
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "model.glb"
            path.write_bytes(glb.encode_glb(document))

            glb.update_scene_extras(path, {"ugx_max_instances": 7})

            updated = glb.decode_glb(path.read_bytes()).json_document
        self.assertEqual(
            updated["scenes"][0]["extras"],
            {"keep": True, "ugx_max_instances": 7},
        )

    def test_rejects_header_length_mismatch(self):
        malformed = struct.pack("<4sII", b"glTF", 2, 16) + b"\0" * 4
        malformed = malformed[:-1]

        with self.assertRaises(glb.GlbError):
            glb.decode_glb(malformed)

    def test_rejects_unaligned_chunk_length(self):
        document = glb.GlbDocument({"asset": {"version": "2.0"}}, [])
        malformed = bytearray(glb.encode_glb(document))
        malformed[12:16] = struct.pack("<I", 3)

        with self.assertRaisesRegex(glb.GlbError, "unaligned length"):
            glb.decode_glb(bytes(malformed))

    def test_requires_json_chunk_first(self):
        document = glb.GlbDocument({"asset": {"version": "2.0"}}, [])
        malformed = bytearray(glb.encode_glb(document))
        malformed[16:20] = struct.pack("<I", 0x004E4942)

        with self.assertRaisesRegex(glb.GlbError, "first GLB chunk"):
            glb.decode_glb(bytes(malformed))


if __name__ == "__main__":
    unittest.main()
