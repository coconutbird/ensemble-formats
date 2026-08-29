"""Persistent Blender properties and UGX extras synchronization."""

from __future__ import annotations

import bpy
from bpy.props import (
    BoolProperty,
    CollectionProperty,
    EnumProperty,
    FloatProperty,
    FloatVectorProperty,
    IntProperty,
    PointerProperty,
    StringProperty,
)

from . import metadata


VERSION_ITEMS = (
    ("HW2", "Halo Wars 2", "Write UGX v6 geometry and Hogan materials"),
    ("HW1", "Halo Wars DE", "Write UGX v4 geometry and legacy materials"),
)

STUMPY_MODEL_SCALE = 1.575

COORDINATE_PRESET_ITEMS = (
    (
        "AUTO",
        "Automatic",
        "Use legacy Stumpy correction only for scenes carrying both Stumpy and 3ds Max markers",
    ),
    ("STANDARD", "Blender / UGX", "Export Blender model space without correction"),
    (
        "STUMPY",
        "Stumpy 3ds Max",
        "Mirror X and scale by 1.575 to undo the legacy Max-to-Blender pipeline",
    ),
    ("CUSTOM", "Custom", "Use the scale and X-mirror controls below"),
)

SECTION_MODE_ITEMS = (
    ("AUTO", "Automatic", "Derive section skinning from Blender hierarchy and weights"),
    ("SKINNED", "Skinned", "Keep four blend-index and weight slots in each vertex"),
    ("RIGID", "Rigid", "Bind the whole section rigidly to one bone"),
    ("GLOBAL", "Global Bones", "Use the Halo Wars 1 GlobalBones rigid convention"),
)

SOURCE_VERSION_ITEMS = (
    ("UNKNOWN", "Unknown", "No UGX source version has been recorded"),
    *VERSION_ITEMS,
)

MAP_TYPE_ITEMS = tuple(
    (identifier, label, description, index)
    for index, (identifier, label, description) in enumerate(metadata.MAP_TYPES)
)

FAMILY_ITEMS = (
    ("LEGACY", "Legacy / HW1", "Map-based material used by UGX v4"),
    ("HOGAN", "Hogan / HW2", "Shader-permutation material used by UGX v6"),
)

PARAMETER_ENCODING_ITEMS = (
    ("NAMED", "Named", "Semantic constant-buffer parameter"),
    ("REGISTER", "Register", "Raw float4 constant-buffer register"),
)


def _delete_custom_property(owner, key: str) -> None:
    if key in owner:
        del owner[key]


def _composed_legacy_flags(settings) -> int:
    raw = metadata.parse_uint(settings.raw_flags, 32, "Legacy material flags")
    flags = raw & ~metadata.LEGACY_KNOWN_MASK
    for attribute, _label, bit, _description in metadata.LEGACY_FLAGS:
        if bool(getattr(settings, attribute)):
            flags |= 1 << bit
    return flags


def _composed_hogan_flags(permutation) -> int:
    raw = metadata.parse_uint(permutation.raw_flags, 64, "Hogan shader flags")
    flags = raw & ~metadata.HOGAN_KNOWN_MASK
    for attribute, _label, bit, _description in metadata.HOGAN_FLAGS:
        if bool(getattr(permutation, attribute)):
            flags |= 1 << bit
    return flags


def _set_named_flags(owner, definitions, flags: int) -> None:
    for attribute, _label, bit, _description in definitions:
        setattr(owner, attribute, bool(flags & (1 << bit)))


def _clamped_index(collection, index: int) -> int:
    if not collection:
        return 0
    return max(0, min(index, len(collection) - 1))


def _material_family_updated(settings, _context) -> None:
    """Choose the conventional BDT revision when the artist changes family."""
    settings.material_version = 5 if settings.family == "HOGAN" else 4


def _scene_metadata_updated(settings, _context) -> None:
    """Record that a scene value was deliberately set after registration."""
    settings.initialized = True


class UGXTextureMapSettings(bpy.types.PropertyGroup):
    """One legacy UGX texture-map binding."""

    map_type: EnumProperty(
        name="Map Type",
        description="UGX texture-map slot",
        items=MAP_TYPE_ITEMS,
        default="diffuse",
    )
    texture_path: StringProperty(
        name="Game Texture Path",
        description="Texture name or game-relative path written into UGX",
    )
    channel: IntProperty(
        name="UV Channel",
        description="Zero-based texture-coordinate channel",
        min=0,
        max=32_767,
        default=0,
    )
    flags: IntProperty(
        name="Map Flags",
        description="Raw 16-bit engine flags for this map binding",
        min=0,
        max=65_535,
        default=0,
    )
    image: PointerProperty(
        name="Blender Image",
        description="Optional image whose file path can be copied into the UGX binding",
        type=bpy.types.Image,
    )


