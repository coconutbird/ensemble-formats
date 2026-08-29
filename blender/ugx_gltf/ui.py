"""Ergonomic Blender panels for UGX workflow and authoring metadata."""

from __future__ import annotations

from pathlib import PurePath

import bpy

from . import metadata, properties


def _active_material(context):
    material = getattr(context, "material", None)
    if material is not None:
        return material
    obj = getattr(context, "object", None)
    return obj.active_material if obj is not None else None


def _active_item(collection, index: int):
    if not collection:
        return None
    return collection[min(index, len(collection) - 1)]


def _texture_label(path: str) -> str:
    normalized = path.replace("\\", "/")
    return PurePath(normalized).name or path or "Unassigned texture"


class UGX_UL_texture_maps(bpy.types.UIList):
    """Compact list of legacy UGX texture bindings."""

    def draw_item(
        self,
        _context,
        layout,
        _data,
        item,
        _icon,
        _active_data,
        _active_property,
        _index,
    ):
        """Show map type, path leaf, UV channel, and flags."""
        if self.layout_type in {"DEFAULT", "COMPACT"}:
            row = layout.row(align=True)
            row.prop(item, "map_type", text="")
            row.label(text=_texture_label(item.texture_path), icon="TEXTURE")
            row.label(text=f"UV {item.channel}")
            row.label(text=f"0x{item.flags:04X}")
        else:
            layout.label(text="", icon="TEXTURE")


class UGX_UL_shader_permutations(bpy.types.UIList):
    """Compact list of Hogan permutation slots."""

    def draw_item(
        self,
        _context,
        layout,
        _data,
        item,
        _icon,
        _active_data,
        _active_property,
        index,
    ):
        """Show quality slot, shader family, and hash."""
        if self.layout_type in {"DEFAULT", "COMPACT"}:
            row = layout.row(align=True)
            row.label(text=f"{index}: {item.name or 'Unnamed'}", icon="SHADING_RENDERED")
            row.label(text=item.hash)
        else:
            layout.label(text="", icon="SHADING_RENDERED")


class UGX_UL_shader_parameters(bpy.types.UIList):
    """Compact list of Hogan constant-buffer values."""

    def draw_item(
        self,
        _context,
        layout,
        _data,
        item,
        _icon,
        _active_data,
        _active_property,
        _index,
    ):
        """Show stage, semantic/register name, and first values."""
        if self.layout_type in {"DEFAULT", "COMPACT"}:
            row = layout.row(align=True)
            row.label(text=item.stage)
            label = item.name if item.encoding == "NAMED" else f"r{item.register_index}"
            row.label(text=label or "Unnamed")
            shown = item.components if item.encoding == "NAMED" else 4
            values = ", ".join(f"{value:.3g}" for value in item.value[:shown])
            row.label(text=values)
        else:
            layout.label(text="", icon="DRIVER")


class UGX_MT_add_texture_map(bpy.types.Menu):
    """Menu containing all thirteen UGX legacy map types."""

    bl_label = "Add UGX Texture Map"
    bl_idname = "UGX_MT_add_texture_map"

    def draw(self, _context):
        """Offer each engine map slot by its descriptive name."""
        for identifier, label, _description in metadata.MAP_TYPES:
            operator = self.layout.operator(
                "ugx_gltf.add_texture_map", text=label, icon="TEXTURE"
            )
            operator.map_type = identifier


