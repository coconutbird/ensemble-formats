"""Static tests for Blender extension sources and manifest metadata."""

from __future__ import annotations

from pathlib import Path
import tomllib
import unittest


EXTENSION_DIR = Path(__file__).parents[1] / "ugx_gltf"


class ExtensionSourceTests(unittest.TestCase):
    """Catch syntax and packaging errors without requiring a Blender install."""

    def test_all_python_sources_compile(self):
        for path in sorted(EXTENSION_DIR.glob("*.py")):
            with self.subTest(path=path.name):
                compile(path.read_text(encoding="utf-8"), str(path), "exec")

    def test_manifest_has_required_extension_fields(self):
        manifest = tomllib.loads(
            (EXTENSION_DIR / "blender_manifest.toml").read_text(encoding="utf-8")
        )

        self.assertEqual(manifest["schema_version"], "1.0.0")
        self.assertEqual(manifest["id"], "ugx_gltf")
        self.assertEqual(manifest["type"], "add-on")
        self.assertGreaterEqual(manifest["blender_version_min"], "4.2.0")
        self.assertIn("files", manifest["permissions"])


if __name__ == "__main__":
    unittest.main()