class UGXUVWVelocitySettings(bpy.types.PropertyGroup):
    """Animated UV velocity for one legacy map type."""

    map_type: EnumProperty(name="Map Type", items=MAP_TYPE_ITEMS, default="diffuse")
    velocity: FloatVectorProperty(
        name="UVW Velocity",
        description="Per-second U, V, and W texture-coordinate velocity",
        size=3,
        default=(0.0, 0.0, 0.0),
    )


class UGXShaderPermutationSettings(bpy.types.PropertyGroup):
    """One Hogan shader name/hash pair and its embedded feature flags."""

    name: StringProperty(
        name="Shader Permutation",
        description="UFX permutation name, including the 64-bit flag suffix",
    )
    hash: StringProperty(
        name="Permutation Hash",
        description="Unsigned 32-bit shader lookup hash (decimal or 0x hexadecimal)",
        default="0x00000000",
    )
    raw_flags: StringProperty(
        name="Raw Feature Mask",
        description="Full 64-bit mask; named toggles preserve unknown bits",
        default="0x0000000000000000",
    )
    height_blend: BoolProperty(name="Height Blend")
    extra_texture_layer: BoolProperty(name="Extra Texture Layer")
    roughness_channel: BoolProperty(name="Roughness Channel")
    emissive_sub_a: BoolProperty(name="Emissive Variant A")
    emissive_sub_b: BoolProperty(name="Emissive Variant B")
    emissive: BoolProperty(name="Emissive")
    scroll_anim: BoolProperty(name="Scroll Animation")
    per_channel_uv: BoolProperty(name="Per-channel UV")
    vertex_anim_a: BoolProperty(name="Vertex Animation A")
    vertex_anim_b: BoolProperty(name="Vertex Animation B")
    simplified_texturing: BoolProperty(name="Simplified Texturing")
    material_override: BoolProperty(name="Material Override")
    reduced_texturing: BoolProperty(name="Reduced Texturing")


class UGXShaderParameterSettings(bpy.types.PropertyGroup):
    """A named value or raw float4 Hogan constant-buffer register."""

    stage: EnumProperty(
        name="Stage",
        description="Shader stage whose constant buffer owns this value",
        items=metadata.PARAMETER_STAGES,
        default="PS",
    )
    encoding: EnumProperty(
        name="Encoding",
        description="Named semantic value or positional float4 register",
        items=PARAMETER_ENCODING_ITEMS,
        default="NAMED",
    )
    name: StringProperty(name="Name", description="Constant-buffer parameter name")
    register_index: IntProperty(
        name="Register",
        description="Zero-based raw float4 register index",
        min=0,
        default=0,
    )
    components: IntProperty(
        name="Components",
        description="Number of meaningful components for a named value",
        min=1,
        max=4,
        default=4,
    )
    value: FloatVectorProperty(
        name="Value",
        description="Constant-buffer value (unused components are preserved as zero)",
        size=4,
        default=(0.0, 0.0, 0.0, 0.0),
    )


class UGXMeshSettings(bpy.types.PropertyGroup):
    """Editable section metadata attached to a Blender mesh."""

    initialized: BoolProperty(default=False, options={"HIDDEN"})
    lod_near_distance: FloatProperty(
        name="Near Distance",
        description="Distance at which this LOD starts transitioning in",
        default=0.0,
    )
    lod_far_distance: FloatProperty(
        name="Far Distance",
        description="Distance at which this LOD finishes transitioning out",
        default=metadata.FLOAT32_MAX,
    )
    lod_fade_distance: FloatProperty(
        name="Vertical Fade",
        description="HW2 vertical or atmospheric fade distance",
        default=0.0,
    )
    hw2_has_color: BoolProperty(
        name="Preserve Vertex Color Input",
        description="Keep an HW2 COLOR input even if every encoded value is zero",
        default=False,
    )
    hw2_color_before_skin: BoolProperty(
        name="Color Before Skin Data",
        description="Use the UFX vertex layout that stores COLOR before skin indices and weights",
        default=False,
    )
    section_mode: EnumProperty(
        name="Binding Mode",
        description="How the game binds this UGX section to its skeleton",
        items=SECTION_MODE_ITEMS,
        default="AUTO",
    )
    binding_bone: StringProperty(
        name="Rigid Bone",
        description="Bone used by Rigid or Global Bones section binding",
    )
    force_bone: StringProperty(
        name="Override All Weights",
        description="Optional bone that receives weight 1.0 for every exported vertex",
    )
    max_bones: IntProperty(
        name="Maximum Influences",
        description="Section MaxBones value; zero derives it from vertex weights",
        min=0,
        max=4,
        default=0,
    )


