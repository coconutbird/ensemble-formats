"""Blender properties panels for UGX workflows and metadata."""

from __future__ import annotations

import bpy


def _draw_custom_property(layout, owner, key: str, label: str) -> bool:
    if key not in owner:
        return False
    layout.prop(owner, f'["{key}"]', text=label)
    return True


def _active_material(context):
    material = getattr(context, "material", None)
    if material is not None:
        return material
    obj = getattr(context, "object", None)
    return obj.active_material if obj is not None else None


class SCENE_PT_ugx_gltf(bpy.types.Panel):
    """Scene-level UGX workflow controls."""

    bl_label = "UGX glTF"
    bl_idname = "SCENE_PT_ugx_gltf"
    bl_space_type = "PROPERTIES"
    bl_region_type = "WINDOW"
    bl_context = "scene"

    def draw(self, context):
        """Draw import/export defaults and recorded paths."""
        layout = self.layout
        settings = context.scene.ugx_gltf

        row = layout.row(align=True)
        row.operator("import_scene.ugx", text="Import UGX", icon="IMPORT")
        row.operator("export_scene.ugx", text="Export UGX", icon="EXPORT")

        layout.prop(settings, "target_version")
        layout.prop(settings, "include_skeleton")
        layout.prop(settings, "selected_only")
        layout.prop(settings, "apply_modifiers")

        import_box = layout.box()
        import_box.label(text="Import Options")
        import_box.prop(settings, "verify_checksums")
        import_box.prop(settings, "pack_images")

        metadata = layout.box()
        metadata.label(text="Model Metadata")
        if not _draw_custom_property(
            metadata, context.scene, "ugx_max_instances", "Maximum Instances"
        ):
            metadata.label(text="Maximum Instances: 1 (default)")
        metadata.label(text=f"Source Version: {settings.source_version}")
        if settings.source_path:
            metadata.prop(settings, "source_path")
        if settings.last_export_path:
            metadata.prop(settings, "last_export_path")


class DATA_PT_ugx_gltf(bpy.types.Panel):
    """Editable UGX metadata attached to a mesh datablock."""

    bl_label = "UGX Metadata"
    bl_idname = "DATA_PT_ugx_gltf"
    bl_space_type = "PROPERTIES"
    bl_region_type = "WINDOW"
    bl_context = "data"
    bl_options = {"DEFAULT_CLOSED"}

    @classmethod
    def poll(cls, context):
        """Show only for active mesh data."""
        return context.object is not None and context.object.type == "MESH"

    def draw(self, context):
        """Draw editable section LOD fields and preserved structural metadata."""
        layout = self.layout
        mesh = context.object.data
        found = False
        found |= _draw_custom_property(
            layout, mesh, "ugx_lod_near_distance", "LOD Near Distance"
        )
        found |= _draw_custom_property(
            layout, mesh, "ugx_lod_far_distance", "LOD Far Distance"
        )
        found |= _draw_custom_property(
            layout, mesh, "ugx_lod_fade_distance", "LOD Fade Distance"
        )
        if not found:
            layout.operator("ugx_gltf.initialize_mesh_metadata", icon="ADD")
        if "ugx_granny_mesh_index" in mesh:
            layout.label(text=f"Granny Mesh Index: {mesh['ugx_granny_mesh_index']}")
        if "ugx_triangle_indices" in mesh:
            layout.label(text="Per-bone triangle bindings preserved")


class MATERIAL_PT_ugx_gltf(bpy.types.Panel):
    """Editable UGX material metadata imported from glTF extras."""

    bl_label = "UGX Metadata"
    bl_idname = "MATERIAL_PT_ugx_gltf"
    bl_space_type = "PROPERTIES"
    bl_region_type = "WINDOW"
    bl_context = "material"
    bl_options = {"DEFAULT_CLOSED"}

    @classmethod
    def poll(cls, context):
        """Show only when a material is active."""
        return _active_material(context) is not None

    def draw(self, context):
        """Draw common scalar fields and summarize nested metadata."""
        layout = self.layout
        material = _active_material(context)
        keys = (
            ("ugx_material_version", "Material Version"),
            ("ugx_flags", "Flags"),
            ("ugx_blend_type", "Blend Type"),
            ("ugx_spec_power", "Specular Power"),
            ("ugx_env_reflectivity", "Environment Reflectivity"),
            ("ugx_env_sharpness", "Environment Sharpness"),
            ("ugx_env_fresnel", "Environment Fresnel"),
            ("ugx_env_fresnel_power", "Environment Fresnel Power"),
            ("ugx_accessory_index", "Accessory Index"),
            ("ugx_opacity", "Raw Opacity"),
        )
        found = False
        for key, label in keys:
            found |= _draw_custom_property(layout, material, key, label)
        if not found and "ugx_hogan" not in material:
            layout.operator("ugx_gltf.initialize_material_metadata", icon="ADD")
        if "ugx_hogan" in material:
            layout.label(text="Hogan shader metadata preserved", icon="SHADING_RENDERED")
        if "ugx_maps" in material:
            layout.label(text="UGX texture-map bindings preserved", icon="TEXTURE")
        layout.label(text="Nested values are editable under Custom Properties")


CLASSES = (SCENE_PT_ugx_gltf, DATA_PT_ugx_gltf, MATERIAL_PT_ugx_gltf)


def register():
    """Register UGX metadata panels."""
    for cls in CLASSES:
        bpy.utils.register_class(cls)


def unregister():
    """Unregister UGX metadata panels."""
    for cls in reversed(CLASSES):
        bpy.utils.unregister_class(cls)
