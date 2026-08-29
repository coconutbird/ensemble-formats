"""Blender extension for UGX import and export."""

from . import operators, preferences, properties, ui


def menu_import(self, _context):
    """Add UGX to Blender's File > Import menu."""
    self.layout.operator(operators.IMPORT_SCENE_OT_ugx.bl_idname, text="UGX Model (.ugx)")


def menu_export(self, _context):
    """Add UGX to Blender's File > Export menu."""
    self.layout.operator(operators.EXPORT_SCENE_OT_ugx.bl_idname, text="UGX Model (.ugx)")


def register():
    """Register extension classes and menu entries."""
    preferences.register()
    properties.register()
    operators.register()
    ui.register()
    operators.bpy.types.TOPBAR_MT_file_import.append(menu_import)
    operators.bpy.types.TOPBAR_MT_file_export.append(menu_export)


def unregister():
    """Unregister extension classes and menu entries."""
    operators.bpy.types.TOPBAR_MT_file_export.remove(menu_export)
    operators.bpy.types.TOPBAR_MT_file_import.remove(menu_import)
    ui.unregister()
    operators.unregister()
    properties.unregister()
    preferences.unregister()