class UGXMaterialSettings(bpy.types.PropertyGroup):
    """Editable legacy and Hogan metadata attached to a Blender material."""

    initialized: BoolProperty(default=False, options={"HIDDEN"})
    family: EnumProperty(
        name="Material Family",
        items=FAMILY_ITEMS,
        default="LEGACY",
        update=_material_family_updated,
    )
    material_version: IntProperty(
        name="Material Version",
        description="BDT material @Ver value (normally 4 for legacy and 5 for Hogan)",
        min=0,
        default=4,
    )

    raw_flags: StringProperty(
        name="Raw Flags",
        description="Full legacy 32-bit mask; named toggles preserve unknown bits",
        default="0x00000000",
    )
    color_gloss: BoolProperty(name="Color Gloss")
    opacity_valid: BoolProperty(name="Use Opacity")
    two_sided: BoolProperty(name="Two Sided")
    disable_shadows: BoolProperty(name="Disable Shadow Casting")
    global_env: BoolProperty(name="Global Environment")
    terrain_conform: BoolProperty(name="Terrain Conform")
    local_reflection: BoolProperty(name="Local Reflection")
    disable_shadow_reception: BoolProperty(name="Disable Shadow Reception")
    blend_type: EnumProperty(
        name="Blend Mode",
        description="Legacy UGX render blending mode",
        items=metadata.BLEND_TYPES,
        default="0",
    )
    opacity: FloatProperty(
        name="Opacity",
        description="Used by the engine when Use Opacity is enabled",
        default=1.0,
        soft_min=0.0,
        soft_max=1.0,
    )
    spec_power: FloatProperty(name="Specular Power", default=10.0)
    spec_color: FloatVectorProperty(
        name="Specular Color",
        size=3,
        subtype="COLOR",
        default=(1.0, 1.0, 1.0),
    )
    env_reflectivity: FloatProperty(name="Reflectivity", default=1.0)
    env_sharpness: FloatProperty(name="Sharpness", default=1.0)
    env_fresnel: FloatProperty(name="Fresnel", default=0.5)
    env_fresnel_power: FloatProperty(name="Fresnel Power", default=4.0)
    accessory_index: StringProperty(
        name="Accessory Index",
        description="Unsigned 32-bit legacy material accessory index",
        default="0",
    )
    maps: CollectionProperty(type=UGXTextureMapSettings)
    active_map_index: IntProperty(default=0, min=0)
    uvw_velocities: CollectionProperty(type=UGXUVWVelocitySettings)
    show_uvw_velocities: BoolProperty(name="UVW Animation", default=False)

    hogan_ufx_version: IntProperty(name="UFX Version", min=0, default=9)
    hogan_blend_mode: IntProperty(
        name="Blend Mode",
        description="Raw Hogan blend-mode value",
        min=0,
        default=0,
    )
    hogan_shadow_requires_consts: BoolProperty(
        name="Shadow Requires Constants",
        description="Upload material constants for the shadow pass",
    )
    hogan_skinned: BoolProperty(
        name="Skinned Shader",
        description="Select the skinned-mesh shader path",
    )
    hogan_terrain_blending: BoolProperty(
        name="Terrain Blending",
        description="Enable the Hogan terrain-blending path",
    )
    hogan_textures: StringProperty(
        name="Texture Pattern",
        description="Hogan texture path pattern, commonly ending in model_[al]",
    )
    permutations: CollectionProperty(type=UGXShaderPermutationSettings)
    active_permutation_index: IntProperty(default=0, min=0)
    parameters: CollectionProperty(type=UGXShaderParameterSettings)
    active_parameter_index: IntProperty(default=0, min=0)


class UGXSceneSettings(bpy.types.PropertyGroup):
    """Settings shared by the UGX panels and file operators."""

    initialized: BoolProperty(default=False, options={"HIDDEN"})
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
    max_instances: IntProperty(
        name="Maximum Instances",
        description="Number of instanced index-buffer copies baked for the game",
        min=1,
        max=32_767,
        default=1,
        update=_scene_metadata_updated,
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
        description="Reflect geometry, winding, normals, and skeleton bind matrices across X",
        default=False,
    )


