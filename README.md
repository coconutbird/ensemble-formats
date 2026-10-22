# ensemble-formats

Rust crates that read and write the binary file formats of Halo Wars: Definitive Edition (HW1)
and Halo Wars 2 (HW2). `ensemble-assets` and `openensemble` use these crates.

[`AGENTS.md`](AGENTS.md) gives the rules for changes: writing style, lints, layout, crate layers
and checks.

## Crates

"R" is read support and "W" is write support. "no_std" crates use `alloc` and `nostdio`.

| Crate            | Format or purpose                                                                    | R   | W   | Game | no_std |
| ---------------- | ------------------------------------------------------------------------------------ | --- | --- | ---- | ------ |
| `bdt`            | `BBinaryDataTree` packed document tree (inside XMB and UGX materials)                | yes | yes | both | yes    |
| `bdt-serde`      | Serde mapping between Rust types and `bdt` nodes                                     | yes | yes | both | yes    |
| `binary-support` | String table and pointer fix-up helpers for binary writers                           | —   | —   | both | yes    |
| `cli-support`    | Exit codes, size format, archive extraction and shader output for the CLI tools      | —   | —   | both | no     |
| `ddx`            | DDX textures: DDS files (DE) and ECF-wrapped Xbox 360 DDX; decode to RGBA            | yes | DDS | HW1  | yes    |
| `ecf`            | ECF chunk container, checksums and deflate streams                                   | yes | yes | both | yes    |
| `era`            | ERA archives: ECF with TEA encryption and Merkle signatures                          | yes | yes | HW1  | yes    |
| `era-cli`        | `era` tool                                                                           | —   | —   | HW1  | no     |
| `fxb`            | FXB0 compiled effect files (`*.bin` in `shader.era`) with DXBC shaders               | yes | yes | HW1  | yes    |
| `fxb-cli`        | `fxb` tool                                                                           | —   | —   | HW1  | no     |
| `lgt`            | LGT animated light scenes from the 3ds Max light exporter                            | yes | no  | HW1  | yes    |
| `pkg`            | PKG (`capack`) archives with FNV-1a name hashes, versions 1 and 2                    | yes | yes | HW2  | yes    |
| `pkg-cli`        | `pkg` tool                                                                           | —   | —   | HW2  | no     |
| `test-utils`     | Game directory, file discovery and ERA helpers for tests                             | —   | —   | both | no     |
| `uax`            | UAX animations: ECF with a packed x64 Granny `file_info` graph                       | yes | yes | both | yes    |
| `uax-cli`        | `uax` tool                                                                           | —   | —   | both | no     |
| `ufx`            | UFXS compiled shaders: D3D12 root signature, DXBC stages, vertex input, CB inference | yes | no  | HW2  | yes    |
| `ufx-cli`        | `ufx` tool                                                                           | —   | —   | HW2  | no     |
| `ugx`            | UGX models: v4 (HW1) and v6 (HW2) geometry, materials, Granny skeleton, AABB tree    | yes | yes | both | yes    |
| `ugx-cli`        | `ugx` tool                                                                           | —   | —   | both | no     |
| `ugx-gltf`       | UGX to glTF 2.0 and back; v4 to v6 conversion                                        | yes | yes | both | no     |
| `xmb`            | XMB binary XML (ECF with a `bdt` tree), PC and Xbox 360 layouts                      | yes | yes | both | yes    |
| `xmb-cli`        | `xmb` tool                                                                           | —   | —   | both | no     |
| `xmb-serde`      | Deserialize XMB or text XML into Rust types                                          | yes | no  | both | yes    |
| `xml`            | Small text XML reader, writer and escape helpers                                     | yes | yes | both | yes    |
| `xsd`            | XSD simulation terrain: header, height grid, tile types                              | yes | no  | HW1  | yes    |
| `xtd`            | XTD terrain visual data: atlas, tessellation, lighting, AO, alpha                    | yes | yes | HW1  | yes    |
| `xtt`            | XTT terrain textures: atlas links, albedo, decals, roads, foliage                    | yes | yes | HW1  | yes    |

`era`, `ecf`, `pkg`, `ugx`, `bdt-serde` and `xmb-serde` put their `std` code behind a `std`
feature. `era` also has a `rayon` feature.

