# UGX Writer: GrannyFileInfo Header Size Fix

## Problem

Round-trip UGX files (load → modify → save → load in-game) crash or fail to load.
The root cause is in `crates/ugx/src/writer.rs` → `build_granny_data()`.

## Root Cause

The writer serializes the Granny chunk (0x703) with a `GrannyFileInfo` header of
**0x70 bytes (112)**, but the engine's `Granny_RebasePointers` expects the full
struct to be **0x94 bytes (148)**.

The skeleton pointer array is placed at offset 0x70, which overlaps with the
`TrackGroups` and `Animations` fields of the real `GrannyFileInfo` struct.
When `Granny_RebasePointers` walks the type descriptor, it reads skeleton data
as array counts/pointers and corrupts memory — causing a crash before the
`BGrannyModel::load` validation even runs.

## IDA Evidence

### GrannyFileInfo type descriptor (`off_14145C7D0` → `0x141461B60`)

Each entry is a Granny type member (44 bytes: u32 type + ptr name + ptr ref_type + 24 bytes padding).
Field names resolved from string pointers in xgameFinal.exe:

| Offset | Size | Field          | Granny Member Type   |
|--------|------|----------------|----------------------|
| +0x00  | 8    | ArtToolInfo    | reference (ptr)      |
| +0x08  | 8    | ExporterInfo   | reference (ptr)      |
| +0x10  | 8    | FromFileName   | string (ptr)         |
| +0x18  | 12   | Textures       | ref_array (count+ptr)|
| +0x24  | 12   | Materials      | ref_array (count+ptr)|
| +0x30  | 12   | Skeletons      | ref_array (count+ptr)|
| +0x3C  | 12   | VertexDatas    | ref_array (count+ptr)|
| +0x48  | 12   | TriTopologies  | ref_array (count+ptr)|
| +0x54  | 12   | Meshes         | ref_array (count+ptr)|
| +0x60  | 12   | Models         | ref_array (count+ptr)|
| +0x6C  | 12   | TrackGroups    | ref_array (count+ptr)|
| +0x78  | 12   | Animations     | ref_array (count+ptr)|
| +0x84  | 16   | ExtendedData   | variant (type+data)  |
| **0x94** | | **Total**      |                      |

Note: ref_array is 4 bytes (u32 count) + 8 bytes (u64 pointer) = 12 bytes.
Reference/string fields are 8 bytes (u64 pointer). Variant is 16 bytes (two u64 ptrs).

### BGrannyModel::load (`0x14073d2f0`)

After `Granny_RebasePointers(off_14145C7D0, chunk_data, chunk_data, 1)` succeeds,
the engine validates:

```c
// v12 = chunk_data (rebased in-place)
if ( *(u32*)(v12 + 0x60) != 1              // ModelCount must be 1
  || !**(u64**)(v12 + 0x64)                // Models[0] must be non-NULL
  || (name = *(char**)(v12 + 0x10)) == NULL // FromFileName must exist
  || _stricmp(name, "gr2ugx") )            // must be "gr2ugx"
```

### Corruption chain with current 0x70 header

1. Writer places `skeleton_ptr_array` at offset **0x70** (value = 0x78, the skeleton struct addr)
2. Skeleton struct starts at **0x78** (Name ptr, BoneCount, Bones ptr, LODType)
3. `Granny_RebasePointers` walks the full type descriptor:
   - **+0x6C** TrackGroupCount: reads 0 from padding → safe, but...
   - **+0x70** TrackGroupPtr: reads `0x78` (skeleton struct addr) → rebases it → **corrupts skeleton ptr array**
   - **+0x78** AnimationCount: reads skeleton Name ptr lower 4 bytes → huge count
   - **+0x7C** AnimationPtr: reads garbage → tries to walk N animation structs → **crash**
   - **+0x84** ExtendedData type ptr: reads BoneCount/Bones ptr → interprets as type descriptor → **crash**

## Fix

### File: `crates/ugx/src/writer.rs`

**Line ~351:** Change `header_size` from `0x70` to `0x94`:

```rust
// BEFORE:
let header_size: usize = 0x70; // file_info header

// AFTER:
let header_size: usize = 0x94; // GrannyFileInfo: full 148-byte struct
                                // (13 fields: ArtToolInfo..ExtendedData)
                                // Must match engine's type descriptor at off_14145C7D0
```

**Line ~396:** Update the comment:

```rust
// BEFORE:
// ---- File info header [0x00..0x70] ----

// AFTER:
// ---- File info header [0x00..0x94] ----
// Fields not written (zero = count 0 / NULL pointer) are skipped by RebasePointers:
//   +0x00 ArtToolInfo, +0x08 ExporterInfo, +0x18 Textures, +0x24 Materials,
//   +0x3C VertexDatas, +0x48 TriTopologies, +0x6C TrackGroups, +0x78 Animations,
//   +0x84 ExtendedData
```

That's it. The buffer is already zero-initialized (`vec![0u8; ...]`), so all unused
fields are already NULL/0 — which is exactly what RebasePointers expects for empty
arrays. The only change is making the header large enough so that the skeleton pointer
array (and everything after it) doesn't overlap with the GrannyFileInfo struct.

All downstream offsets (`skeleton_ptr_array_offset`, `skeleton_struct_offset`,
`model_ptr_array_offset`, etc.) are computed relative to `header_size`, so they
shift automatically. No other code changes needed.

## Verification

After applying the fix, the granny chunk layout becomes:

```
[0x00 .. 0x94)  GrannyFileInfo (148 bytes, zero-filled except populated fields)
[0x94 .. 0x9C)  Skeleton pointer array (1 × u64)
[0x9C .. 0xB4)  Skeleton struct (24 bytes)
[0xB4 .. 0xBC)  Model pointer array (1 × u64)
[0xBC .. 0x11C) Model struct (96 bytes)
[0x11C ...)     Bone array, mesh bindings, mesh ptrs, mesh structs, bone bindings, strings
```

`Granny_RebasePointers` will now see:
- +0x6C TrackGroupCount = 0, TrackGroupPtr = NULL → skip
- +0x78 AnimationCount = 0, AnimationPtr = NULL → skip
- +0x84 ExtendedData type = NULL → skip
- All populated fields (+0x10, +0x30, +0x54, +0x60) point to valid data beyond 0x94

## Other Verified Items (no changes needed)

- ECF header endianness (big-endian) ✅
- ECF file ID `0xAAC93746` ✅
- Adler32 checksum (standard, not modified) ✅
- `GEOM_HEADER_SIGNATURE` = `0xC2340004` ✅
- `instanceIndexMultiplier` at +0x32 (power-of-two, read as i16) ✅
- Packed array headers and section layout (152-byte stride) ✅
- All chunk IDs: 0x700 cached, 0x701 IB, 0x702 VB, 0x703 granny, 0x704 materials ✅