def initialize_scene_metadata(scene) -> None:
    """Load scene extras into its ergonomic settings."""
    settings = scene.ugx_gltf
    settings.max_instances = max(
        1,
        min(32_767, metadata.integer(scene.get("ugx_max_instances"), 1)),
    )
    settings.initialized = True


def save_scene_metadata(scene) -> None:
    """Write ergonomic scene settings to glTF-compatible custom properties."""
    settings = scene.ugx_gltf
    scene["ugx_max_instances"] = int(settings.max_instances)
    settings.initialized = True


def initialize_mesh_metadata(mesh) -> None:
    """Load flat mesh extras into ergonomic settings."""
    settings = mesh.ugx_gltf
    settings.lod_near_distance = metadata.finite_float(
        mesh.get("ugx_lod_near_distance"), 0.0
    )
    settings.lod_far_distance = metadata.finite_float(
        mesh.get("ugx_lod_far_distance"), metadata.FLOAT32_MAX
    )
    settings.lod_fade_distance = metadata.finite_float(
        mesh.get("ugx_lod_fade_distance"), 0.0
    )
    settings.hw2_has_color = bool(mesh.get("ugx_hw2_has_color", False))
    settings.hw2_color_before_skin = bool(
        mesh.get("ugx_hw2_color_before_skin", False)
    )
    section_mode = str(mesh.get("ugx_section_mode", "AUTO")).upper()
    settings.section_mode = (
        section_mode
        if section_mode in {"AUTO", "SKINNED", "RIGID", "GLOBAL"}
        else "AUTO"
    )
    settings.binding_bone = str(mesh.get("ugx_binding_bone", ""))
    settings.force_bone = str(mesh.get("ugx_force_bone", ""))
    settings.max_bones = max(
        0,
        min(4, metadata.integer(mesh.get("ugx_max_bones"), 0)),
    )
    settings.initialized = True


def save_mesh_metadata(mesh) -> None:
    """Write ergonomic mesh settings to glTF-compatible custom properties."""
    settings = mesh.ugx_gltf
    mesh["ugx_lod_near_distance"] = float(settings.lod_near_distance)
    mesh["ugx_lod_far_distance"] = float(settings.lod_far_distance)
    mesh["ugx_lod_fade_distance"] = float(settings.lod_fade_distance)
    mesh["ugx_hw2_has_color"] = bool(settings.hw2_has_color)
    mesh["ugx_hw2_color_before_skin"] = bool(settings.hw2_color_before_skin)
    if settings.section_mode == "AUTO":
        _delete_custom_property(mesh, "ugx_section_mode")
    else:
        mesh["ugx_section_mode"] = settings.section_mode
    if settings.binding_bone:
        mesh["ugx_binding_bone"] = settings.binding_bone
    else:
        _delete_custom_property(mesh, "ugx_binding_bone")
    if settings.force_bone:
        mesh["ugx_force_bone"] = settings.force_bone
    else:
        _delete_custom_property(mesh, "ugx_force_bone")
    if settings.max_bones:
        mesh["ugx_max_bones"] = int(settings.max_bones)
    else:
        _delete_custom_property(mesh, "ugx_max_bones")
    settings.initialized = True


def _initialize_uvw(settings, imported) -> None:
    settings.uvw_velocities.clear()
    for (map_type, _label, _description), velocity in zip(
        metadata.MAP_TYPES, metadata.normalize_uvw(imported), strict=True
    ):
        item = settings.uvw_velocities.add()
        item.map_type = map_type
        item.velocity = velocity


def _initialize_maps(settings, imported) -> None:
    settings.maps.clear()
    for map_type, entries in metadata.normalize_maps(imported).items():
        for entry in entries:
            item = settings.maps.add()
            item.map_type = map_type
            item.texture_path = entry["name"]
            item.channel = entry["channel"]
            item.flags = entry["flags"]
    settings.active_map_index = _clamped_index(settings.maps, 0)


def _initialize_permutations(settings, imported: dict[str, object]) -> None:
    settings.permutations.clear()
    shader_flags = str(imported.get("shader_flags", ""))
    for index, entry in enumerate(imported["shader_permutations"]):
        item = settings.permutations.add()
        item.name = entry["name"]
        item.hash = metadata.format_hex(entry["hash"], 32)
        flags = metadata.shader_flags_from_name(item.name)
        if flags is None and index == 0 and shader_flags:
            try:
                flags = metadata.parse_uint(f"0x{shader_flags}", 64, "Shader flags")
            except metadata.MetadataError:
                flags = 0
        flags = flags or 0
        item.raw_flags = metadata.format_hex(flags, 64)
        _set_named_flags(item, metadata.HOGAN_FLAGS, flags)
    settings.active_permutation_index = _clamped_index(settings.permutations, 0)


