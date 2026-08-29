# ensemble-rs

A Rust library for parsing Halo Wars Definitive Edition and Halo Wars 2 file formats.

## Crates

| Crate     | Description                             |
| --------- | --------------------------------------- |
| `ecf`     | ECF container format parser             |
| `era`     | ERA archive format (encrypted ECF)      |
| `pkg`     | PKG archive format (HW2 capack)         |
| `xmb`     | XMB binary XML parser                   |
| `ugx`     | UGX 3D model geometry parser            |
| `ufx`     | HW2 UFXS compiled-shader parser         |
| `uax`     | UAX animation format parser             |
| `ddx`     | DDX/DDS texture format reader/writer    |
| `bdt`     | BDT binary data tree (packed documents) |
| `era-cli` | CLI tool for ERA archives               |
| `pkg-cli` | CLI tool for PKG archives               |
| `xmb-cli` | CLI tool for XMB files                  |
| `ugx-cli` | CLI tool for UGX models                 |
| `ufx-cli` | CLI tool for HW2 UFX shaders            |

## Blender UGX extension

`blender/ugx_gltf` is a Blender 4.2+ import/export extension backed by the Rust
`ugx-gltf` converter. It supports editable UGX meshes, materials, skeletons,
skin weights, and UGX-specific glTF metadata. Build an installable archive for
the current platform with:

```text
python blender/package_extension.py
```

See [`blender/ugx_gltf/README.md`](blender/ugx_gltf/README.md) for installation,
development, metadata, and format details.

## UGX version conversion

The `ugx` command can repack a model directly between the Halo Wars DE v4 and
Halo Wars 2 v6 representations:

```text
ugx convert --input model_v4.ugx --output model_v6.ugx --version hw2
ugx convert --input model_v6.ugx --output model_v4.ugx --version hw1
```

Conversion rebuilds target-specific vertex packing, materials, skeleton/section
metadata, bounds, accessories, and acceleration data. The command verifies the
written version and strictly reads the output before returning success. For an
editable workflow, import either version with the Blender extension, modify the
scene, and choose Halo Wars DE or Halo Wars 2 as the export target.

The formats do not carry identical information: v6-only LOD distances cannot be
stored in a v4 section, and Hogan/legacy material conversion necessarily uses
the closest available texture and shader mapping. Geometry, topology, skinning,
and shared skeleton metadata are retained within the target packing precision.

## File Formats

### Container/Archive Formats

| Format  | Extension   | Description                                                                                                                 | Status         |
| ------- | ----------- | --------------------------------------------------------------------------------------------------------------------------- | -------------- |
| **ECF** | (container) | Ensemble Common Format - base container wrapping various file types. Uses big-endian byte order. Header magic: `0xDABA7737` | ✅ Implemented |
| **ERA** | `.era`      | Encrypted Resource Archive. ECF container with TEA encryption. Header ID: `0x17FDBA9C`                                      | ✅ Implemented |
| **PKG** | `.pkg`      | Packed File Archive (HW2 "capack"). Unencrypted archive with FNV-1a hashed filenames                                        | ✅ Implemented |

### Binary Data Formats

| Format  | Extension | ECF File ID  | Description                                                                                           | Status                         |
| ------- | --------- | ------------ | ----------------------------------------------------------------------------------------------------- | ------------------------------ |
| **UGX** | `.ugx`    | `0xAAC93746` | 3D model geometry (vertices, indices, materials, bones, bounding volumes)                             | ✅ HW1 v4 + HW2 v6 validated   |
| **UFX** | `.ufx`    | —            | HW2 UFXS container with D3D12 root signature and DXBC shader stages                                   | ✅ HW2 retail validated        |
| **UAX** | `.uax`    | `0xAAC93747` | Skeletal animation data (Granny format wrapper with duration, name, track groups)                     | ✅ HW1 DE validated            |
| **DDX** | `.ddx`    | `0x13CF5D01` | Texture format. DE uses standard DDS files; Xbox 360 uses ECF-wrapped format with deflate compression | ✅ Implemented                 |
| **XTD** | `.xtd`    | —            | Terrain height/visual data (chunks, lighting, ambient occlusion)                                      | ⚠️ Untested (see below)       |
| **XTT** | `.xtt`    | —            | Terrain texturing data (atlas, roads, foliage)                                                        | ✅ Implemented                 |

