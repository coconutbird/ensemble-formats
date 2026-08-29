"""Headless Blender smoke test for the complete UGX import/export pipeline.

Run from the workspace root after building ``ugx-cli``:

    blender --background --python blender/tests/blender_smoke.py
"""

from __future__ import annotations

import json
import os
from pathlib import Path
import sys
import tempfile

import bpy


WORKSPACE = Path(__file__).resolve().parents[2]
BLENDER_DIR = WORKSPACE / "blender"
if str(BLENDER_DIR) not in sys.path:
    sys.path.insert(0, str(BLENDER_DIR))

import ugx_gltf  # noqa: E402
from ugx_gltf import bridge  # noqa: E402


def clear_scene() -> None:
    """Delete every object in the active test scene."""
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)


def build_triangle() -> bpy.types.Object:
    """Create one editable mesh and material with UGX metadata."""
    mesh = bpy.data.meshes.new("smoke_mesh")
    mesh.from_pydata(
        [(0.0, 0.0, 0.0), (1.0, 0.0, 0.0), (0.0, 1.0, 0.0)],
        [],
        [(0, 1, 2)],
    )
    mesh.update()

    material = bpy.data.materials.new("smoke_material")
    material.diffuse_color = (0.25, 0.5, 0.75, 1.0)
    mesh.materials.append(material)

    obj = bpy.data.objects.new("smoke_triangle", mesh)
    bpy.context.collection.objects.link(obj)
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    assert bpy.ops.ugx_gltf.initialize_mesh_metadata() == {"FINISHED"}
    assert bpy.ops.ugx_gltf.initialize_material_metadata() == {"FINISHED"}
    mesh["ugx_lod_near_distance"] = 2.5
    mesh["ugx_lod_far_distance"] = 100.0
    mesh["ugx_lod_fade_distance"] = 4.0
    material["ugx_material_version"] = 4
    material["ugx_spec_power"] = 23.0
    obj.location = (2.0, -3.0, 4.0)
    obj.rotation_euler = (0.2, -0.3, 0.7)
    obj.scale = (1.25, 0.5, 2.0)
    bpy.context.view_layer.update()
    bpy.context.scene["ugx_max_instances"] = 3
    return obj


def world_positions(obj: bpy.types.Object) -> list[tuple[float, float, float]]:
    """Return deterministically ordered world-space mesh positions."""
    return sorted(tuple(obj.matrix_world @ vertex.co) for vertex in obj.data.vertices)


def assert_positions_near(
    actual: list[tuple[float, float, float]],
    expected: list[tuple[float, float, float]],
) -> None:
    """Compare packed-geometry positions with a small half-float tolerance."""
    assert len(actual) == len(expected), (actual, expected)
    for actual_position, expected_position in zip(actual, expected, strict=True):
        for actual_component, expected_component in zip(
            actual_position, expected_position, strict=True
        ):
            assert abs(actual_component - expected_component) < 0.02, (
                actual,
                expected,
            )


def verify_exported_metadata(
    gltf_path: Path, *, expect_legacy: bool, expect_lod: bool
) -> None:
    """Assert that edited Blender custom properties survived UGX conversion."""
    root = json.loads(gltf_path.read_text(encoding="utf-8"))
    scene_extras = root["scenes"][root.get("scene", 0)]["extras"]
    assert scene_extras["ugx_max_instances"] == 3, scene_extras

    mesh_extras = root["meshes"][0]["extras"]
    if expect_lod:
        assert mesh_extras["ugx_lod_near_distance"] == 2.5, mesh_extras
        assert mesh_extras["ugx_lod_far_distance"] == 100.0, mesh_extras
        assert mesh_extras["ugx_lod_fade_distance"] == 4.0, mesh_extras

    material_extras = root["materials"][0]["extras"]
    assert material_extras["ugx_material_version"] == 4, material_extras
    if expect_legacy:
        assert material_extras["ugx_spec_power"] == 23.0, material_extras
    else:
        assert "ugx_hogan" in material_extras, material_extras


def cross_convert_real_fixture(
    converter: Path,
    temporary_path: Path,
    source_relative: str,
    target_version: str,
    *,
    verify_checksums: bool,
) -> None:
    """Import a real fixture in Blender and export it for the other game."""
    source = WORKSPACE / source_relative
    assert source.is_file()
    source_summary = bridge.inspect_ugx(
        converter, source, verify_checksums=verify_checksums
    )
    clear_scene()
    result = bpy.ops.import_scene.ugx(
        filepath=str(source),
        include_skeleton=True,
        verify_checksums=verify_checksums,
        pack_images=False,
    )
    assert result == {"FINISHED"}, result
    mesh_objects = [obj for obj in bpy.context.scene.objects if obj.type == "MESH"]
    armatures = [obj for obj in bpy.context.scene.objects if obj.type == "ARMATURE"]
    assert mesh_objects, "real fixture imported no meshes"
    assert len(armatures) == 1, armatures
    assert len(armatures[0].data.bones) == source_summary["bones"]

    destination = temporary_path / f"{source.stem}_{target_version.lower()}.ugx"
    result = bpy.ops.export_scene.ugx(
        filepath=str(destination),
        version=target_version,
        include_skeleton=True,
        selected_only=False,
        apply_modifiers=False,
    )
    assert result == {"FINISHED"}, result
    summary = bridge.inspect_ugx(converter, destination)
    assert summary["version"] == target_version.lower(), summary
    assert summary["sections"] == source_summary["sections"], summary
    assert summary["materials"] == source_summary["materials"], summary
    assert summary["bones"] == source_summary["bones"], summary
    assert summary["vertices"] > 0, summary


