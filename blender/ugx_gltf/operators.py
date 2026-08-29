"""Blender operators that connect UGX files to Blender's glTF pipeline."""

from __future__ import annotations

from pathlib import Path
import tempfile

import bpy
from bpy.props import BoolProperty, EnumProperty, StringProperty
from bpy_extras.io_utils import ExportHelper, ImportHelper

from . import bridge, glb
from .properties import VERSION_ITEMS


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


def _scene_max_instances(scene) -> int:
    """Return a checked UGX instance limit from scene custom properties."""
    raw_value = scene.get("ugx_max_instances", 1)
    if isinstance(raw_value, bool):
        raise bridge.BridgeError("UGX maximum instances must be an integer")
    try:
        value = int(raw_value)
    except (TypeError, ValueError, OverflowError) as error:
        raise bridge.BridgeError("UGX maximum instances must be an integer") from error
    if value != raw_value:
        raise bridge.BridgeError("UGX maximum instances must be an integer")
    if not -32_768 <= value <= 32_767:
        raise bridge.BridgeError(
            "UGX maximum instances must fit a signed 16-bit integer"
        )
    return value


def _active_material(context):
    """Resolve the active material in both UI and headless contexts."""
    material = getattr(context, "material", None)
    if material is not None:
        return material
    obj = getattr(context, "object", None)
    return obj.active_material if obj is not None else None


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

    def invoke(self, context, event):
        """Seed file-browser options from the scene settings."""
        settings = context.scene.ugx_gltf
        self.version = settings.target_version
        self.include_skeleton = settings.include_skeleton
        self.selected_only = settings.selected_only
        self.apply_modifiers = settings.apply_modifiers
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
        try:
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
                result = bpy.ops.export_scene.gltf(**options)
                _require_finished(result, "export")
                max_instances = _scene_max_instances(context.scene)
                glb.update_scene_extras(
                    glb_path,
                    {"ugx_max_instances": max_instances},
                )
                bridge.convert_gltf_to_ugx(
                    executable,
                    glb_path,
                    destination,
                    version=self.version,
                    include_skeleton=self.include_skeleton,
                    timeout_seconds=timeout,
                )
        except (bridge.BridgeError, glb.GlbError, OSError) as error:
            self.report({"ERROR"}, _short_error(error))
            return {"CANCELLED"}

        settings = context.scene.ugx_gltf
        settings.last_export_path = str(destination)
        settings.target_version = self.version
        settings.include_skeleton = self.include_skeleton
        settings.selected_only = self.selected_only
        settings.apply_modifiers = self.apply_modifiers
        self.report({"INFO"}, f"Exported {destination}")
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
        """Create the flat custom properties consumed by ugx-gltf."""
        mesh = context.object.data
        defaults = {
            "ugx_lod_near_distance": 0.0,
            "ugx_lod_far_distance": 3.4028234663852886e38,
            "ugx_lod_fade_distance": 0.0,
        }
        for key, value in defaults.items():
            if key not in mesh:
                mesh[key] = value
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
        """Create the material-version property consumed by ugx-gltf."""
        material = _active_material(context)
        if "ugx_material_version" not in material:
            material["ugx_material_version"] = 4
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
    UGX_GLTF_OT_initialize_mesh_metadata,
    UGX_GLTF_OT_initialize_material_metadata,
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