def _parameter_value(raw) -> tuple[int, tuple[float, float, float, float]]:
    value = metadata.plain_value(raw)
    if isinstance(value, list):
        components = max(1, min(4, len(value)))
        padded = [metadata.finite_float(component, 0.0) for component in value[:4]]
    else:
        components = 1
        padded = [metadata.finite_float(value, 0.0)]
    padded.extend([0.0] * (4 - len(padded)))
    return components, tuple(padded)


def _initialize_parameters(settings, imported: dict[str, object]) -> None:
    settings.parameters.clear()
    for stage, key in (("VS", "vs_cb"), ("PS", "ps_cb")):
        for name, raw in imported[key].items():
            item = settings.parameters.add()
            item.stage = stage
            item.encoding = "NAMED"
            item.name = name
            item.components, item.value = _parameter_value(raw)
    for stage, key in (
        ("VS", "vs_params"),
        ("PS", "ps_params"),
        ("HS", "hs_params"),
        ("DS", "ds_params"),
        ("GS", "gs_params"),
    ):
        for register_index, raw in enumerate(imported[key]):
            item = settings.parameters.add()
            item.stage = stage
            item.encoding = "REGISTER"
            item.name = f"r{register_index}"
            item.register_index = register_index
            item.components = 4
            _components, item.value = _parameter_value(raw)
    settings.active_parameter_index = _clamped_index(settings.parameters, 0)


def initialize_material_metadata(material, family: str | None = None) -> None:
    """Load nested material extras into ergonomic settings."""
    settings = material.ugx_gltf
    imported_hogan = metadata.normalize_hogan(material.get("ugx_hogan"))
    has_hogan = "ugx_hogan" in material
    settings.family = family or ("HOGAN" if has_hogan else "LEGACY")
    settings.material_version = max(
        0,
        metadata.integer(
            material.get("ugx_material_version"), 5 if has_hogan else 4
        ),
    )

    flags = metadata.integer(material.get("ugx_flags"), 0) & 0xFFFF_FFFF
    if not bool(getattr(material, "use_backface_culling", True)):
        flags |= 1 << 2
    settings.raw_flags = metadata.format_hex(flags, 32)
    _set_named_flags(settings, metadata.LEGACY_FLAGS, flags)
    settings.blend_type = str(
        max(0, min(3, metadata.integer(material.get("ugx_blend_type"), 0)))
    )
    settings.opacity = metadata.finite_float(
        material.get("ugx_opacity"), float(material.diffuse_color[3])
    )
    settings.spec_power = metadata.finite_float(material.get("ugx_spec_power"), 10.0)
    settings.spec_color = metadata.vector(
        material.get("ugx_spec_color"), 3, (1.0, 1.0, 1.0)
    )
    settings.env_reflectivity = metadata.finite_float(
        material.get("ugx_env_reflectivity"), 1.0
    )
    settings.env_sharpness = metadata.finite_float(
        material.get("ugx_env_sharpness"), 1.0
    )
    settings.env_fresnel = metadata.finite_float(
        material.get("ugx_env_fresnel"), 0.5
    )
    settings.env_fresnel_power = metadata.finite_float(
        material.get("ugx_env_fresnel_power"), 4.0
    )
    settings.accessory_index = str(
        metadata.integer(material.get("ugx_accessory_index"), 0) & 0xFFFF_FFFF
    )
    _initialize_maps(settings, material.get("ugx_maps"))
    _initialize_uvw(settings, material.get("ugx_uvw_velocity"))

    settings.hogan_ufx_version = max(0, int(imported_hogan["ufx_version"]))
    settings.hogan_blend_mode = max(0, int(imported_hogan["blend_mode"]))
    settings.hogan_shadow_requires_consts = imported_hogan["shadow_requires_consts"]
    settings.hogan_skinned = imported_hogan["skinned"]
    settings.hogan_terrain_blending = imported_hogan["terrain_blending"]
    settings.hogan_textures = imported_hogan["textures"]
    _initialize_permutations(settings, imported_hogan)
    _initialize_parameters(settings, imported_hogan)
    settings.initialized = True


def _serialized_maps(settings) -> dict[str, list[dict[str, object]]]:
    result: dict[str, list[dict[str, object]]] = {}
    for item in settings.maps:
        if not item.texture_path:
            continue
        result.setdefault(item.map_type, []).append(
            {
                "name": item.texture_path,
                "channel": int(item.channel),
                "flags": int(item.flags),
            }
        )
    return result


