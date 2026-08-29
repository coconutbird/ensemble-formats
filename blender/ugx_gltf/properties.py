"""Persistent Blender scene settings for UGX workflows."""

from __future__ import annotations

import bpy
from bpy.props import BoolProperty, EnumProperty, PointerProperty, StringProperty


VERSION_ITEMS = (
    ("HW2", "Halo Wars 2", "Write UGX v6 geometry and Hogan materials"),
    ("HW1", "Halo Wars DE", "Write UGX v4 geometry and legacy materials"),
)

SOURCE_VERSION_ITEMS = (
    ("UNKNOWN", "Unknown", "No UGX source version has been recorded"),
    *VERSION_ITEMS,
)


class UGXSceneSettings(bpy.types.PropertyGroup):
    """Settings shared by the UGX panels and file operators."""

    source_path: StringProperty(
        name="Source UGX",
        description="Most recently imported UGX file",
        subtype="FILE_PATH",
    )
    last_export_path: StringProperty(
        name="Last Export",
        description="Most recently exported UGX file",
        subtype="FILE_PATH",
    )
    source_version: EnumProperty(
        name="Source Version",
        description="Version detected in the imported UGX file",
        items=SOURCE_VERSION_ITEMS,
        default="UNKNOWN",
    )
    target_version: EnumProperty(
        name="Target Game",
        description="UGX layout and material family to write",
        items=VERSION_ITEMS,
        default="HW2",
    )
    include_skeleton: BoolProperty(
        name="Skeleton",
        description="Import or export armatures, joints, and skin weights",
        default=True,
    )
    verify_checksums: BoolProperty(
        name="Verify Checksums",
        description="Reject UGX files whose ECF checksums are invalid",
        default=True,
    )
    pack_images: BoolProperty(
        name="Pack Images",
        description="Pack texture images resolved by Blender into the blend file",
        default=True,
    )
    selected_only: BoolProperty(
        name="Selected Objects",
        description="Export only selected objects",
        default=False,
    )
    apply_modifiers: BoolProperty(
        name="Apply Modifiers",
        description="Evaluate non-armature modifiers before conversion",
        default=False,
    )


CLASSES = (UGXSceneSettings,)


def register():
    """Register persistent scene properties."""
    for cls in CLASSES:
        bpy.utils.register_class(cls)
    bpy.types.Scene.ugx_gltf = PointerProperty(type=UGXSceneSettings)


def unregister():
    """Unregister persistent scene properties."""
    del bpy.types.Scene.ugx_gltf
    for cls in reversed(CLASSES):
        bpy.utils.unregister_class(cls)
