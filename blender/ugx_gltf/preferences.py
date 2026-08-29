"""Extension preferences for locating the Rust converter."""

from __future__ import annotations

import bpy
from bpy.props import IntProperty, StringProperty

from . import bridge


class UGXGLTFPreferences(bpy.types.AddonPreferences):
    """User-specific converter configuration."""

    bl_idname = __package__

    converter_path: StringProperty(
        name="Rust Converter",
        description="Path to the ugx executable; leave empty for bundled, UGX_CLI, or PATH lookup",
        subtype="FILE_PATH",
    )
    timeout_seconds: IntProperty(
        name="Conversion Timeout",
        description="Maximum time allowed for one Rust conversion",
        default=300,
        min=10,
        max=3600,
        subtype="TIME",
    )

    def draw(self, _context):
        """Draw converter preferences and discovery status."""
        layout = self.layout
        layout.prop(self, "converter_path")
        layout.prop(self, "timeout_seconds")
        row = layout.row()
        try:
            configured = bpy.path.abspath(self.converter_path) if self.converter_path else ""
            executable = bridge.resolve_converter(configured)
        except bridge.ConverterNotFoundError:
            row.label(text="Rust converter not found", icon="ERROR")
        else:
            row.label(text=str(executable), icon="CHECKMARK")
        layout.operator("ugx_gltf.validate_converter", icon="FILE_REFRESH")


CLASSES = (UGXGLTFPreferences,)


def register():
    """Register extension preferences."""
    for cls in CLASSES:
        bpy.utils.register_class(cls)


def unregister():
    """Unregister extension preferences."""
    for cls in reversed(CLASSES):
        bpy.utils.unregister_class(cls)