def _serialized_uvw(settings) -> list[list[float]]:
    by_type = {item.map_type: list(item.velocity) for item in settings.uvw_velocities}
    return [list(by_type.get(map_type, (0.0, 0.0, 0.0))) for map_type in metadata.MAP_TYPE_IDS]


def _serialized_permutations(settings) -> tuple[list[dict[str, object]], str | None]:
    result = []
    first_flags = None
    for index, item in enumerate(settings.permutations):
        if index >= 4:
            break
        flags = _composed_hogan_flags(item)
        name = item.name
        if metadata.shader_flags_from_name(name) is not None:
            name = metadata.replace_shader_flags(name, flags)
            item.name = name
        if first_flags is None:
            first_flags = f"{flags:016x}"
        result.append(
            {
                "name": name,
                "hash": metadata.parse_uint(item.hash, 32, "Permutation hash"),
            }
        )
    return result, first_flags


def _serialized_parameters(settings) -> dict[str, object]:
    named = {"VS": {}, "PS": {}}
    registers = {stage: {} for stage in ("VS", "PS", "HS", "DS", "GS")}
    for item in settings.parameters:
        value = [float(component) for component in item.value]
        if item.encoding == "NAMED" and item.stage in named and item.name:
            named[item.stage][item.name] = (
                value[0] if item.components == 1 else value[: item.components]
            )
        elif item.encoding == "REGISTER":
            registers[item.stage][int(item.register_index)] = value

    result: dict[str, object] = {}
    if named["VS"]:
        result["vs_cb"] = named["VS"]
    if named["PS"]:
        result["ps_cb"] = named["PS"]
    for stage, key in (
        ("VS", "vs_params"),
        ("PS", "ps_params"),
        ("HS", "hs_params"),
        ("DS", "ds_params"),
        ("GS", "gs_params"),
    ):
        stage_registers = registers[stage]
        if stage_registers:
            highest = max(stage_registers)
            result[key] = [
                stage_registers.get(index, [0.0, 0.0, 0.0, 0.0])
                for index in range(highest + 1)
            ]
    return result


def material_extras(material) -> dict[str, object]:
    """Build exact JSON metadata for one Blender material."""
    settings = material.ugx_gltf
    extras: dict[str, object] = {
        "ugx_material_version": int(settings.material_version)
    }
    if settings.family == "LEGACY":
        flags = _composed_legacy_flags(settings)
        material.use_backface_culling = not bool(flags & (1 << 2))
        extras.update(
            {
                "ugx_flags": flags & ~(1 << 2),
                "ugx_blend_type": int(settings.blend_type),
                "ugx_opacity": float(settings.opacity),
                "ugx_spec_power": float(settings.spec_power),
                "ugx_spec_color": list(settings.spec_color),
                "ugx_env_reflectivity": float(settings.env_reflectivity),
                "ugx_env_sharpness": float(settings.env_sharpness),
                "ugx_env_fresnel": float(settings.env_fresnel),
                "ugx_env_fresnel_power": float(settings.env_fresnel_power),
                "ugx_accessory_index": metadata.parse_uint(
                    settings.accessory_index, 32, "Accessory index"
                ),
                "ugx_maps": _serialized_maps(settings),
                "ugx_uvw_velocity": _serialized_uvw(settings),
            }
        )
    else:
        permutations, shader_flags = _serialized_permutations(settings)
        hogan = {
            "shader_permutations": permutations,
            "ufx_version": int(settings.hogan_ufx_version),
            "blend_mode": int(settings.hogan_blend_mode),
            "shadow_requires_consts": bool(settings.hogan_shadow_requires_consts),
            "skinned": bool(settings.hogan_skinned),
            "terrain_blending": bool(settings.hogan_terrain_blending),
            "textures": settings.hogan_textures,
            **_serialized_parameters(settings),
        }
        if shader_flags is not None:
            hogan["shader_flags"] = shader_flags
        extras["ugx_hogan"] = hogan
    return extras


def _id_property_value(value):
    """Convert exact JSON values to Blender's signed-32-bit ID property range."""
    if isinstance(value, bool) or value is None:
        return value
    if isinstance(value, int):
        return value if -0x8000_0000 <= value <= 0x7FFF_FFFF else float(value)
    if isinstance(value, dict):
        return {key: _id_property_value(item) for key, item in value.items()}
    if isinstance(value, (list, tuple)):
        return [_id_property_value(item) for item in value]
    return value