class SCENE_PT_ugx_gltf(bpy.types.Panel):
    """Scene-level UGX workflow controls."""

    bl_label = "UGX glTF"
    bl_idname = "SCENE_PT_ugx_gltf"
    bl_space_type = "PROPERTIES"
    bl_region_type = "WINDOW"
    bl_context = "scene"

    def draw(self, context):
        """Draw conversion actions, model metadata, and import/export policy."""
        layout = self.layout
        settings = context.scene.ugx_gltf
        if not settings.initialized:
            properties.initialize_scene_metadata(context.scene)

        row = layout.row(align=True)
        row.scale_y = 1.3
        row.operator("import_scene.ugx", text="Import UGX", icon="IMPORT")
        row.operator("export_scene.ugx", text="Export UGX", icon="EXPORT")

        target = layout.box()
        target.label(text="Export Target", icon="OUTPUT")
        target.prop(settings, "target_version")
        target.prop(settings, "include_skeleton")
        target.prop(settings, "selected_only")
        target.prop(settings, "apply_modifiers")
        target.prop(settings, "coordinate_preset")
        objects = (
            context.selected_objects
            if settings.selected_only
            else context.scene.objects
        )
        scale, mirror_x, transform_label = properties.resolve_authoring_transform(
            context.scene,
            objects,
            settings.coordinate_preset,
            settings.model_scale,
            settings.mirror_x,
        )
        if settings.coordinate_preset == "CUSTOM":
            target.prop(settings, "model_scale")
            target.prop(settings, "mirror_x")
        target.label(
            text=f"Resolved: {transform_label}, scale {scale:g}, mirror X {mirror_x}",
            icon="ORIENTATION_GLOBAL",
        )
        if properties.is_stumpy_max_scene(context.scene, objects):
            target.label(text="Legacy Stumpy / 3ds Max scene detected", icon="INFO")
            target.operator("ugx_gltf.apply_stumpy_compatibility", icon="MODIFIER")

        model = layout.box()
        model.label(text="Model Metadata", icon="SCENE_DATA")
        model.prop(settings, "max_instances")
        model.label(
            text="Packing, bounds, section flags, and bone remaps are rebuilt automatically.",
            icon="INFO",
        )

        importing = layout.box()
        importing.label(text="Import Options", icon="IMPORT")
        importing.prop(settings, "verify_checksums")
        importing.prop(settings, "pack_images")

        source = layout.box()
        source.label(text=f"Source Version: {settings.source_version}")
        if settings.source_path:
            source.prop(settings, "source_path")
        if settings.last_export_path:
            source.prop(settings, "last_export_path")


class DATA_PT_ugx_gltf(bpy.types.Panel):
    """Editable UGX section metadata attached to a mesh datablock."""

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
        """Draw LOD and HW2 vertex-layout controls plus structural status."""
        layout = self.layout
        mesh = context.object.data
        settings = mesh.ugx_gltf
        if not settings.initialized:
            layout.operator("ugx_gltf.initialize_mesh_metadata", icon="ADD")
            layout.label(text="Export will initialize safe defaults automatically.", icon="INFO")
            return

        lod = layout.box()
        lod.label(text="Section LOD (Halo Wars 2)", icon="MOD_DECIM")
        lod.prop(settings, "lod_near_distance")
        lod.prop(settings, "lod_far_distance")
        lod.prop(settings, "lod_fade_distance")

        vertex = layout.box()
        vertex.label(text="HW2 Vertex Layout", icon="VERTEXSEL")
        vertex.prop(settings, "hw2_has_color")
        row = vertex.row()
        row.enabled = settings.hw2_has_color
        row.prop(settings, "hw2_color_before_skin")

        binding = layout.box()
        binding.label(text="Section Binding", icon="ARMATURE_DATA")
        binding.prop(settings, "section_mode")
        armature = properties.armature_for_mesh_object(context.object)
        if settings.section_mode in {"RIGID", "GLOBAL"}:
            if armature is None:
                binding.prop(settings, "binding_bone")
            else:
                binding.prop_search(
                    settings,
                    "binding_bone",
                    armature.data,
                    "bones",
                )
        else:
            if armature is None:
                binding.prop(settings, "force_bone")
            else:
                binding.prop_search(
                    settings,
                    "force_bone",
                    armature.data,
                    "bones",
                )
            binding.prop(settings, "max_bones")
            weighted_bone = properties.exclusively_weighted_bone(context.object)
            if weighted_bone:
                binding.label(text=f"Current weights: 100% {weighted_bone}", icon="INFO")

        structure = layout.box()
        structure.label(text="Preserved / Derived Structure", icon="LOCKED")
        if "ugx_granny_mesh_index" in mesh:
            structure.label(text=f"Granny Mesh Index: {mesh['ugx_granny_mesh_index']}")
        if "ugx_triangle_indices" in mesh:
            structure.label(text="Per-bone triangle bindings preserved", icon="CHECKMARK")
        structure.label(
            text="Rigidity, skinning, section layout, and offsets follow Blender geometry.",
            icon="INFO",
        )


