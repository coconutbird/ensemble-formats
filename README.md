# ensemble-rs

A Rust library for parsing Halo Wars Definitive Edition file formats.

## Crates

| Crate     | Description                             |
| --------- | --------------------------------------- |
| `ecf`     | ECF container format parser             |
| `era`     | ERA archive format (encrypted ECF)      |
| `xmb`     | XMB binary XML parser                   |
| `ugx`     | UGX 3D model geometry parser            |
| `ddx`     | DDX/DDS texture format reader/writer    |
| `bdt`     | BDT binary data tree (packed documents) |
| `era-cli` | CLI tool for ERA archives               |
| `xmb-cli` | CLI tool for XMB files                  |
| `ugx-cli` | CLI tool for UGX models                 |

## File Formats

### Container/Archive Formats

| Format  | Extension   | Description                                                                                                                 | Status         |
| ------- | ----------- | --------------------------------------------------------------------------------------------------------------------------- | -------------- |
| **ECF** | (container) | Ensemble Common Format - base container wrapping various file types. Uses big-endian byte order. Header magic: `0xDABA7737` | ✅ Implemented |
| **ERA** | `.era`      | Encrypted Resource Archive. ECF container with TEA encryption. Header ID: `0x17FDBA9C`                                      | ✅ Implemented |

### Binary Data Formats

| Format  | Extension | ECF File ID  | Description                                                                                           | Status             |
| ------- | --------- | ------------ | ----------------------------------------------------------------------------------------------------- | ------------------ |
| **UGX** | `.ugx`    | `0xAAC93746` | 3D model geometry (vertices, indices, materials, bones, bounding volumes)                             | ✅ Implemented     |
| **UAX** | `.uax`    | `0xAAC93747` | Skeletal animation data                                                                               | ❌ Not implemented |
| **DDX** | `.ddx`    | `0x13CF5D01` | Texture format. DE uses standard DDS files; Xbox 360 uses ECF-wrapped format with deflate compression | ✅ Implemented     |
| **XTD** | `.xtd`    | —            | Terrain height/visual data (chunks, lighting, ambient occlusion)                                      | ❌ Not implemented |
| **XTT** | `.xtt`    | —            | Terrain texturing data (atlas, roads, foliage)                                                        | ❌ Not implemented |

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

### UGX (Model Geometry)

ECF-based 3D model format containing:

- **Chunk 0x700**: Cached geometry data (header, sections, bones, accessories)
- **Chunk 0x701**: Index buffer
- **Chunk 0x702**: Vertex buffer
- **Chunk 0x703**: GRX data (Granny skeleton reference)
- **Chunk 0x704**: Material data
- **Chunk 0x705**: Bounding tree

### UAX (Animation)

ECF-based animation format:

- **Chunk 0x700**: Animation data

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

## Usage

```rust
use era::EraArchive;
use xmb::Xmb;

// Open an ERA archive
let archive = EraArchive::open("root.era")?;

// List files
for entry in archive.entries() {
    println!("{}", entry.name());
}

// Extract and parse an XMB file
let data = archive.read("data/objects.xml.xmb")?;
let xmb = Xmb::parse(&data)?;
let xml = xmb.to_xml()?;

// Parse a DDX texture
use ddx::DdxTexture;
let texture_data = archive.read("art/system/default/defaultwhite.ddx")?;
let texture = DdxTexture::from_bytes(&texture_data)?;
println!("{}x{} {:?}", texture.info.width, texture.info.height, texture.info.data_format);

// Write texture as standard DDS file
let dds_bytes = texture.to_dds()?;
std::fs::write("output.dds", dds_bytes)?;
```

## License

MIT