def save_material_metadata(material) -> None:
    """Write editor values to Blender custom properties for blend persistence."""
    extras = material_extras(material)
    for key in (
        "ugx_material_version",
        "ugx_flags",
        "ugx_blend_type",
        "ugx_opacity",
        "ugx_spec_power",
        "ugx_spec_color",
        "ugx_env_reflectivity",
        "ugx_env_sharpness",
        "ugx_env_fresnel",
        "ugx_env_fresnel_power",
        "ugx_accessory_index",
        "ugx_maps",
        "ugx_uvw_velocity",
        "ugx_hogan",
    ):
        _delete_custom_property(material, key)
    for key, value in extras.items():
        material[key] = _id_property_value(value)
    settings = material.ugx_gltf
    settings.initialized = True


def material_extras_for_objects(objects) -> dict[str, dict[str, object]]:
    """Return exact material extras keyed by Blender's unique material names."""
    materials = {}
    for obj in objects:
        if getattr(obj, "type", None) != "MESH":
            continue
        for material in obj.data.materials:
            if material is not None:
                materials[material.name] = material_extras(material)
    return materials


def initialize_objects_metadata(objects) -> None:
    """Hydrate all unique meshes and materials used by an object collection."""
    meshes = {}
    materials = {}
    for obj in objects:
        if getattr(obj, "type", None) != "MESH":
            continue
        meshes[obj.data.as_pointer()] = obj.data
        for material in obj.data.materials:
            if material is not None:
                materials[material.as_pointer()] = material
    for mesh in meshes.values():
        initialize_mesh_metadata(mesh)
    for material in materials.values():
        initialize_material_metadata(material)


def is_stumpy_max_scene(scene, objects) -> bool:
    """Return whether exported data carries both Stumpy and 3ds Max markers."""
    scene_keys = set(scene.keys())
    has_stumpy = bool({"ugxMats", "textureMode"} & scene_keys)
    has_max = any("MaxHandle" in obj for obj in objects)
    return has_stumpy and has_max


STUMPY_MAP_FIELDS = (
    ("diffuse", "df"),
    ("normal", "nm"),
    ("gloss", "sp"),
    ("opacity", "op"),
    ("xform", "xf"),
    ("emissive", "em"),
    ("ao", "ao"),
    ("env", "env"),
    ("envmask", "envm"),
    ("emxform", "emxf"),
    ("distortion", "dt"),
    ("highlight", "hl"),
    ("modulate", "md"),
)

STUMPY_FLAG_FIELDS = (
    ("color_gloss", "cFlagColorGloss"),
    ("opacity_valid", "cFlagOpacityValid"),
    ("two_sided", "cFlagTwoSided"),
    ("disable_shadows", "cFlagDisableShadows"),
    ("global_env", "cFlagGlobalEnv"),
    ("terrain_conform", "cFlagTerrainConform"),
    ("local_reflection", "cFlagLocalReflection"),
    ("disable_shadow_reception", "cFlagDisableShadowReception"),
)


def _stumpy_material_record(scene, obj) -> dict[str, object] | None:
    records = metadata.plain_value(scene.get("ugxMats"))
    if not isinstance(records, list) or not records:
        return None
    selector = obj.get("ugxMatIndex")
    if isinstance(selector, str) and not selector.isdecimal():
        return next(
            (
                record
                for record in records
                if isinstance(record, dict) and record.get("matName") == selector
            ),
            None,
        )
    try:
        stored_index = int(selector)
    except (TypeError, ValueError, OverflowError):
        return None
    record_index = stored_index - 1
    if not 0 <= record_index < len(records):
        return None
    record = records[record_index]
    return record if isinstance(record, dict) else None


def _stumpy_channel(record: dict[str, object], suffix: str) -> int:
    try:
        return max(0, min(32_767, int(record.get(f"chan_{suffix}", 0))))
    except (TypeError, ValueError, OverflowError):
        return 0