def _draw_legacy_flags(layout, settings) -> None:
    box = layout.box()
    box.label(text="Render Flags", icon="RESTRICT_RENDER_OFF")
    grid = box.grid_flow(row_major=True, columns=2, even_columns=True, align=True)
    for attribute, _label, _bit, _description in metadata.LEGACY_FLAGS:
        grid.prop(settings, attribute)
    box.prop(settings, "raw_flags")


def _draw_texture_maps(layout, settings) -> None:
    box = layout.box()
    box.label(text="Texture Bindings", icon="TEXTURE")
    row = box.row()
    row.template_list(
        "UGX_UL_texture_maps",
        "",
        settings,
        "maps",
        settings,
        "active_map_index",
        rows=4,
    )
    controls = row.column(align=True)
    controls.menu("UGX_MT_add_texture_map", text="", icon="ADD")
    controls.operator("ugx_gltf.remove_texture_map", text="", icon="REMOVE")
    controls.separator()
    up = controls.operator("ugx_gltf.move_texture_map", text="", icon="TRIA_UP")
    up.direction = "UP"
    down = controls.operator("ugx_gltf.move_texture_map", text="", icon="TRIA_DOWN")
    down.direction = "DOWN"

    item = _active_item(settings.maps, settings.active_map_index)
    if item is not None:
        details = box.column(align=True)
        details.prop(item, "map_type")
        details.prop(item, "texture_path")
        row = details.row(align=True)
        row.prop(item, "channel")
        row.prop(item, "flags")
        row = details.row(align=True)
        row.prop(item, "image")
        row.operator("ugx_gltf.use_image_path", text="Copy Path", icon="COPYDOWN")
        velocity = next(
            (entry for entry in settings.uvw_velocities if entry.map_type == item.map_type),
            None,
        )
        if velocity is not None:
            details.prop(velocity, "velocity", text="UVW Velocity")

    row = box.row(align=True)
    row.prop(
        settings,
        "show_uvw_velocities",
        text="All UVW Velocities",
        icon="TRIA_DOWN" if settings.show_uvw_velocities else "TRIA_RIGHT",
        emboss=False,
    )
    if settings.show_uvw_velocities:
        for item in settings.uvw_velocities:
            box.prop(
                item,
                "velocity",
                text=metadata.MAP_TYPE_LABELS.get(item.map_type, item.map_type),
            )


def _draw_legacy(layout, settings) -> None:
    _draw_legacy_flags(layout, settings)

    blend = layout.box()
    blend.label(text="Blending", icon="IMAGE_ALPHA")
    blend.prop(settings, "blend_type")
    opacity = blend.row()
    opacity.enabled = settings.opacity_valid
    opacity.prop(settings, "opacity")

    surface = layout.box()
    surface.label(text="Surface", icon="MATERIAL")
    surface.prop(settings, "spec_power")
    surface.prop(settings, "spec_color")
    surface.prop(settings, "accessory_index")

    environment = layout.box()
    environment.label(text="Environment Reflection", icon="WORLD")
    environment.prop(settings, "env_reflectivity")
    environment.prop(settings, "env_sharpness")
    environment.prop(settings, "env_fresnel")
    environment.prop(settings, "env_fresnel_power")

    _draw_texture_maps(layout, settings)


def _draw_hogan_flags(layout, permutation) -> None:
    flags = layout.box()
    flags.label(text="Permutation Feature Mask", icon="OPTIONS")
    grid = flags.grid_flow(row_major=True, columns=2, even_columns=True, align=True)
    for attribute, _label, _bit, _description in metadata.HOGAN_FLAGS:
        grid.prop(permutation, attribute)
    flags.prop(permutation, "raw_flags")
    flags.label(
        text="A changed feature mask also needs the matching retail permutation hash.",
        icon="ERROR",
    )