## Command-line tools

| Binary | Crate     | Commands                                                                                             |
| ------ | --------- | ---------------------------------------------------------------------------------------------------- |
| `era`  | `era-cli` | `list`, `info`, `extract`, `create`, `verify` (Merkle signature)                                     |
| `pkg`  | `pkg-cli` | `list`, `info`, `extract`, `create`                                                                  |
| `xmb`  | `xmb-cli` | `to-xml`, `to-xmb`, `info`; a file argument without a command selects the direction                  |
| `ugx`  | `ugx-cli` | `info`, `to-gltf`, `from-gltf`, `convert`, `dump`, `diff`, `scan`                                    |
| `uax`  | `uax-cli` | `info`, `dump`                                                                                       |
| `fxb`  | `fxb-cli` | `list`, `info`, `extract`                                                                            |
| `ufx`  | `ufx-cli` | no subcommands; flags `--disasm`, `--cb-only`, `--flags`, `--infer-params`, `--cfg`, `--batch-infer` |

### UGX version conversion

`ugx convert` repacks a model between HW1 v4 and HW2 v6:

```text
ugx convert --input model_v4.ugx --output model_v6.ugx --version hw2
ugx convert --input model_v6.ugx --output model_v4.ugx --version hw1
```

The command rebuilds vertex packing, materials, skeleton and section data, bounds, accessories
and the AABB tree. Then it reads the output again in strict mode. For an editable round trip,
use glTF:

```text
ugx to-gltf --input model.ugx --output model.gltf
ugx from-gltf --input model.gltf --output rebuilt.ugx --version hw1
ugx diff --original model.ugx --roundtrip rebuilt.ugx
```

`from-gltf` writes v6 (`hw2`) if you do not give `--version`. `diff` compares the two files chunk
by chunk. The two versions do not hold the same data. A v4 section cannot store the v6 LOD
distances. Material conversion between legacy and Hogan materials uses the nearest texture and
shader mapping.

### Blender extension

`blender/ugx_gltf` is a Blender 4.2+ import and export extension. It calls the `ugx` tool. To
make an installable archive for the current platform, run `mise run blender:package`. See
[`blender/ugx_gltf/README.md`](blender/ugx_gltf/README.md).

## Tests and verification

Run the normal tests:

```sh
cargo test --workspace --all-features
```

Tests that need game files have `#[ignore = "requires ..."]`. To run them:

1. Copy `.env.example` to `.env`.
2. Set `HW1_GAME_DIR` to the Halo Wars: Definitive Edition directory (the directory with the
   `.era` files).
3. Set `HW2_GAME_DIR` to an extracted Halo Wars 2 directory (loose `.ugx`, `.uax`, `.ufx` files).
4. Run `cargo test --workspace --all-features -- --ignored`.

`test_utils::env::require_game_dir` reads the variable from the environment or from `.env`. It
panics if the variable is not set or is not a directory. Thus an ignored test does not skip
silently. Some ignored tests use local fixtures, not a game directory:

- `ddx`: `root.era` in the workspace root or in `crates/ddx`.
- `xtd`, `xtt`: extracted files below `test_extract/scenario/skirmish/design/blood_gulch/`. One
  `xtt` test reads `--filter/scenario/skirmish/design/release/release.xtt`.

### Status

