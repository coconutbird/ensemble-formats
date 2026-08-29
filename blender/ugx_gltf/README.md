# UGX glTF for Blender

This Blender 4.2+ extension imports and exports Halo Wars UGX models. Blender's
built-in glTF support handles scene data, while the repository's Rust `ugx`,
`ugx-gltf`, and `ugx-cli` crates perform all UGX parsing, validation, rebuilding,
and serialization.

## Features

- Import `.ugx` from the File menu or by dragging it into the 3D viewport.
- Export the scene or selected objects as HW1 DE (v4) or HW2 (v6) UGX.
- Round-trip mesh sections, normals, tangents, UVs, vertex colors, materials,
  bones, skin weights, rigid bone bindings, and UGX glTF extras.
- Edit regular geometry, armatures, weights, materials, and transforms with
  Blender's standard tools before export.
- Edit section LOD distances, maximum instance count, and common material
  metadata from the Properties editor. Advanced Hogan and texture-map metadata
  remains available under each datablock's Custom Properties.
- Validate source ECF checksums and remember the detected source game version.

UGX stores model geometry and a bind skeleton, not UAX animation clips. Blender
animations are therefore not written to UGX by this extension. Game texture
references remain in material metadata; Blender can only display an image when
the referenced file is available in a format its glTF importer can load.

UGX has one model skeleton and triangle-list geometry. Export therefore supports
one Blender armature, limits skinning to four influences per vertex, and writes
the base mesh without shape-key morph targets. Blender's glTF exporter
triangulates polygon meshes during conversion. Apply or remove shape keys before
export when their deformed result must become the UGX base mesh.

## Install a bundled build

From the repository root, run:

```text
python blender/package_extension.py
```

The script builds the Rust `ugx` executable in release mode and creates a
platform-specific archive under `dist/`. In Blender, choose \*\*Edit > Preferences

> Get Extensions > Install from Disk**, select that zip, and enable **UGX glTF\*\*.

The generated manifest is restricted to the platform used to build it because
the archive contains a native Rust executable.

## Development install

Build the converter first:

```text
cargo build -p ugx-cli
```

Install or symlink `blender/ugx_gltf` as a local extension. Then open the UGX
glTF preferences and select `target/debug/ugx` (or `ugx.exe` on Windows). The
extension also checks `UGX_CLI` and `PATH` when the preference is empty.

Use **Validate Rust Converter** in the extension preferences before importing.

## Metadata round-trip

`ugx-gltf` stores data without a standard glTF equivalent in `extras`. Blender
imports these extras as Custom Properties, and this extension forces Custom
Properties on during GLB export. Keep properties whose names start with `ugx_`
unless you intentionally want the Rust importer to reconstruct defaults.

Structural fields such as section offsets, bounds, packed vertex layouts, and
AABB data are rebuilt by Rust after edits; they are not user-editable Blender
properties.
