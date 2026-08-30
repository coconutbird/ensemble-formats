"""Blender operators that connect UGX files to Blender's glTF pipeline."""

from __future__ import annotations

from contextlib import contextmanager
from pathlib import Path
import tempfile

import bpy
from bpy.props import BoolProperty, EnumProperty, FloatProperty, StringProperty
from bpy_extras.io_utils import ExportHelper, ImportHelper

from . import bridge, glb, metadata, properties
from .properties import COORDINATE_PRESET_ITEMS, MAP_TYPE_ITEMS, VERSION_ITEMS


def _preferences(context):
    addon = context.preferences.addons.get(__package__)
    return addon.preferences if addon is not None else None


def _converter(context) -> tuple[Path, int]:
    preferences = _preferences(context)
    configured = preferences.converter_path if preferences is not None else ""
    if configured:
        configured = bpy.path.abspath(configured)
    timeout = preferences.timeout_seconds if preferences is not None else 300
    return bridge.resolve_converter(configured), timeout


def _supported_kwargs(operator, values: dict[str, object]) -> dict[str, object]:
    """Filter glTF options across Blender 4.2+ API revisions."""
    try:
        supported = {prop.identifier for prop in operator.get_rna_type().properties}
    except RuntimeError as error:
        raise bridge.BridgeError(
            "Blender's built-in glTF extension is unavailable"
        ) from error
    return {key: value for key, value in values.items() if key in supported}


def _require_finished(result: set[str], operation: str) -> None:
    if "FINISHED" not in result:
        raise bridge.BridgeError(
            f"Blender glTF {operation} did not finish: {sorted(result)}"
        )


def _short_error(error: Exception) -> str:
    message = str(error).replace("\n", " ").strip()
    return message if len(message) <= 500 else f"{message[:497]}..."


def _active_material(context):
    """Resolve the active material in both UI and headless contexts."""
    material = getattr(context, "material", None)
    if material is not None:
        return material
    obj = getattr(context, "object", None)
    return obj.active_material if obj is not None else None


def _set_forced_binding(obj, target_bone: str) -> None:
    """Store an export-only full-weight override without editing vertex groups."""
    settings = obj.data.ugx_gltf
    if not settings.initialized:
        properties.initialize_mesh_metadata(obj.data)
    settings.section_mode = "SKINNED"
    settings.force_bone = target_bone
    settings.max_bones = 4
    properties.save_mesh_metadata(obj.data)


def _prepare_stumpy_compatibility(
    scene,
    objects,
    *,
    remigrate_materials: bool,
    process_bindings: bool,
):
    """Migrate legacy metadata and apply only evidence-backed binding repairs."""
    stumpy_objects = []
    verified = []
    preserved = []
    for obj in objects:
        if getattr(obj, "type", None) != "MESH" or "MaxHandle" not in obj:
            continue
        stumpy_objects.append(obj)
        if not process_bindings or properties.has_explicit_section_binding(obj):
            continue
        status, target_bone, _detail = properties.diagnose_stumpy_binding(obj)
        if status == "VERIFIED":
            verified.append((obj, target_bone))
        elif status == "PRESERVED":
            preserved.append(obj.name)

    migrated_materials = 0
    for obj in stumpy_objects:
        if remigrate_materials or properties.needs_stumpy_material_migration(scene, obj):
            migrated_materials += properties.apply_stumpy_material(scene, obj)
    repaired = []
    for obj, target_bone in verified:
        _set_forced_binding(obj, target_bone)
        repaired.append(f"{obj.name} -> {target_bone}")
    return repaired, preserved, migrated_materials