| Format                        | What the tests check                                                                                        | Result                                           |
| ----------------------------- | ----------------------------------------------------------------------------------------------------------- | ------------------------------------------------ |
| UAX                           | All UAX files in all HW1 ERAs: strict parse, semantic write and read, byte-exact raw round trip             | 2,301 files pass                                 |
| UGX v4                        | All UGX files in all HW1 ERAs parse and validate as v4                                                      | 2,468 files, 18,411 sections pass                |
| UGX v6                        | All loose HW2 UGX files parse and validate as v6                                                            | Last recorded run: 15,632 files, 92,726 sections |
| UGX to glTF                   | 100 HW1 models per test: same-version round trip and v4 to v6 conversion                                    | Fails: see [Known defects](#known-defects)       |
| FXB                           | Up to 500 FXB files from shader directories in the HW1 ERAs round trip byte-exact                           | 66 files pass (all such files)                   |
| UFX                           | All loose HW2 UFX files: stage table, DXBC ranges, vertex-input strides                                     | Last recorded run: 2,893 files                   |
| UAX (HW2)                     | First 50 loose HW2 UAX files round trip byte-exact                                                          | Not recorded                                     |
| DDX                           | All DDX files in a local `root.era` parse; write each as DDS, parse again, compare size and payload         | Local fixture only                               |
| XTD, XTT                      | One Blood Gulch file (and one Release XTT): parse, decode, round trip of the common prefix without checksum | Fixture only; no corpus test                     |
| ECF, ERA, BDT, XMB, bdt-serde | Synthetic unit tests                                                                                        | No corpus test                                   |
| XSD, LGT                      | Synthetic unit tests                                                                                        | No test with game files                          |
| PKG                           | No tests                                                                                                    | Round trip fails: [#4]                           |

The HW1 rows with numbers come from a run against the Steam release of Halo Wars: Definitive
Edition. The HW2 numbers come from an earlier run; no HW2 directory was available for the latest
run.

## Format coverage and gaps

The engine evidence comes from the retail `xgameFinal.exe` (x64, base `0x140000000`). Names
such as `BTerrainIOLoader_loadXTDInternal` are names in the shared IDA database. The file counts
come from the 46 ERA archives of the Steam release.

### Binary formats in HW1

| Format                     | Engine reader                                                                                          | Status in this repository                                                                                                                                                                                                                                                                   |
| -------------------------- | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| ECF                        | `ECF_ReadAndValidateStream` @ `0x140686530`, `ECFArchive__load_FromStream` @ `0x140685a30`             | Done: `ecf`                                                                                                                                                                                                                                                                                 |
| ERA (`.era`)               | header check `sub_1406846C0` (`0x17FDBA9C`), `BArchiveManager_reloadRootArchive` @ `0x14017ce90`       | Done: `era` (read, write, encrypt, sign, verify)                                                                                                                                                                                                                                            |
| XMB (`*.xmb`)              | `BXMLReader_load` @ `0x140620eb0`                                                                      | Done: `xmb`. The XML schemas are in `ensemble-assets`.                                                                                                                                                                                                                                      |
| UGX (`.ugx`)               | `BGrannyModel__load_UGX_File` @ `0x14073d6f0` (chunks `0x700`–`0x705`)                                 | Done: `ugx`, including the Granny chunk `0x703`                                                                                                                                                                                                                                             |
| UAX (`.uax`)               | `BGrannyAnimation__load_UAX_File` @ `0x140735a30` (chunk `0x700`)                                      | Done: `uax`                                                                                                                                                                                                                                                                                 |
| DDX (`.ddx`), XPR (`.xpr`) | `BD3DTextureManager_BManagedTexture_load` @ `0x1407c22e0`                                              | Done: `ddx`. The 10 retail `.xpr` files are DDS files, so `ddx` reads them too.                                                                                                                                                                                                             |
| FXB (`*.bin`)              | `BFXLEffectFileLoader_createEffect` @ `0x1407c87b0`, magic `fxb0` @ `0x1411e4ce8`                      | Done: `fxb`                                                                                                                                                                                                                                                                                 |
| XTD (`.xtd`)               | `BTerrainIOLoader_loadXTDInternal` @ `0x140668db0`                                                     | Done: `xtd` reads all chunks that the engine reads (`0x1111`, `0x2222`, `0x8888`, `0xAAAA`–`0xDDDD`)                                                                                                                                                                                        |
| XTT (`.xtt`)               | `BTerrainIOLoader_loadXTTInternal` @ `0x14066afa0`                                                     | Done: `xtt` reads all chunks that the engine reads (`0x1111`, `0x2222`, `0x6666`, `0x8888`, `0xAAAA`, `0xBBBB`)                                                                                                                                                                             |
| XSD (`.xsd`)               | `BTerrainSimRep_loadECFXSDInternal` @ `0x1400657e0`                                                    | Partial: `xsd` reads `0x1111`, `0x2222`, `0x8888`. It skips `0x4444` and `0xCCCC` (byte grids, one byte for each data tile), `0xAAAA` and `0xABBB` (16-bit height grids with their own header). `BUnitActionMoveAir_computeFlightStep` reads the `0xABBB` grid (flight heights). No writer. |
| XSD delta (`.xsddelta`)    | suffix `.XSDDelta` @ `0x1411b78c8`, used by `sub_14037D240` (terrain load in `BScenario_load`)         | Missing. Two retail files (`crevice`, `exile`); not ECF.                                                                                                                                                                                                                                    |
| XTH (`.xth`)               | `BTerrainHeightField_loadECFXTH` @ `0x140667a90`                                                       | Missing. ECF file with precomputed height-field data; 35 retail files.                                                                                                                                                                                                                      |
| LRP (`.lrp`)               | `BLrpTree_loadECFLRP` @ `0x140255590`, called from `BScenario_load`                                    | Missing. ECF long-range pathing tree; 35 retail files.                                                                                                                                                                                                                                      |
| LGT (`.lgt`)               | `BLightEffectManager_init` @ `0x1407100f0`                                                             | Partial: `lgt` reads; no writer; no test with the 127 retail files.                                                                                                                                                                                                                         |
| ABC font (`.abc`)          | `courier_10.abc` @ `0x1411b43e0` (`sub_14035F890`), `courier_12.abc` @ `0x1411dbc50` (`sub_1406E9850`) | Missing. Glyph metrics next to a `.tga` page; 8 retail files in `fonts\`.                                                                                                                                                                                                                   |
| Save game (`.sav`)         | `BSaveGame` writer `sub_140366C70`, reader `sub_14036DD30`; path `cache:\savegame` (`sub_140365DC0`)   | Missing                                                                                                                                                                                                                                                                                     |
| Recorded game (`.rec`)     | path `cache:\recordgame` (`sub_1403585F0`)                                                             | Missing                                                                                                                                                                                                                                                                                     |

The engine also reads text and XML files: XMB-wrapped `.vis`, `.pfx`, `.dmg`, `.tfx`, `.cin`,
`.scn`, `.sc2`, `.sc3`, `.gls`, `.tactics`, `.physics`, `.blueprint`, `.shp`, the database
files, the text `.fls` SH fill files and the `startup\*.cfg` files. `xmb` reads and writes the
container. `ensemble-assets` owns the schemas; see its
[data coverage](https://github.com/coconutbird/ensemble-assets#data-coverage) and
[asset resolution](https://github.com/coconutbird/ensemble-assets/blob/main/docs/asset-resolution.md)
documents.

### Third-party formats

| Format                                     | Engine evidence                                                                    | Status                                                                |
| ------------------------------------------ | ---------------------------------------------------------------------------------- | --------------------------------------------------------------------- |
| Granny data                                | Granny 2.11 runtime (`granny_file_format.cpp` @ `0x140f1b930`)                     | Only inside UGX and UAX (`ugx`, `uax`). The game has no loose `.gr2`. |
| Wwise banks (`Sounds.pck`, `.bnk`, `.wem`) | `init.bnk` (`sub_140007CE0`), `%u.bnk` (`sub_1407FD9A0`), `%u.wem` @ `0x1411e7d18` | Missing                                                               |
| Scaleform (`.gfx`, `.swf`)                 | `scaleform.gfx` @ `0x140f7f448`; 138 `.gfx` and 19 `.swf` retail files             | Missing                                                               |
| Bink video (`.bik`)                        | `video\%s.bik` @ `0x14119b9d8`; 367 retail files                                   | Missing                                                               |
| TGA, TTF, TTC, WAV                         | standard formats                                                                   | Not in scope                                                          |

### HW2

HW2 is not in the IDA database. `pkg`, `ufx` and the v6 paths of `ugx` and `uax` cover the HW2
files that the tests use. [`notes/hw2_asset_resolution.md`](notes/hw2_asset_resolution.md) gives
the HW2 asset layout.

### Known defects

- `ugx-gltf`: the HW1 corpus tests fail. On 100 sampled models, the same-version round trip fails
  for 1 model and the v4 to v6 conversion fails for 8 models. The writer rejects sections whose
  `MaxBones` (3) is smaller than the 4 influences in the vertex data, for example
  `art\unsc\vehicle\warthog_01\warthogmarine_02.ugx`. In three other models (for example
  `art\covenant\vehicle\wraith_01\shield_01.ugx`), the conversion changes bone weight 0 of vertex
  0 from 0 to 1.
- `pkg`: `pkg extract` cannot read the last entry of an archive that `pkg create` made ([#4]).
- Workspace: the `cfglib` dependency of `ufx` points at a deleted branch ([#5]).
- `bdt`: the little-endian and big-endian readers repeat the node decoding ([#6]).
- `xtd`, `xtt`: the readers drop chunks with unknown IDs, and the writers reject them.
- `ugx`: the section bone remap assumes one byte for each entry
  (`crates/ugx/src/types/section.rs`).

[#4]: https://github.com/coconutbird/ensemble-formats/issues/4
[#5]: https://github.com/coconutbird/ensemble-formats/issues/5
[#6]: https://github.com/coconutbird/ensemble-formats/issues/6

## Format details

### ECF

ECF is the base container of most binary files. The header is 32 bytes, big-endian, with magic
`0xDABA7737` and a file ID (for example `0xAAC93746` for UGX). Each chunk has a 64-bit ID, an
offset, a size, an alignment and flags. A chunk can be a deflate stream.

### ERA

ERA is an ECF container (file ID `0x17FDBA9C`) with TEA encryption in CBC mode. The IV is
`0x15EF0AF334248FE2` and the archive password is `3zDdptN*rV=qOkRbE*NAuWM6`. An archive can have a
Merkle one-time signature at offset 48. The verifier walks the Merkle tree with a KISS PRNG
that the header hash seeds. The HW1 public key (tree root hash) is
`FD016BE719C21BD14F84CA9961A3F3CB18221F25`.

### UGX

| Chunk   | Content                                              |
| ------- | ---------------------------------------------------- |
| `0x700` | Cached data: header, sections, bones, accessories    |
| `0x701` | Index buffer                                         |
| `0x702` | Vertex buffer                                        |
| `0x703` | Granny data: skeleton and mesh bindings (`"gr2ugx"`) |
| `0x704` | Materials (`bdt` tree)                               |
| `0x705` | AABB tree (v4 only)                                  |

v4 sections are 152 bytes and embed a `UnivertPacker`. v6 sections are 72 bytes. The matching
UFX vertex declaration gives the v6 vertex layout.

### UAX

Chunk `0x700` holds a packed x64 Granny `file_info` graph of `0x94` bytes. It has no separate
Granny section header. Pointers are 64-bit little-endian offsets from the start of the chunk.
The semantic reader and writer support all 19 curve formats of the game, and vector, transform,
text and loop data.

### DDX

DE `.ddx` files are DDS files (magic `DDS `). Xbox 360 DDX files are ECF containers (file ID
`0x13CF5D01`) with a header chunk and deflated mip chunks. The writer writes DDS. A DDS file
whose `caps2` declares six faces is a cubemap; `DdxTexture::subresources` splits faces and mips.

### Terrain

| File   | Content                                                                     |
| ------ | --------------------------------------------------------------------------- |
| `.xtd` | Visual chunks, position and normal atlas, tessellation, lighting, AO, alpha |
| `.xtt` | Texture atlas links, albedo atlas, decals, roads, foliage                   |
| `.xsd` | Simulation height grid and tile types                                       |

## Usage

```rust
// Open and decrypt an ERA archive, then read one entry.
let file = std::io::BufReader::new(std::fs::File::open("root.era")?);
let mut archive = era::Reader::from_encrypted(file, era::TeaKeys::default_archive_keys())?;
if let Some(index) = archive.find_by_name("data\\objects.xml.xmb") {
    let bytes = archive.read_entry(index)?;
    // XMB to XML and back.
    let doc = xmb::Reader::read(&bytes)?;
    let xml = doc.to_xml();
    let xmb_bytes = xmb::Writer::write(&xmb::Document::from_xml(&xml)?, xmb::Format::PC)?;
}

// Read the chunks of an ECF file.
let ecf = ecf::Reader::new(&data)?;
for (i, chunk) in ecf.chunks().iter().enumerate() {
    println!("chunk {:016X}: {} bytes", chunk.id, ecf.chunk_data(i)?.len());
}

// Read a texture and terrain files.
let texture = ddx::DdxTexture::from_bytes(&texture_bytes)?;
let xtd = xtd::Reader::read(&xtd_bytes)?;
let xtt = xtt::Reader::read(&xtt_bytes)?;
```

## License

MIT
