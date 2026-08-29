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
from _bpy_restrict_state import RestrictBlend


WORKSPACE = Path(__file__).resolve().parents[2]
BLENDER_DIR = WORKSPACE / "blender"
if str(BLENDER_DIR) not in sys.path:
    sys.path.insert(0, str(BLENDER_DIR))

import ugx_gltf  # noqa: E402
from ugx_gltf import bridge, properties  # noqa: E402


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
    mesh_settings = mesh.ugx_gltf
    mesh_settings.lod_near_distance = 2.5
    mesh_settings.lod_far_distance = 100.0
    mesh_settings.lod_fade_distance = 4.0
    mesh_settings.hw2_has_color = True
    mesh_settings.hw2_color_before_skin = True

    material_settings = material.ugx_gltf
    material_settings.material_version = 4
    material_settings.color_gloss = True
    material_settings.opacity_valid = True
    material_settings.two_sided = True
    material_settings.disable_shadows = True
    material_settings.global_env = True
    material_settings.terrain_conform = True
    material_settings.local_reflection = True
    material_settings.disable_shadow_reception = True
    material_settings.blend_type = "2"
    material_settings.opacity = 0.625
    material_settings.spec_power = 23.0
    material_settings.spec_color = (0.125, 0.25, 0.5)
    material_settings.env_reflectivity = 0.75
    material_settings.env_sharpness = 0.875
    material_settings.env_fresnel = 0.375
    material_settings.env_fresnel_power = 6.5
    material_settings.accessory_index = "4294967295"
    texture = material_settings.maps.add()
    texture.map_type = "diffuse"
    texture.texture_path = r"art\smoke\smoke_diffuse.dds"
    texture.channel = 1
    texture.flags = 0x1234
    diffuse_velocity = next(
        item for item in material_settings.uvw_velocities if item.map_type == "diffuse"
    )
    diffuse_velocity.velocity = (0.25, -0.5, 1.0)
    obj.location = (2.0, -3.0, 4.0)
    obj.rotation_euler = (0.2, -0.3, 0.7)
    obj.scale = (1.25, 0.5, 2.0)
    bpy.context.view_layer.update()
    bpy.context.scene.ugx_gltf.max_instances = 3
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


