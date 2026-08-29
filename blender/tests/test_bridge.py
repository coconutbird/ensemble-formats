"""Tests for the Blender-independent Rust converter bridge."""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest


MODULE_PATH = Path(__file__).parents[1] / "ugx_gltf" / "bridge.py"
SPEC = importlib.util.spec_from_file_location("ugx_gltf_bridge", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
bridge = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = bridge
SPEC.loader.exec_module(bridge)


class ResolveConverterTests(unittest.TestCase):
    """Exercise converter discovery without importing Blender."""

    def test_explicit_path_wins(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            explicit = root / bridge.converter_filename()
            explicit.touch()
            bundled = root / "package" / "bin" / bridge.converter_filename()
            bundled.parent.mkdir(parents=True)
            bundled.touch()

            resolved = bridge.resolve_converter(
                str(explicit),
                package_dir=root / "package",
                environ={},
                path_lookup=lambda _name: None,
            )

            self.assertEqual(resolved, explicit.resolve())

    def test_bundled_path_precedes_environment(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            bundled = root / "bin" / bridge.converter_filename()
            bundled.parent.mkdir()
            bundled.touch()
            environment = root / "environment-ugx"
            environment.touch()

            resolved = bridge.resolve_converter(
                package_dir=root,
                environ={bridge.CONVERTER_ENV: str(environment)},
                path_lookup=lambda _name: None,
            )

            self.assertEqual(resolved, bundled.resolve())

    def test_missing_converter_explains_configuration(self):
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaises(bridge.ConverterNotFoundError) as raised:
                bridge.resolve_converter(
                    package_dir=Path(temporary),
                    environ={},
                    path_lookup=lambda _name: None,
                )

        self.assertIn(bridge.CONVERTER_ENV, str(raised.exception))


class InspectTests(unittest.TestCase):
    """Validate machine-readable converter inspection handling."""

    def test_inspect_parses_summary(self):
        expected = {"format": "ugx", "version": "hw2", "sections": 2}
        original = bridge.run_converter
        bridge.run_converter = lambda *_args, **_kwargs: bridge.ConversionResult(
            ("ugx",), json.dumps(expected), ""
        )
        try:
            actual = bridge.inspect_ugx(Path("ugx"), Path("model.ugx"))
        finally:
            bridge.run_converter = original

        self.assertEqual(actual, expected)

    def test_rejects_unexpected_summary(self):
        original = bridge.run_converter
        bridge.run_converter = lambda *_args, **_kwargs: bridge.ConversionResult(
            ("ugx",), "{}", ""
        )
        try:
            with self.assertRaises(bridge.BridgeError):
                bridge.inspect_ugx(Path("ugx"), Path("model.ugx"))
        finally:
            bridge.run_converter = original


class ExportCommandTests(unittest.TestCase):
    """Check authoring transforms are forwarded without a shell."""

    def test_stumpy_transform_arguments(self):
        captured = []
        original = bridge.run_converter

        def capture(_executable, arguments, *, timeout_seconds):
            del timeout_seconds
            captured.extend(arguments)
            return bridge.ConversionResult(("ugx",), "", "")

        bridge.run_converter = capture
        try:
            bridge.convert_gltf_to_ugx(
                Path("ugx"),
                Path("source.glb"),
                Path("output.ugx"),
                version="HW1",
                model_scale=1.575,
                mirror_x=True,
            )
        finally:
            bridge.run_converter = original

        self.assertIn("--scale", captured)
        self.assertEqual(captured[captured.index("--scale") + 1], "1.575")
        self.assertIn("--mirror-x", captured)

    def test_rejects_invalid_scale(self):
        with self.assertRaises(bridge.BridgeError):
            bridge.convert_gltf_to_ugx(
                Path("ugx"),
                Path("source.glb"),
                Path("output.ugx"),
                version="HW1",
                model_scale=0.0,
            )


if __name__ == "__main__":
    unittest.main()