def apply_stumpy_material(scene, obj) -> int:
    """Migrate one object's saved Stumpy material record to UGX metadata."""
    record = _stumpy_material_record(scene, obj)
    if record is None:
        return 0
    materials = [material for material in obj.data.materials if material is not None]
    if not materials:
        material = bpy.data.materials.new(str(record.get("matName", "UGX Material")))
        obj.data.materials.append(material)
        materials = [material]
    for material in materials:
        initialize_material_metadata(material, family="LEGACY")
        settings = material.ugx_gltf
        settings.material_version = 4
        settings.blend_type = "0"
        settings.opacity = 1.0
        settings.spec_power = metadata.finite_float(record.get("spec"), 500.0)
        settings.spec_color = tuple(
            metadata.finite_float(record.get(key), 1.0)
            for key in ("specR", "specG", "specB")
        )
        settings.env_reflectivity = metadata.finite_float(record.get("envRefl"), 1.0)
        settings.env_sharpness = metadata.finite_float(record.get("envSharp"), 1.0)
        settings.env_fresnel = metadata.finite_float(record.get("envFres"), 1.0)
        settings.env_fresnel_power = metadata.finite_float(record.get("envFresP"), 0.5)
        settings.accessory_index = "0"
        for attribute, source in STUMPY_FLAG_FIELDS:
            setattr(settings, attribute, bool(record.get(source, False)))
        settings.maps.clear()
        for map_type, suffix in STUMPY_MAP_FIELDS:
            path = str(record.get(f"path_{suffix}", ""))
            if not path:
                continue
            item = settings.maps.add()
            item.map_type = map_type
            item.texture_path = path
            item.channel = _stumpy_channel(record, suffix)
            item.flags = 0
        _initialize_uvw(
            settings,
            [
                metadata.vector(record.get(f"uvw_{suffix}"), 3, (0.0, 0.0, 0.0))
                for _map_type, suffix in STUMPY_MAP_FIELDS
            ],
        )
        save_material_metadata(material)
    return len(materials)


def resolve_authoring_transform(
    scene,
    objects,
    preset: str,
    custom_scale: float,
    custom_mirror_x: bool,
) -> tuple[float, bool, str]:
    """Resolve an export preset to scale, reflection, and a UI label."""
    resolved = preset
    if preset == "AUTO":
        resolved = "STUMPY" if is_stumpy_max_scene(scene, objects) else "STANDARD"
    if resolved == "STUMPY":
        return STUMPY_MODEL_SCALE, True, "Stumpy 3ds Max"
    if resolved == "CUSTOM":
        return float(custom_scale), bool(custom_mirror_x), "Custom"
    return 1.0, False, "Blender / UGX"


def armature_for_mesh_object(obj):
    """Find the armature responsible for deforming a mesh object."""
    for modifier in obj.modifiers:
        if modifier.type == "ARMATURE" and modifier.object is not None:
            return modifier.object
    if obj.parent is not None and obj.parent.type == "ARMATURE":
        return obj.parent
    return None


def exclusively_weighted_bone(obj) -> str | None:
    """Return a bone name when every mesh vertex has exactly that full weight."""
    if obj.type != "MESH" or not obj.data.vertices:
        return None
    names = {group.index: group.name for group in obj.vertex_groups}
    common = None
    for vertex in obj.data.vertices:
        weighted = [
            group
            for group in vertex.groups
            if group.weight > 0.000001 and group.group in names
        ]
        if len(weighted) != 1 or abs(weighted[0].weight - 1.0) > 0.0001:
            return None
        name = names[weighted[0].group]
        if common is None:
            common = name
        elif common != name:
            return None
    return common


def prepare_objects_for_export(scene, objects) -> None:
    """Synchronize every exported datablock before Blender writes glTF extras."""
    if not scene.ugx_gltf.initialized:
        initialize_scene_metadata(scene)
    save_scene_metadata(scene)
    meshes = {}
    materials = {}
    for obj in objects:
        if getattr(obj, "type", None) != "MESH":
            continue
        meshes[obj.data.as_pointer()] = obj.data
        for material in obj.data.materials:
            if material is not None:
                materials[material.as_pointer()] = material
    for mesh in meshes.values():
        if not mesh.ugx_gltf.initialized:
            initialize_mesh_metadata(mesh)
        save_mesh_metadata(mesh)
    for material in materials.values():
        if not material.ugx_gltf.initialized:
            initialize_material_metadata(material)
        save_material_metadata(material)


CLASSES = (
    UGXTextureMapSettings,
    UGXUVWVelocitySettings,
    UGXShaderPermutationSettings,
    UGXShaderParameterSettings,
    UGXMeshSettings,
    UGXMaterialSettings,
    UGXSceneSettings,
)


def register():
    """Register persistent scene, mesh, and material properties."""
    for cls in CLASSES:
        bpy.utils.register_class(cls)
    bpy.types.Mesh.ugx_gltf = PointerProperty(type=UGXMeshSettings)
    bpy.types.Material.ugx_gltf = PointerProperty(type=UGXMaterialSettings)
    bpy.types.Scene.ugx_gltf = PointerProperty(type=UGXSceneSettings)


def unregister():
    """Unregister persistent properties."""
    del bpy.types.Scene.ugx_gltf
    del bpy.types.Material.ugx_gltf
    del bpy.types.Mesh.ugx_gltf
    for cls in reversed(CLASSES):
        bpy.utils.unregister_class(cls)