@contextmanager
def _expose_export_armatures(context, objects, *, select: bool):
    """Temporarily expose armatures required by exported mesh skins."""
    states = []
    seen = set()
    for obj in objects:
        if getattr(obj, "type", None) != "MESH":
            continue
        armature = properties.armature_for_mesh_object(obj)
        if armature is None or armature.name in seen:
            continue
        seen.add(armature.name)
        if context.view_layer.objects.get(armature.name) is None:
            raise bridge.BridgeError(
                f"Armature '{armature.name}' for mesh '{obj.name}' is excluded "
                "from the active view layer"
            )
        states.append(
            (
                armature,
                armature.hide_get(),
                armature.hide_viewport,
                armature.select_get(),
            )
        )
        armature.hide_viewport = False
        armature.hide_set(False)
        if select:
            armature.select_set(True)
    try:
        yield
    finally:
        for armature, hidden, hidden_in_viewport, selected in reversed(states):
            if select:
                armature.select_set(selected)
            armature.hide_set(hidden)
            armature.hide_viewport = hidden_in_viewport


class UGX_GLTF_OT_validate_converter(bpy.types.Operator):
    """Check that the configured Rust converter can run."""

    bl_idname = "ugx_gltf.validate_converter"
    bl_label = "Validate Rust Converter"
    bl_options = {"INTERNAL"}

    def execute(self, context):
        """Resolve and query the converter version."""
        try:
            executable, timeout = _converter(context)
            version = bridge.converter_version(
                executable, timeout_seconds=min(timeout, 30)
            )
        except bridge.BridgeError as error:
            self.report({"ERROR"}, _short_error(error))
            return {"CANCELLED"}
        self.report({"INFO"}, f"Using {version} at {executable}")
        return {"FINISHED"}


class IMPORT_SCENE_OT_ugx(bpy.types.Operator, ImportHelper):
    """Import a UGX model through the Rust glTF converter."""

    bl_idname = "import_scene.ugx"
    bl_label = "Import UGX"
    bl_options = {"UNDO", "PRESET"}

    filename_ext = ".ugx"
    filter_glob: StringProperty(default="*.ugx", options={"HIDDEN"})
    include_skeleton: BoolProperty(
        name="Skeleton",
        description="Import bones, armature, and skin weights",
        default=True,
    )
    verify_checksums: BoolProperty(
        name="Verify Checksums",
        description="Reject UGX files whose ECF checksums are invalid",
        default=True,
    )
    pack_images: BoolProperty(
        name="Pack Images",
        description="Pack any texture images Blender can resolve",
        default=True,
    )

    def invoke(self, context, event):
        """Seed file-browser options from the scene settings."""
        settings = context.scene.ugx_gltf
        self.include_skeleton = settings.include_skeleton
        self.verify_checksums = settings.verify_checksums
        self.pack_images = settings.pack_images
        return ImportHelper.invoke(self, context, event)

    def execute(self, context):
        """Convert UGX to temporary glTF and import it into Blender."""
        source = Path(bpy.path.abspath(self.filepath)).expanduser()
        if not source.is_file():
            self.report({"ERROR"}, f"UGX file not found: {source}")
            return {"CANCELLED"}

        try:
            executable, timeout = _converter(context)
            summary = bridge.inspect_ugx(
                executable,
                source,
                verify_checksums=self.verify_checksums,
                timeout_seconds=timeout,
            )
            with tempfile.TemporaryDirectory(prefix="ugx_gltf_import_") as temporary:
                gltf_path = Path(temporary) / f"{source.stem}.gltf"
                bridge.convert_ugx_to_gltf(
                    executable,
                    source,
                    gltf_path,
                    include_skeleton=self.include_skeleton,
                    verify_checksums=self.verify_checksums,
                    timeout_seconds=timeout,
                )
                options = _supported_kwargs(
                    bpy.ops.import_scene.gltf,
                    {
                        "filepath": str(gltf_path),
                        "import_pack_images": self.pack_images,
                        "import_select_created_objects": True,
                        "import_scene_extras": True,
                        "import_scene_as_collection": True,
                    },
                )
                result = bpy.ops.import_scene.gltf(**options)
                _require_finished(result, "import")
        except (bridge.BridgeError, glb.GlbError, OSError) as error:
            self.report({"ERROR"}, _short_error(error))
            return {"CANCELLED"}

        settings = context.scene.ugx_gltf
        detected = str(summary.get("version", "")).upper()
        settings.source_path = str(source)
        settings.source_version = detected if detected in {"HW1", "HW2"} else "UNKNOWN"
        if detected in {"HW1", "HW2"}:
            settings.target_version = detected
        settings.include_skeleton = self.include_skeleton
        settings.verify_checksums = self.verify_checksums
        settings.pack_images = self.pack_images
        context.scene["ugx_max_instances"] = int(summary.get("max_instances", 1))
        properties.initialize_scene_metadata(context.scene)
        properties.initialize_objects_metadata(context.selected_objects)

        self.report(
            {"INFO"},
            "Imported "
            f"{summary.get('sections', '?')} sections, "
            f"{summary.get('vertices', '?')} vertices, and "
            f"{summary.get('bones', '?')} bones",
        )
        return {"FINISHED"}


