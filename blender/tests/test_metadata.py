"""Tests for Blender-independent UGX metadata normalization."""

from __future__ import annotations

import importlib.util
from pathlib import Path
import unittest


MODULE_PATH = Path(__file__).parents[1] / "ugx_gltf" / "metadata.py"
SPEC = importlib.util.spec_from_file_location("ugx_gltf_metadata", MODULE_PATH)
metadata = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(metadata)


class FakeIdGroup:
    """Minimal stand-in for Blender's non-Mapping IDPropertyGroup."""

    def __init__(self, values):
        self.values = values

    def items(self):
        return self.values.items()


class MetadataTests(unittest.TestCase):
    """Cover exact integer and nested-record handling used by the plugin."""

    def test_plain_value_recurses_through_id_property_groups(self):
        value = FakeIdGroup(
            {"diffuse": [FakeIdGroup({"name": "a.dds", "channel": 1, "flags": 7})]}
        )

        self.assertEqual(
            metadata.plain_value(value),
            {"diffuse": [{"name": "a.dds", "channel": 1, "flags": 7}]},
        )

    def test_plain_value_decodes_blender_nested_extras_repr(self):
        value = "{'enabled': True, 'maps': [{'name': 'a.dds', 'flags': 7}]}"

        self.assertEqual(
            metadata.plain_value(value),
            {"enabled": True, "maps": [{"name": "a.dds", "flags": 7}]},
        )

    def test_unsigned_fields_accept_full_ugx_ranges(self):
        self.assertEqual(metadata.parse_uint("0xFFFFFFFF", 32, "hash"), 0xFFFFFFFF)
        self.assertEqual(
            metadata.parse_uint("0xFFFFFFFFFFFFFFFF", 64, "flags"),
            0xFFFFFFFFFFFFFFFF,
        )
        with self.assertRaises(metadata.MetadataError):
            metadata.parse_uint("0x100000000", 32, "hash")
        with self.assertRaises(metadata.MetadataError):
            metadata.parse_uint("-1", 32, "hash")

    def test_map_normalization_preserves_all_editable_fields(self):
        maps = metadata.normalize_maps(
            FakeIdGroup(
                {
                    "normal": [
                        FakeIdGroup(
                            {"name": r"art\unit_nm.dds", "channel": -2, "flags": 65535}
                        )
                    ]
                }
            )
        )

        self.assertEqual(
            maps,
            {
                "normal": [
                    {"name": r"art\unit_nm.dds", "channel": 0, "flags": 65535}
                ]
            },
        )

    def test_hogan_flag_suffix_can_be_edited_without_changing_family(self):
        name = "HOGAN_STANDARD_00080000A8000960.ufx"
        flags = metadata.shader_flags_from_name(name)

        self.assertEqual(flags, 0x00080000A8000960)
        self.assertEqual(
            metadata.replace_shader_flags(name, flags ^ (1 << 32)),
            "HOGAN_STANDARD_00080001A8000960.ufx",
        )

    def test_hogan_normalization_preserves_unsigned_hash(self):
        normalized = metadata.normalize_hogan(
            FakeIdGroup(
                {
                    "shader_permutations": [
                        FakeIdGroup(
                            {
                                "name": "HOGAN_STANDARD_0000000000000000",
                                "hash": 4294967295.0,
                            }
                        )
                    ],
                    "ufx_version": 9,
                    "textures": r"art\model_[al]",
                }
            )
        )

        self.assertEqual(normalized["shader_permutations"][0]["hash"], 0xFFFFFFFF)
        self.assertEqual(normalized["textures"], r"art\model_[al]")


if __name__ == "__main__":
    unittest.main()
