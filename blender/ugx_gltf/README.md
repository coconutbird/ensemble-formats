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
- Edit maximum instances, section LOD and HW2 vertex-layout metadata, every
  legacy render flag, blend/specular/environment values, all 13 texture-map
  types, game texture paths, UV channels, map flags, and UVW animation directly
  in the Properties editor.
- Preserve or override each section's skinned, rigid, or GlobalBones binding,
  binding bone, and `MaxBones` value without destructive weight editing.
- Detect legacy Stumpy/3ds Max scenes and apply their required 1.575 scale,
  X reflection, winding correction, and skeleton bind-matrix conversion.
- Edit Hogan texture patterns, render booleans, up to four shader
  name/hash/feature-mask permutations, and named or raw constant-buffer values.
- Validate source ECF checksums and remember the detected source game version.

UGX stores model geometry and a bind skeleton, not UAX animation clips. Blender
animations are therefore not written to UGX by this extension. Game texture
references remain in material metadata; Blender can only display an image when
the referenced file is available in a format its glTF importer can load.
The path shown in **Game Texture Path** is authoritative for UGX export. Assigning
a Blender Image is optional; **Copy Path** copies that image's path into the game
binding when desired.

UGX has one model skeleton and triangle-list geometry. Export therefore supports
one Blender armature, limits skinning to four influences per vertex, and writes
the base mesh without shape-key morph targets. Blender's glTF exporter
triangulates polygon meshes during conversion. Apply or remove shape keys before
export when their deformed result must become the UGX base mesh.

## Legacy Stumpy / 3ds Max scenes

The old Stumpy pipeline used a Collada/Granny coordinate conversion that is not
part of normal Blender glTF export. When a scene contains both Stumpy properties
(`ugxMats` or `textureMode`) and a 3ds Max `MaxHandle`, **Automatic** authoring
coordinates resolve to **Stumpy 3ds Max**. This mirrors X and scales geometry
and skeleton translations by 1.575 during conversion; the `.blend` is not
modified.

Some Max-imported meshes are incorrectly weighted entirely to
`GrannyRootBone`. When an `AttachBone` is present, the exporter stops with an
actionable message instead of writing a misleading model. Select the affected
mesh and armature, then use **Scene Properties > UGX glTF > Apply Stumpy
Compatibility**. The operator sets an export-only `AttachBone` override and
`MaxBones=4`, and migrates the saved Stumpy material flags, texture paths,
channels, and UVW velocities to the active Blender material; original vertex
groups remain untouched. The same binding fields can be edited manually under
**Mesh Data Properties > UGX Metadata > Section Binding**.

## Install a bundled build

From the repository root, run:

```text
mise run blender:package
```

The script builds the Rust `ugx` executable in release mode and creates a
platform-specific archive under `dist/`. In Blender, choose **Edit > Preferences
> Get Extensions > Install from Disk**, select that zip, and enable **UGX glTF**.

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
Properties on during GLB export. The UGX panels hydrate those values after
import and write exact JSON extras before conversion, including unsigned hashes
that are wider than Blender's native custom-property integer range.

Hogan feature bits are encoded in each permutation name. Changing the named
feature toggles updates that suffix on export, but a retail shader permutation's
name and 32-bit hash are a matched pair; use a known hash for the resulting
permutation rather than inventing one.

Structural fields such as section offsets, bounds, packed vertex layouts, and
AABB data are rebuilt by Rust after edits; they are not user-editable Blender
properties.