def main() -> None:
    """Exercise register, export, Rust conversion, import, and unregister."""
    converter = WORKSPACE / "target" / "debug" / bridge.converter_filename()
    if not converter.is_file():
        raise RuntimeError(f"Build ugx-cli before this test: missing {converter}")
    os.environ[bridge.CONVERTER_ENV] = str(converter)

    ugx_gltf.register()
    try:
        clear_scene()
        source_object = build_triangle()
        expected_world_positions = world_positions(source_object)
        with tempfile.TemporaryDirectory(prefix="ugx_blender_smoke_") as temporary:
            temporary_path = Path(temporary)

            duplicate = source_object.copy()
            duplicate.data = source_object.data.copy()
            duplicate.name = "unselected_triangle"
            bpy.context.collection.objects.link(duplicate)
            duplicate.select_set(False)
            source_object.select_set(True)
            bpy.context.view_layer.objects.active = source_object
            selected_path = temporary_path / "selected.ugx"
            result = bpy.ops.export_scene.ugx(
                filepath=str(selected_path),
                version="HW2",
                include_skeleton=False,
                selected_only=True,
                apply_modifiers=False,
            )
            assert result == {"FINISHED"}, result
            selected_summary = bridge.inspect_ugx(converter, selected_path)
            assert selected_summary["sections"] == 1, selected_summary
            bpy.data.objects.remove(duplicate, do_unlink=True)

            modifier = source_object.modifiers.new("smoke_mirror", "MIRROR")
            modifier.use_mirror_merge = False
            modified_path = temporary_path / "modified.ugx"
            result = bpy.ops.export_scene.ugx(
                filepath=str(modified_path),
                version="HW2",
                include_skeleton=False,
                selected_only=False,
                apply_modifiers=True,
            )
            assert result == {"FINISHED"}, result
            modified_summary = bridge.inspect_ugx(converter, modified_path)
            assert modified_summary["triangles"] == 2, modified_summary
            source_object.modifiers.remove(modifier)

            ugx_path = temporary_path / "smoke.ugx"
            result = bpy.ops.export_scene.ugx(
                filepath=str(ugx_path),
                version="HW2",
                include_skeleton=False,
                selected_only=False,
                apply_modifiers=False,
            )
            assert result == {"FINISHED"}, result
            assert ugx_path.is_file() and ugx_path.stat().st_size > 0

            inspection_path = temporary_path / "inspection.gltf"
            bridge.convert_ugx_to_gltf(
                converter,
                ugx_path,
                inspection_path,
                include_skeleton=False,
            )
            verify_exported_metadata(
                inspection_path, expect_legacy=False, expect_lod=True
            )

            hw1_path = temporary_path / "smoke_hw1.ugx"
            result = bpy.ops.export_scene.ugx(
                filepath=str(hw1_path),
                version="HW1",
                include_skeleton=False,
                selected_only=False,
                apply_modifiers=False,
            )
            assert result == {"FINISHED"}, result
            hw1_inspection = temporary_path / "inspection_hw1.gltf"
            bridge.convert_ugx_to_gltf(
                converter,
                hw1_path,
                hw1_inspection,
                include_skeleton=False,
            )
            verify_exported_metadata(
                hw1_inspection, expect_legacy=True, expect_lod=False
            )

            clear_scene()
            result = bpy.ops.import_scene.ugx(
                filepath=str(ugx_path),
                include_skeleton=False,
                verify_checksums=True,
                pack_images=False,
            )
            assert result == {"FINISHED"}, result
            meshes = [obj for obj in bpy.context.scene.objects if obj.type == "MESH"]
            assert len(meshes) == 1, meshes
            assert_positions_near(world_positions(meshes[0]), expected_world_positions)
            assert bpy.context.scene["ugx_max_instances"] == 3

            cross_convert_real_fixture(
                converter,
                temporary_path,
                "launcher_01.ugx",
                "HW2",
                verify_checksums=False,
            )
            cross_convert_real_fixture(
                converter,
                temporary_path,
                "input/mesh_magnum_01.ugx",
                "HW1",
                verify_checksums=True,
            )
    finally:
        ugx_gltf.unregister()

    print("UGX Blender smoke test passed")


if __name__ == "__main__":
    main()