### XML-Based Formats

| Format  | Extension | Description                                          | Status         |
| ------- | --------- | ---------------------------------------------------- | -------------- |
| **XMB** | `.xmb`    | Binary XML format - compressed representation of XML | ✅ Implemented |
| **BDT** | (packed)  | Binary Data Tree - packed document format            | ✅ Implemented |

Most game data files are XML stored as XMB with specific schemas:

| Schema         | Extension        | Description                             |
| -------------- | ---------------- | --------------------------------------- |
| Visual         | `.vis.xmb`       | Model variants, attachments, animations |
| Particle       | `.pfx.xmb`       | Particle effect definitions             |
| Terrain Effect | `.tfx`           | Impact effects per terrain type         |
| Tactics        | `.tactics.xmb`   | Unit AI and combat behavior             |
| Physics        | `.physics.xmb`   | Physics simulation parameters           |
| Blueprint      | `.blueprint.xmb` | Object blueprints                       |
| Shape          | `.shp.xmb`       | Collision shapes                        |

### HW2 Compatibility Notes

The **UAX** crate has been checked against the Halo Wars DE loader in IDA and exhaustively tested against all 2,301 UAX files in a complete HW1 DE installation. HW2 UAX data has not yet received the same corpus-level verification. The **XTD** crate remains experimental and untested against real game data.

The **UGX** v4 and v6 paths are deliberately versioned side by side. Corpus
tests cover 2,468 HW1 v4 files (18,411 sections) and 15,632 HW2 v6 files
(92,726 sections), including index ranges and decoded vertex layouts. The v6
glTF path retains both retail color/skin byte orders and does not reinterpret
trailing vertex-stride payload as texture coordinates. The **UFX** parser is
checked against all 2,893 retail HW2 shaders; the corpus contains UFXS versions
5, 6, 7, and 9, 13,412 D3D12 vertex-input records, and vertex, hull, domain,
pixel, and compute programs.

All other formats (ECF, ERA, PKG, XMB, DDX, XTT, BDT) have been tested and confirmed working with both HW1 DE and HW2 data where applicable.

### Third-Party Formats

| Format      | Extension      | Description                                                  | Status             |
| ----------- | -------------- | ------------------------------------------------------------ | ------------------ |
| **GFX**     | `.gfx`         | Scaleform/Flash UI files                                     | ❌ Not implemented |
| **GR2**     | (internal)     | Granny 3D animation (RAD Game Tools) - loaded via Granny SDK | N/A (proprietary)  |
| **TTF/TTC** | `.ttf`, `.ttc` | TrueType fonts                                               | Standard format    |
| **TGA**     | `.tga`         | Targa images                                                 | Standard format    |

## File Format Details

### ECF (Ensemble Common Format)

The base container format used by most Halo Wars binary files. Structure:

- Header (32 bytes, big-endian)
- Chunk headers array
- Aligned chunk data

Key fields:

- Magic: `0xDABA7737`
- File ID: Identifies the file type (e.g., `0xAAC93746` for UGX)
- Chunks: Each chunk has a 64-bit ID, offset, size, and alignment

### ERA (Encrypted Resource Archive)

ECF container with TEA block encryption and optional Merkle One-Time Signature verification.

**Encryption**: TEA (Tiny Encryption Algorithm) in CBC mode with IV `0x15EF0AF334248FE2` and the archive password `3zDdptN*rV=qOkRbE*NAuWM6`.

**Digital Signatures**: ERA archives may contain a Merkle OTS signature block at offset 48 (after the ECF + ERA headers). The signature is verified against a public key (Merkle tree root hash) using a KISS PRNG-driven tree walk seeded by the header hash.

Known public keys:

| Game                          | Public Key (hex)                           |
| ----------------------------- | ------------------------------------------ |
| Halo Wars: Definitive Edition | `FD016BE719C21BD14F84CA9961A3F3CB18221F25` |

### UGX (Model Geometry)

ECF-based 3D model format containing:

- **Chunk 0x700**: Cached geometry data (header, sections, bones, accessories)
- **Chunk 0x701**: Index buffer
- **Chunk 0x702**: Vertex buffer
- **Chunk 0x703**: GRX data (Granny skeleton reference)
- **Chunk 0x704**: Material data
- **Chunk 0x705**: Bounding tree

HW1 cached data uses v4 152-byte sections with embedded vertex packers. HW2
uses v6 72-byte sections and external UFX vertex declarations; both layouts
remain independently readable and writable.

### UAX (Animation)

ECF-based animation format using RAD Game Tools' Granny SDK internally.

- **Chunk 0x700**: packed x64 Granny `file_info` object graph

The chunk begins directly with the 0x94-byte packed `file_info`; there is no separate Granny section header. All pointers in the graph are 64-bit little-endian offsets from the start of the chunk until the engine rebases them. The semantic reader and writer support all 19 curve formats defined by the game, along with vector, transform, text, and loop data.

Key animation fields:

- Name (partial path from original .max file)
- Duration (seconds)
- TimeStep (keyframe interval)
- Oversampling
- TrackGroupCount

### DDX (Texture)

DDX files come in two variants:

**Definitive Edition**: Standard DDS files (DirectDraw Surface) with `.ddx` extension. Magic: `0x20534444` ("DDS ").

**Xbox 360 (original)**: ECF container with:

- File ID: `0x13CF5D01`
- Header chunk: `0x1D8828C6ECAF45F2`
- Mip0 chunk: `0x3F74B8E87D2B44BF`
- MipChain chunk: `0x46F1FD3F394348B8`
- Deflate-compressed mip data

Supported formats: A8R8G8B8, A8B8G8R8, A8, DXT1/3/5, DXN, DXT5N, DXT5Y, DXT5H, A16B16G16R16F, and DXTQ variants.

### XTD/XTT (Terrain)

Terrain is split into two files:

- **XTD**: Height field, visual chunks, lighting, ambient occlusion
- **XTT**: Texture atlas, roads, foliage

## Architecture

All core crates (`ecf`, `era`, `xmb`, `bdt`, `xml`, `xtd`, `xtt`) are `no_std + alloc` compatible. They use slice-based APIs (`&[u8]` in, `Vec<u8>` out) with zero-copy header parsing via `zerocopy`. The `era` crate gates `std` streaming I/O and `rayon` parallelism behind opt-in features.

## Usage

### Reading an ERA archive

```rust
// Open and decrypt an ERA archive
let file = std::io::BufReader::new(std::fs::File::open("root.era")?);
let mut archive = era::Reader::from_encrypted(file, era::TeaKeys::default_archive_keys())?;

// List files
for entry in archive.iter() {
    println!("{}", entry.filename.as_deref().unwrap_or("<unnamed>"));
}

// Extract a file by name
if let Some(idx) = archive.find_by_name("data\\objects.xml.xmb") {
    let data = archive.read_entry(idx)?;
}
```

### Parsing XMB / XML

```rust
use xmb::{Reader, Writer, Document, Format};

// XMB -> Document -> XML string
let doc = Reader::read(&xmb_bytes)?;
let xml_string = doc.to_xml();

// XML string -> Document -> XMB bytes
let doc = Document::from_xml(&xml_string)?;
let xmb_bytes = Writer::write(&doc, Format::PC)?;
```

### Reading ECF containers

```rust
use ecf::Reader;

let ecf = Reader::new(&data)?;
for (i, chunk) in ecf.chunks().iter().enumerate() {
    let decompressed = ecf.chunk_data(i)?;
    println!("Chunk {:016X}: {} bytes", chunk.id, decompressed.len());
}
```

### Parsing DDX textures

```rust
use ddx::DdxTexture;

let texture = DdxTexture::from_bytes(&texture_data)?;
println!("{}x{} {:?}", texture.info.width, texture.info.height, texture.info.data_format);
```

### Reading terrain data (XTD / XTT)

```rust
use xtd::Reader as XtdReader;
use xtt::Reader as XttReader;

// Parse terrain displacement
let xtd = XtdReader::read(&xtd_bytes)?;
println!("XTD version: 0x{:04X}", xtd.header.version);

// Parse terrain textures
let xtt = XttReader::read(&xtt_bytes)?;
println!("Active textures: {}", xtt.header.num_active_textures);
```

## License

MIT