class EXPORT_SCENE_OT_ugx(bpy.types.Operator, ExportHelper):
    """Export the Blender scene to UGX through a temporary GLB."""

    bl_idname = "export_scene.ugx"
    bl_label = "Export UGX"
    bl_options = {"PRESET"}

    filename_ext = ".ugx"
    filter_glob: StringProperty(default="*.ugx", options={"HIDDEN"})
    version: EnumProperty(
        name="Target Game",
        description="UGX layout and material family to write",
        items=VERSION_ITEMS,
        default="HW2",
    )
    include_skeleton: BoolProperty(
        name="Skeleton",
        description="Export armatures, joints, and skin weights",
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
    coordinate_preset: EnumProperty(
        name="Authoring Coordinates",
        description="Coordinate convention used by this Blender scene",
        items=COORDINATE_PRESET_ITEMS,
        default="AUTO",
    )
    model_scale: FloatProperty(
        name="Model Scale",
        description="Uniform model-space scale applied only while writing UGX",
        min=0.000001,
        default=1.0,
    )
    mirror_x: BoolProperty(
        name="Mirror X",
        description="Reflect geometry and skeleton bind matrices across X",
        default=False,
    )

    def invoke(self, context, event):
        """Seed file-browser options from the scene settings."""
        settings = context.scene.ugx_gltf
        self.version = settings.target_version
        self.include_skeleton = settings.include_skeleton
        self.selected_only = settings.selected_only
        self.apply_modifiers = settings.apply_modifiers
        self.coordinate_preset = settings.coordinate_preset
        self.model_scale = settings.model_scale
        self.mirror_x = settings.mirror_x
        return ExportHelper.invoke(self, context, event)

    def execute(self, context):
        """Export Blender to temporary GLB, preserve extras, then write UGX."""
        destination = Path(
            bpy.path.ensure_ext(bpy.path.abspath(self.filepath), self.filename_ext)
        ).expanduser()
        objects = (
            context.selected_objects if self.selected_only else context.scene.objects
        )
        if not any(obj.type == "MESH" for obj in objects):
            self.report({"ERROR"}, "No mesh objects are available for UGX export")
            return {"CANCELLED"}
        model_scale, mirror_x, transform_label = properties.resolve_authoring_transform(
            context.scene,
            objects,
            self.coordinate_preset,
            self.model_scale,
            self.mirror_x,
        )
        repaired = []
        preserved = []
        migrated_materials = 0
        try:
            if transform_label == "Stumpy 3ds Max":
                repaired, preserved, migrated_materials = _prepare_stumpy_compatibility(
                    context.scene,
                    objects,
                    remigrate_materials=False,
                    process_bindings=self.include_skeleton,
                )
            properties.prepare_objects_for_export(context.scene, objects)
            material_extras = properties.material_extras_for_objects(objects)
            executable, timeout = _converter(context)
            with tempfile.TemporaryDirectory(prefix="ugx_gltf_export_") as temporary:
                glb_path = Path(temporary) / f"{destination.stem}.glb"
                options = _supported_kwargs(
                    bpy.ops.export_scene.gltf,
                    {
                        "filepath": str(glb_path),
                        "export_format": "GLB",
                        "use_selection": self.selected_only,
                        "export_selected": self.selected_only,
                        "use_active_scene": True,
                        "export_extras": True,
                        "export_yup": True,
                        "export_apply": self.apply_modifiers,
                        "export_animations": False,
                        "export_current_frame": False,
                        "export_morph": False,
                        "export_morph_normal": False,
                        "export_morph_tangent": False,
                        "export_try_sparse_sk": False,
                        "export_skins": self.include_skeleton,
                        "export_all_influences": False,
                        "export_materials": "EXPORT",
                        "export_texcoords": True,
                        "export_normals": True,
                        "export_tangents": True,
                    },
                )
                with _expose_export_armatures(
                    context,
                    objects if self.include_skeleton else (),
                    select=self.selected_only and self.include_skeleton,
                ):
                    result = bpy.ops.export_scene.gltf(**options)
                _require_finished(result, "export")
                max_instances = context.scene.ugx_gltf.max_instances
                glb.update_ugx_extras(
                    glb_path,
                    {"ugx_max_instances": max_instances},
                    material_extras,
                )
                bridge.convert_gltf_to_ugx(
                    executable,
                    glb_path,
                    destination,
                    version=self.version,
                    include_skeleton=self.include_skeleton,
                    model_scale=model_scale,
                    mirror_x=mirror_x,
                    timeout_seconds=timeout,
                )
        except (
            bridge.BridgeError,
            glb.GlbError,
            metadata.MetadataError,
            OSError,
        ) as error:
            self.report({"ERROR"}, _short_error(error))
            return {"CANCELLED"}

        settings = context.scene.ugx_gltf
        settings.last_export_path = str(destination)
        settings.target_version = self.version
        settings.include_skeleton = self.include_skeleton
        settings.selected_only = self.selected_only
        settings.apply_modifiers = self.apply_modifiers
        settings.coordinate_preset = self.coordinate_preset
        settings.model_scale = self.model_scale
        settings.mirror_x = self.mirror_x
        compatibility = []
        if repaired:
            compatibility.append("automatic binding: " + ", ".join(repaired))
        if preserved:
            compatibility.append(
                f"preserved {len(preserved)} existing root binding(s)"
            )
        if migrated_materials:
            compatibility.append(f"migrated {migrated_materials} material(s)")
        suffix = "; " + "; ".join(compatibility) if compatibility else ""
        self.report(
            {"INFO"},
            f"Exported {destination} ({transform_label} coordinates){suffix}",
        )
        return {"FINISHED"}


class UGX_GLTF_OT_apply_stumpy_compatibility(bpy.types.Operator):
    """Configure legacy coordinates, materials, and verified binding repairs."""

    bl_idname = "ugx_gltf.apply_stumpy_compatibility"
    bl_label = "Apply Stumpy Compatibility"
    bl_options = {"UNDO"}

    def execute(self, context):
        """Apply export-only correction without modifying mesh coordinates or weights."""
        scene_settings = context.scene.ugx_gltf
        scene_settings.coordinate_preset = "STUMPY"
        objects = context.selected_objects or context.scene.objects
        repaired, preserved, migrated_materials = _prepare_stumpy_compatibility(
            context.scene,
            objects,
            remigrate_materials=True,
            process_bindings=True,
        )
        message = (
            "Enabled Stumpy coordinates; "
            f"migrated {migrated_materials} material(s)"
        )
        if repaired:
            message += "; verified binding: " + ", ".join(repaired)
        if preserved:
            message += f"; preserved {len(preserved)} existing root binding(s)"
        self.report({"INFO"}, message)
        return {"FINISHED"}


class UGX_GLTF_OT_initialize_mesh_metadata(bpy.types.Operator):
    """Add editable UGX LOD defaults to the active mesh."""

    bl_idname = "ugx_gltf.initialize_mesh_metadata"
    bl_label = "Add UGX LOD Metadata"
    bl_options = {"UNDO", "INTERNAL"}

    @classmethod
    def poll(cls, context):
        """Enable only when the active object owns mesh data."""
        return context.object is not None and context.object.type == "MESH"

    def execute(self, context):
        """Create editable mesh metadata consumed by ugx-gltf."""
        mesh = context.object.data
        properties.initialize_mesh_metadata(mesh)
        properties.save_mesh_metadata(mesh)
        return {"FINISHED"}


class UGX_GLTF_OT_initialize_material_metadata(bpy.types.Operator):
    """Add baseline UGX metadata to the active material."""

    bl_idname = "ugx_gltf.initialize_material_metadata"
    bl_label = "Add UGX Material Metadata"
    bl_options = {"UNDO", "INTERNAL"}

    @classmethod
    def poll(cls, context):
        """Enable only when an active material exists."""
        return _active_material(context) is not None

    def execute(self, context):
        """Create a safe legacy authoring material consumed by ugx-gltf."""
        material = _active_material(context)
        properties.initialize_material_metadata(material, family="LEGACY")
        material.ugx_gltf.two_sided = False
        properties.save_material_metadata(material)
        return {"FINISHED"}


class UGX_GLTF_OT_apply_material_metadata(bpy.types.Operator):
    """Synchronize the active material editor with its glTF extras."""

    bl_idname = "ugx_gltf.apply_material_metadata"
    bl_label = "Apply UGX Metadata"
    bl_options = {"UNDO", "INTERNAL"}

    @classmethod
    def poll(cls, context):
        """Enable only when an active material exists."""
        return _active_material(context) is not None

    def execute(self, context):
        """Validate and serialize all editable material fields."""
        try:
            properties.save_material_metadata(_active_material(context))
        except metadata.MetadataError as error:
            self.report({"ERROR"}, _short_error(error))
            return {"CANCELLED"}
        self.report({"INFO"}, "UGX material metadata applied")
        return {"FINISHED"}


class UGX_GLTF_OT_reload_material_metadata(bpy.types.Operator):
    """Reload the active editor from the material's glTF extras."""

    bl_idname = "ugx_gltf.reload_material_metadata"
    bl_label = "Reload Preserved Metadata"
    bl_options = {"UNDO", "INTERNAL"}

    @classmethod
    def poll(cls, context):
        """Enable only when an active material exists."""
        return _active_material(context) is not None

    def execute(self, context):
        """Discard unapplied editor values and hydrate from custom properties."""
        properties.initialize_material_metadata(_active_material(context))
        return {"FINISHED"}


class UGX_GLTF_OT_add_texture_map(bpy.types.Operator):
    """Add a texture binding to the active UGX material."""

    bl_idname = "ugx_gltf.add_texture_map"
    bl_label = "Add Texture Map"
    bl_options = {"UNDO", "INTERNAL"}

    map_type: EnumProperty(name="Map Type", items=MAP_TYPE_ITEMS, default="diffuse")

    @classmethod
    def poll(cls, context):
        """Enable only when an active material exists."""
        return _active_material(context) is not None

    def execute(self, context):
        """Append a map entry and make it active."""
        material = _active_material(context)
        settings = material.ugx_gltf
        if not settings.initialized:
            properties.initialize_material_metadata(material)
        item = settings.maps.add()
        item.map_type = self.map_type
        item.channel = 0
        item.flags = 7
        settings.active_map_index = len(settings.maps) - 1
        return {"FINISHED"}


class UGX_GLTF_OT_remove_texture_map(bpy.types.Operator):
    """Remove the selected UGX texture binding."""

    bl_idname = "ugx_gltf.remove_texture_map"
    bl_label = "Remove Texture Map"
    bl_options = {"UNDO", "INTERNAL"}

    @classmethod
    def poll(cls, context):
        """Require a selected map on the active material."""
        material = _active_material(context)
        return material is not None and bool(material.ugx_gltf.maps)

    def execute(self, context):
        """Remove the active map without disturbing other slots."""
        settings = _active_material(context).ugx_gltf
        index = min(settings.active_map_index, len(settings.maps) - 1)
        settings.maps.remove(index)
        settings.active_map_index = max(0, min(index, len(settings.maps) - 1))
        return {"FINISHED"}


class UGX_GLTF_OT_move_texture_map(bpy.types.Operator):
    """Move the selected map within its serialized order."""

    bl_idname = "ugx_gltf.move_texture_map"
    bl_label = "Move Texture Map"
    bl_options = {"UNDO", "INTERNAL"}

    direction: EnumProperty(
        name="Direction",
        items=(("UP", "Up", "Move earlier"), ("DOWN", "Down", "Move later")),
    )

    @classmethod
    def poll(cls, context):
        """Require at least two map entries."""
        material = _active_material(context)
        return material is not None and len(material.ugx_gltf.maps) > 1

    def execute(self, context):
        """Move the active map one row."""
        settings = _active_material(context).ugx_gltf
        source = min(settings.active_map_index, len(settings.maps) - 1)
        destination = source + (-1 if self.direction == "UP" else 1)
        if 0 <= destination < len(settings.maps):
            settings.maps.move(source, destination)
            settings.active_map_index = destination
        return {"FINISHED"}


class UGX_GLTF_OT_use_image_path(bpy.types.Operator):
    """Copy the selected Blender image path into the active UGX map."""

    bl_idname = "ugx_gltf.use_image_path"
    bl_label = "Use Blender Image Path"
    bl_options = {"UNDO", "INTERNAL"}

    @classmethod
    def poll(cls, context):
        """Require an active map with an assigned image."""
        material = _active_material(context)
        if material is None or not material.ugx_gltf.maps:
            return False
        settings = material.ugx_gltf
        index = min(settings.active_map_index, len(settings.maps) - 1)
        return settings.maps[index].image is not None

    def execute(self, context):
        """Copy the non-expanded Blender path, falling back to the image name."""
        settings = _active_material(context).ugx_gltf
        item = settings.maps[min(settings.active_map_index, len(settings.maps) - 1)]
        item.texture_path = item.image.filepath or item.image.name
        return {"FINISHED"}


class UGX_GLTF_OT_add_shader_permutation(bpy.types.Operator):
    """Add or duplicate a Hogan shader permutation."""

    bl_idname = "ugx_gltf.add_shader_permutation"
    bl_label = "Add Shader Permutation"
    bl_options = {"UNDO", "INTERNAL"}

    @classmethod
    def poll(cls, context):
        """Hogan materials support at most four permutation slots."""
        material = _active_material(context)
        return material is not None and len(material.ugx_gltf.permutations) < 4

    def execute(self, context):
        """Duplicate the active entry or create a blank Hogan standard entry."""
        settings = _active_material(context).ugx_gltf
        source = None
        if settings.permutations:
            source = settings.permutations[
                min(settings.active_permutation_index, len(settings.permutations) - 1)
            ]
        item = settings.permutations.add()
        if source is None:
            item.name = "HOGAN_STANDARD_0000000000000000"
        else:
            item.name = source.name
            item.hash = source.hash
            item.raw_flags = source.raw_flags
            for attribute, _label, _bit, _description in metadata.HOGAN_FLAGS:
                setattr(item, attribute, getattr(source, attribute))
        settings.active_permutation_index = len(settings.permutations) - 1
        return {"FINISHED"}


class UGX_GLTF_OT_remove_shader_permutation(bpy.types.Operator):
    """Remove the active Hogan shader permutation."""

    bl_idname = "ugx_gltf.remove_shader_permutation"
    bl_label = "Remove Shader Permutation"
    bl_options = {"UNDO", "INTERNAL"}

    @classmethod
    def poll(cls, context):
        """Require a selected permutation."""
        material = _active_material(context)
        return material is not None and bool(material.ugx_gltf.permutations)

    def execute(self, context):
        """Remove the selected permutation."""
        settings = _active_material(context).ugx_gltf
        index = min(settings.active_permutation_index, len(settings.permutations) - 1)
        settings.permutations.remove(index)
        settings.active_permutation_index = max(
            0, min(index, len(settings.permutations) - 1)
        )
        return {"FINISHED"}


class UGX_GLTF_OT_add_shader_parameter(bpy.types.Operator):
    """Add an editable Hogan constant-buffer value."""

    bl_idname = "ugx_gltf.add_shader_parameter"
    bl_label = "Add Shader Parameter"
    bl_options = {"UNDO", "INTERNAL"}

    @classmethod
    def poll(cls, context):
        """Enable only when an active material exists."""
        return _active_material(context) is not None

    def execute(self, context):
        """Append a named pixel-shader parameter."""
        settings = _active_material(context).ugx_gltf
        item = settings.parameters.add()
        item.stage = "PS"
        item.encoding = "NAMED"
        item.name = "parameter"
        item.components = 1
        settings.active_parameter_index = len(settings.parameters) - 1
        return {"FINISHED"}


class UGX_GLTF_OT_remove_shader_parameter(bpy.types.Operator):
    """Remove the active Hogan constant-buffer value."""

    bl_idname = "ugx_gltf.remove_shader_parameter"
    bl_label = "Remove Shader Parameter"
    bl_options = {"UNDO", "INTERNAL"}

    @classmethod
    def poll(cls, context):
        """Require a selected parameter."""
        material = _active_material(context)
        return material is not None and bool(material.ugx_gltf.parameters)

    def execute(self, context):
        """Remove the selected parameter."""
        settings = _active_material(context).ugx_gltf
        index = min(settings.active_parameter_index, len(settings.parameters) - 1)
        settings.parameters.remove(index)
        settings.active_parameter_index = max(
            0, min(index, len(settings.parameters) - 1)
        )
        return {"FINISHED"}


class UGX_FH_import(bpy.types.FileHandler):
    """Enable dragging a UGX file into the 3D viewport."""

    bl_idname = "UGX_FH_import"
    bl_label = "UGX model"
    bl_import_operator = IMPORT_SCENE_OT_ugx.bl_idname
    bl_file_extensions = ".ugx"

    @classmethod
    def poll_drop(cls, context):
        """Accept drops into a 3D view."""
        return context.area is not None and context.area.type == "VIEW_3D"


CLASSES = (
    UGX_GLTF_OT_validate_converter,
    IMPORT_SCENE_OT_ugx,
    EXPORT_SCENE_OT_ugx,
    UGX_GLTF_OT_apply_stumpy_compatibility,
    UGX_GLTF_OT_initialize_mesh_metadata,
    UGX_GLTF_OT_initialize_material_metadata,
    UGX_GLTF_OT_apply_material_metadata,
    UGX_GLTF_OT_reload_material_metadata,
    UGX_GLTF_OT_add_texture_map,
    UGX_GLTF_OT_remove_texture_map,
    UGX_GLTF_OT_move_texture_map,
    UGX_GLTF_OT_use_image_path,
    UGX_GLTF_OT_add_shader_permutation,
    UGX_GLTF_OT_remove_shader_permutation,
    UGX_GLTF_OT_add_shader_parameter,
    UGX_GLTF_OT_remove_shader_parameter,
    UGX_FH_import,
)


def register():
    """Register UGX file and metadata operators."""
    for cls in CLASSES:
        bpy.utils.register_class(cls)


def unregister():
    """Unregister UGX file and metadata operators."""
    for cls in reversed(CLASSES):
        bpy.utils.unregister_class(cls)