def _draw_hogan_permutations(layout, settings) -> None:
    box = layout.box()
    box.label(text="Shader Permutations", icon="SHADING_RENDERED")
    row = box.row()
    row.template_list(
        "UGX_UL_shader_permutations",
        "",
        settings,
        "permutations",
        settings,
        "active_permutation_index",
        rows=3,
    )
    controls = row.column(align=True)
    controls.operator("ugx_gltf.add_shader_permutation", text="", icon="ADD")
    controls.operator("ugx_gltf.remove_shader_permutation", text="", icon="REMOVE")
    permutation = _active_item(settings.permutations, settings.active_permutation_index)
    if permutation is None:
        box.label(text="A loadable Hogan material needs a known permutation.", icon="ERROR")
        return
    box.prop(permutation, "name")
    box.prop(permutation, "hash")
    _draw_hogan_flags(box, permutation)


def _draw_hogan_parameters(layout, settings) -> None:
    box = layout.box()
    box.label(text="Shader Parameters", icon="DRIVER")
    row = box.row()
    row.template_list(
        "UGX_UL_shader_parameters",
        "",
        settings,
        "parameters",
        settings,
        "active_parameter_index",
        rows=5,
    )
    controls = row.column(align=True)
    controls.operator("ugx_gltf.add_shader_parameter", text="", icon="ADD")
    controls.operator("ugx_gltf.remove_shader_parameter", text="", icon="REMOVE")
    parameter = _active_item(settings.parameters, settings.active_parameter_index)
    if parameter is None:
        box.label(text="No material constant overrides", icon="INFO")
        return
    row = box.row(align=True)
    row.prop(parameter, "stage")
    row.prop(parameter, "encoding")
    if parameter.encoding == "NAMED":
        row = box.row(align=True)
        row.prop(parameter, "name")
        row.prop(parameter, "components")
    else:
        box.prop(parameter, "register_index")
    box.prop(parameter, "value")


def _draw_hogan(layout, settings) -> None:
    core = layout.box()
    core.label(text="Hogan Material", icon="MATERIAL")
    row = core.row(align=True)
    row.prop(settings, "hogan_ufx_version")
    row.prop(settings, "hogan_blend_mode")
    core.prop(settings, "hogan_shadow_requires_consts")
    core.prop(settings, "hogan_skinned")
    core.prop(settings, "hogan_terrain_blending")
    core.prop(settings, "hogan_textures")
    core.label(text="Use the game-relative pattern stored by the model.", icon="INFO")
    _draw_hogan_permutations(layout, settings)
    _draw_hogan_parameters(layout, settings)


class MATERIAL_PT_ugx_gltf(bpy.types.Panel):
    """Complete UGX material authoring controls."""

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
        """Draw every semantic legacy or Hogan material field."""
        layout = self.layout
        material = _active_material(context)
        settings = material.ugx_gltf
        if not settings.initialized:
            layout.operator("ugx_gltf.initialize_material_metadata", icon="ADD")
            if "ugx_material_version" in material or "ugx_hogan" in material:
                layout.operator("ugx_gltf.reload_material_metadata", icon="FILE_REFRESH")
            return

        header = layout.box()
        header.prop(settings, "family", expand=True)
        header.prop(settings, "material_version")
        if settings.family == "LEGACY":
            _draw_legacy(layout, settings)
        else:
            _draw_hogan(layout, settings)

        row = layout.row(align=True)
        row.operator("ugx_gltf.apply_material_metadata", icon="CHECKMARK")
        row.operator("ugx_gltf.reload_material_metadata", text="Reload", icon="FILE_REFRESH")
        layout.label(text="Export applies these settings automatically.", icon="INFO")


CLASSES = (
    UGX_UL_texture_maps,
    UGX_UL_shader_permutations,
    UGX_UL_shader_parameters,
    UGX_MT_add_texture_map,
    SCENE_PT_ugx_gltf,
    DATA_PT_ugx_gltf,
    MATERIAL_PT_ugx_gltf,
)


def register():
    """Register UGX metadata panels, lists, and menus."""
    for cls in CLASSES:
        bpy.utils.register_class(cls)


def unregister():
    """Unregister UGX metadata panels, lists, and menus."""
    for cls in reversed(CLASSES):
        bpy.utils.unregister_class(cls)