def verify_stumpy_compatibility_operator(
    converter: Path, temporary_path: Path
) -> None:
    """Exercise legacy-scene detection and its non-destructive binding repair."""
    clear_scene()
    armature_data = bpy.data.armatures.new("stumpy_armature")
    armature = bpy.data.objects.new("stumpy_armature", armature_data)
    bpy.context.collection.objects.link(armature)
    bpy.context.view_layer.objects.active = armature
    armature.select_set(True)
    bpy.ops.object.mode_set(mode="EDIT")
    root = armature.data.edit_bones.new("GrannyRootBone")
    root.head = (0.0, 0.0, 0.0)
    root.tail = (0.0, 1.0, 0.0)
    attach = armature.data.edit_bones.new("AttachBone")
    attach.head = (0.0, 1.0, 0.0)
    attach.tail = (0.0, 2.0, 0.0)
    attach.parent = root
    bpy.ops.object.mode_set(mode="OBJECT")

    mesh = bpy.data.meshes.new("stumpy_mesh")
    mesh.from_pydata(
        [(1.0, 0.0, 0.0), (2.0, 0.0, 0.0), (1.0, 1.0, 0.0)],
        [],
        [(0, 1, 2)],
    )
    material = bpy.data.materials.new("stumpy_material")
    mesh.materials.append(material)
    obj = bpy.data.objects.new("stumpy_mesh", mesh)
    bpy.context.collection.objects.link(obj)
    modifier = obj.modifiers.new("Armature", "ARMATURE")
    modifier.object = armature
    group = obj.vertex_groups.new(name="GrannyRootBone")
    group.add([0, 1, 2], 1.0, "REPLACE")
    obj["MaxHandle"] = 7
    obj["ugxMatIndex"] = 1
    bpy.context.scene["ugxMats"] = [
        {
            "path_df": r"\unsc\smoke\stumpy_df",
            "chan_df": "1",
            "cFlagTwoSided": True,
        }
    ]
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj

    assert properties.is_stumpy_max_scene(bpy.context.scene, [obj, armature])
    assert bpy.ops.ugx_gltf.apply_stumpy_compatibility() == {"FINISHED"}
    settings = mesh.ugx_gltf
    assert bpy.context.scene.ugx_gltf.coordinate_preset == "STUMPY"
    assert settings.section_mode == "SKINNED"
    assert settings.force_bone == "AttachBone"
    assert settings.max_bones == 4
    assert mesh["ugx_section_mode"] == "SKINNED"
    assert mesh["ugx_force_bone"] == "AttachBone"
    assert mesh["ugx_max_bones"] == 4
    material_settings = material.ugx_gltf
    assert material_settings.two_sided
    diffuse = next(item for item in material_settings.maps if item.map_type == "diffuse")
    assert diffuse.texture_path == r"\unsc\smoke\stumpy_df"
    assert diffuse.channel == 1

    armature.select_set(False)
    armature.hide_set(True)
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    destination = temporary_path / "hidden_stumpy_armature.ugx"
    result = bpy.ops.export_scene.ugx(
        filepath=str(destination),
        version="HW1",
        include_skeleton=True,
        selected_only=True,
        apply_modifiers=False,
        coordinate_preset="STUMPY",
    )
    assert result == {"FINISHED"}, result
    assert armature.hide_get(), "export did not restore the hidden armature state"
    summary = bridge.inspect_ugx(converter, destination)
    assert summary["bones"] == 2, summary
    assert summary["vertices"] == 3, summary


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
        assert mesh_extras["ugx_hw2_has_color"] is True, mesh_extras

    material_extras = root["materials"][0]["extras"]
    assert material_extras["ugx_material_version"] == 4, material_extras
    if expect_legacy:
        assert material_extras["ugx_spec_power"] == 23.0, material_extras
        assert material_extras["ugx_flags"] == 0xFB, material_extras
        assert root["materials"][0]["doubleSided"] is True
        assert material_extras["ugx_blend_type"] == 2, material_extras
        visual_alpha = root["materials"][0]["pbrMetallicRoughness"][
            "baseColorFactor"
        ][3]
        assert abs(visual_alpha - 0.625) <= 1.0 / 255.0, root["materials"][0]
        assert material_extras["ugx_spec_color"] == [0.125, 0.25, 0.5]
        assert material_extras["ugx_env_reflectivity"] == 0.75
        assert material_extras["ugx_env_sharpness"] == 0.875
        assert material_extras["ugx_env_fresnel"] == 0.375
        assert material_extras["ugx_env_fresnel_power"] == 6.5
        assert material_extras["ugx_accessory_index"] == 4294967295
        diffuse = material_extras["ugx_maps"]["diffuse"][0]
        assert diffuse == {
            "name": r"art\smoke\smoke_diffuse.dds",
            "channel": 1,
            "flags": 0x1234,
        }, diffuse
        assert material_extras["ugx_uvw_velocity"][0] == [0.25, -0.5, 1.0]
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

    # Blender enables extensions under this guard, where bpy.data is the
    # deliberately limited _RestrictData object.
    with RestrictBlend():
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

            hogan_material = meshes[0].data.materials[0]
            hogan_settings = hogan_material.ugx_gltf
            assert hogan_settings.initialized
            assert hogan_settings.family == "HOGAN"
            assert hogan_settings.permutations
            assert hogan_settings.parameters
            hogan_settings.hogan_shadow_requires_consts = True
            hogan_settings.hogan_terrain_blending = True
            hogan_settings.hogan_textures = r"art\smoke\edited_[al]"
            edited_parameter = next(
                item
                for item in hogan_settings.parameters
                if item.encoding == "NAMED"
            )
            edited_parameter.value = (
                0.75,
                edited_parameter.value[1],
                edited_parameter.value[2],
                edited_parameter.value[3],
            )
            edited_hogan_path = temporary_path / "edited_hogan.ugx"
            result = bpy.ops.export_scene.ugx(
                filepath=str(edited_hogan_path),
                version="HW2",
                include_skeleton=False,
                selected_only=False,
                apply_modifiers=False,
            )
            assert result == {"FINISHED"}, result
            edited_hogan_gltf = temporary_path / "edited_hogan.gltf"
            bridge.convert_ugx_to_gltf(
                converter,
                edited_hogan_path,
                edited_hogan_gltf,
                include_skeleton=False,
            )
            edited_root = json.loads(edited_hogan_gltf.read_text(encoding="utf-8"))
            edited_hogan = edited_root["materials"][0]["extras"]["ugx_hogan"]
            assert edited_hogan["shadow_requires_consts"] is True
            assert edited_hogan["terrain_blending"] is True
            assert edited_hogan["textures"] == r"art\smoke\edited_[al]"
            parameter_map = edited_hogan[
                "vs_cb" if edited_parameter.stage == "VS" else "ps_cb"
            ]
            edited_value = parameter_map[edited_parameter.name]
            first_component = edited_value[0] if isinstance(edited_value, list) else edited_value
            assert abs(first_component - 0.75) < 1e-6, parameter_map

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
            verify_stumpy_compatibility_operator(converter, temporary_path)
    finally:
        ugx_gltf.unregister()

    print("UGX Blender smoke test passed")


if __name__ == "__main__":
    main()
