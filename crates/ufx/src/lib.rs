//! Zero-copy parser for the UFXS container format used by Halo Wars 2
//! compiled shaders.
//!
//! A `.ufx` file wraps a D3D12 root signature and up to six DXBC shader stages
//! inside a proprietary Ensemble header. The header also stores the D3D12
//! vertex-input declaration used with UGX buffers. Halo Wars 2 retail data
//! contains vertex, geometry, hull, domain, pixel, and compute stage slots.
//!
//! This crate uses [`d3dasm`] to parse the embedded DXBC containers.
//!
//! # Quick start
//!
//! ```ignore
//! let data = std::fs::read("shader.ufx")?;
//! let ufx = ufx::parse(&data)?;
//!
//! // Inspect vertex shader constant buffers
//! if let Some(vs) = &ufx.vertex_shader {
//!     if let Some(prog) = vs.program() {
//!         println!("VS: SM {}.{}", prog.major_version, prog.minor_version);
//!     }
//! }
//!
//! // Run semantic inference on the pixel shader
//! if let Some(ps) = ufx.pixel_shader() {
//!     if let Some(prog) = ps.program() {
//!         use ufx::cb_infer::{infer_cb_params, HOGAN_PS_SLOT, HOGAN_VS_SLOT};
//!         let params = infer_cb_params(prog, HOGAN_PS_SLOT, HOGAN_VS_SLOT);
//!         for p in &params {
//!             println!("  {} => {} [{}]", p.components, p.semantic, p.confidence);
//!         }
//!     }
//! }
//! ```

#![no_std]
extern crate alloc;

pub mod cb_infer;
mod vertex_input;

pub use vertex_input::{VertexFormat, VertexInput, VertexSemantic};

pub use d3dasm::{Shader, dxbc};

use alloc::vec::Vec;
use core::fmt;

/// Hogan ubershader feature flag bit positions.
///
/// The 64-bit hex value in permutation names (e.g. `HOGAN_STANDARD_00080000A8000960`)
/// encodes which features are active.  Each variant here is the **bit position**
/// (0-based) in that 64-bit mask.  Use [`HoganFlag::test`] to check a flag.
///
/// These bits control which texture samplers are bound, which constant-buffer
/// registers are populated, and which code paths the ubershader takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum HoganFlag {
    /// Height-blend terrain: adds `height_blend_range` + `color_gradient` to CB7.
    HeightBlend = 23,
    /// Extra texture layer (T4 UV scale); also enables emissive with bits 29/30.
    ExtraTextureLayer = 27,
    /// Roughness channel value override in CB8.
    RoughnessChannel = 28,
    /// Emissive sub-feature A (combined with [`ExtraTextureLayer`](Self::ExtraTextureLayer)).
    EmissiveSubA = 29,
    /// Emissive sub-feature B (combined with [`ExtraTextureLayer`](Self::ExtraTextureLayer)).
    EmissiveSubB = 30,
    /// Emissive map / emissive intensity + T4 UV scale.
    Emissive = 32,
    /// Scroll animation (requires [`Emissive`](Self::Emissive)).
    ScrollAnim = 35,
    /// Per-channel UV scale (T5).
    PerChannelUv = 38,
    /// Vertex animation variant A (CB7).
    VertexAnimA = 41,
    /// Vertex animation variant B (CB7).
    VertexAnimB = 44,
    /// Simplified texturing — suppresses `normal_intensity` in CB8.
    SimplifiedTexturing = 46,
    /// Full material override block (detail blend, roughness, spec color).
    MaterialOverride = 49,
    /// Reduced texturing — suppresses `normal_intensity` in CB8.
    ReducedTexturing = 53,
}

impl HoganFlag {
    /// Test whether this flag bit is set in a 64-bit feature mask.
    #[inline]
    #[must_use]
    pub fn test(self, flags: u64) -> bool {
        flags & (1u64 << (self as u32)) != 0
    }
}

const UFXS_MAGIC: &[u8; 4] = b"UFXS";

/// Bytes shared by every supported UFXS header before the version-specific data.
const UFXS_PREFIX_SIZE: usize = 0x0C;
const STAGE_COUNT: usize = 7;

/// A root-signature or programmable-shader slot in a UFXS stage table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShaderStage {
    /// D3D12 root signature (`RTS0` in a DXBC wrapper).
    RootSignature,
    /// Vertex shader.
    Vertex,
    /// Geometry shader.
    Geometry,
    /// Hull shader.
    Hull,
    /// Domain shader.
    Domain,
    /// Pixel shader.
    Pixel,
    /// Compute shader.
    Compute,
}

impl ShaderStage {
    /// Every UFXS table slot in on-disk order.
    pub const ALL: [Self; 7] = [
        Self::RootSignature,
        Self::Vertex,
        Self::Geometry,
        Self::Hull,
        Self::Domain,
        Self::Pixel,
        Self::Compute,
    ];

    const fn profile(self) -> Option<&'static str> {
        match self {
            Self::RootSignature => None,
            Self::Vertex => Some("vs"),
            Self::Geometry => Some("gs"),
            Self::Hull => Some("hs"),
            Self::Domain => Some("ds"),
            Self::Pixel => Some("ps"),
            Self::Compute => Some("cs"),
        }
    }
}

impl fmt::Display for ShaderStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::RootSignature => "root signature",
            Self::Vertex => "vertex shader",
            Self::Geometry => "geometry shader",
            Self::Hull => "hull shader",
            Self::Domain => "domain shader",
            Self::Pixel => "pixel shader",
            Self::Compute => "compute shader",
        })
    }
}

/// Byte range of one DXBC container recorded in the UFXS header.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ShaderRange {
    /// Byte offset from the beginning of the UFX file.
    pub offset: u32,
    /// Exact DXBC container size. A zero size marks an absent stage.
    pub size: u32,
}

impl ShaderRange {
    /// Whether the stage is present in the file.
    #[must_use]
    pub const fn is_present(self) -> bool {
        self.size != 0
    }
}

/// Version-decoded UFXS stage-table ranges.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StageRanges {
    /// Root-signature container range.
    pub root_signature: ShaderRange,
    /// Vertex-shader container range.
    pub vertex: ShaderRange,
    /// Geometry-shader container range.
    pub geometry: ShaderRange,
    /// Hull-shader container range.
    pub hull: ShaderRange,
    /// Domain-shader container range.
    pub domain: ShaderRange,
    /// Pixel-shader container range.
    pub pixel: ShaderRange,
    /// Compute-shader container range.
    pub compute: ShaderRange,
}

impl StageRanges {
    /// Return the range for `stage`.
    #[must_use]
    pub const fn get(self, stage: ShaderStage) -> ShaderRange {
        match stage {
            ShaderStage::RootSignature => self.root_signature,
            ShaderStage::Vertex => self.vertex,
            ShaderStage::Geometry => self.geometry,
            ShaderStage::Hull => self.hull,
            ShaderStage::Domain => self.domain,
            ShaderStage::Pixel => self.pixel,
            ShaderStage::Compute => self.compute,
        }
    }

    fn from_array(ranges: [ShaderRange; STAGE_COUNT]) -> Self {
        Self {
            root_signature: ranges[0],
            vertex: ranges[1],
            geometry: ranges[2],
            hull: ranges[3],
            domain: ranges[4],
            pixel: ranges[5],
            compute: ranges[6],
        }
    }
}

/// Errors that can occur when parsing a UFX file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The input is too short to contain the common UFXS prefix.
    TooShort {
        /// Length of the input data.
        len: usize,
    },
    /// The first four bytes are not `UFXS`.
    BadMagic {
        /// The bytes that were found.
        found: [u8; 4],
    },
    /// The version-specific stage table is truncated.
    TruncatedStageTable {
        /// Encoded UFXS version.
        version: u32,
        /// Length of the input data.
        len: usize,
        /// Minimum number of bytes needed for this version's table.
        needed: usize,
    },
    /// The version-specific vertex-input header is truncated.
    TruncatedVertexInputHeader {
        /// Length of the input data.
        len: usize,
        /// Minimum number of bytes needed for the header.
        needed: usize,
    },
    /// The UFXS version does not have a known HW2 stage-table layout.
    UnsupportedVersion {
        /// Unsupported encoded version.
        version: u32,
    },
    /// A present shader range is outside the file.
    InvalidShaderRange {
        /// Stage whose range is invalid.
        stage: ShaderStage,
        /// Encoded byte offset.
        offset: u32,
        /// Encoded byte size.
        size: u32,
        /// Length of the input data.
        len: usize,
    },
    /// A present stage range does not contain the declared DXBC container.
    MissingDxbc {
        /// Stage whose DXBC container is missing or malformed.
        stage: ShaderStage,
        /// Encoded byte offset.
        offset: u32,
        /// Encoded byte size.
        size: u32,
    },
    /// A stage-table slot contains a different shader program type.
    UnexpectedShaderStage {
        /// Stage named by the UFXS table slot.
        expected: ShaderStage,
        /// Program profile decoded from the DXBC container, if any.
        actual: Option<&'static str>,
    },
    /// The declared vertex-input record range is outside the file.
    InvalidVertexInputRange {
        /// Encoded byte offset.
        offset: u32,
        /// Number of 10-byte input records.
        count: u32,
        /// Length of the input data.
        len: usize,
    },
    /// The declaration exceeds the runtime's fixed input-element array.
    TooManyVertexInputs {
        /// Number of declared input records.
        count: u32,
    },
    /// A vertex-input record contains an unsupported field value.
    InvalidVertexInputValue {
        /// Zero-based record index.
        input_index: usize,
        /// Name of the invalid field.
        field: &'static str,
        /// Encoded value.
        value: u16,
    },
    /// Append-aligned elements overflow the 16-bit runtime slot stride.
    VertexInputStrideOverflow {
        /// Input slot whose accumulated stride overflowed.
        input_slot: u16,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort { len } => {
                write!(f, "input too short ({len} bytes, need {UFXS_PREFIX_SIZE})")
            }
            Self::BadMagic { found } => write!(
                f,
                "bad magic: expected UFXS, got {:?}",
                core::str::from_utf8(found).unwrap_or("????")
            ),
            Self::TruncatedStageTable {
                version,
                len,
                needed,
            } => write!(
                f,
                "truncated UFXS v{version} stage table ({len} bytes, need {needed})"
            ),
            Self::UnsupportedVersion { version } => {
                write!(f, "unsupported UFXS version {version}")
            }
            Self::TruncatedVertexInputHeader { len, needed } => write!(
                f,
                "truncated UFXS vertex-input header ({len} bytes, need {needed})"
            ),
            Self::InvalidShaderRange {
                stage,
                offset,
                size,
                len,
            } => write!(
                f,
                "invalid {stage} range: offset 0x{offset:X} + size 0x{size:X} exceeds {len} bytes"
            ),
            Self::MissingDxbc {
                stage,
                offset,
                size,
            } => write!(
                f,
                "{stage} range at 0x{offset:X} (0x{size:X} bytes) does not contain the declared DXBC container"
            ),
            Self::UnexpectedShaderStage { expected, actual } => match actual {
                Some(actual) => write!(
                    f,
                    "{expected} table slot contains a {actual} shader program"
                ),
                None => write!(
                    f,
                    "{expected} table slot does not contain a decodable shader program"
                ),
            },
            Self::InvalidVertexInputRange { offset, count, len } => write!(
                f,
                "invalid vertex-input range: {count} records at 0x{offset:X} exceed {len} bytes"
            ),
            Self::TooManyVertexInputs { count } => write!(
                f,
                "UFX declares {count} vertex inputs, but the HW2 runtime supports at most {}",
                vertex_input::MAX_VERTEX_INPUTS
            ),
            Self::InvalidVertexInputValue {
                input_index,
                field,
                value,
            } => write!(
                f,
                "vertex input {input_index} has unsupported {field} value {value}"
            ),
            Self::VertexInputStrideOverflow { input_slot } => {
                write!(
                    f,
                    "vertex input slot {input_slot} stride exceeds 65535 bytes"
                )
            }
        }
    }
}

/// A parsed UFXS container with metadata and structured shader stages.
#[derive(Debug)]
pub struct UfxFile<'a> {
    /// UFXS layout version. HW2 retail files use versions 5, 6, 7, and 9.
    pub version: u32,
    /// Permutation hash — matches the hash stored in UGX material data.
    pub hash: u32,
    /// Root Signature (RTS0 chunk inside a DXBC wrapper).
    pub root_signature: Option<Shader<'a>>,
    /// Vertex Shader.
    pub vertex_shader: Option<Shader<'a>>,
    /// Geometry shader.
    pub geometry_shader: Option<Shader<'a>>,
    /// Hull shader.
    pub hull_shader: Option<Shader<'a>>,
    /// Domain shader.
    pub domain_shader: Option<Shader<'a>>,
    /// Pixel shader. The vector contains at most one entry; it remains a vector
    /// for compatibility with the original UFX API.
    pub pixel_shaders: Vec<Shader<'a>>,
    /// Compute shader.
    pub compute_shader: Option<Shader<'a>>,
    /// Exact version-decoded ranges for every stage.
    pub stage_ranges: StageRanges,
    /// Vertex inputs used by HW2 to construct the D3D12 input layout.
    pub vertex_inputs: Vec<VertexInput>,
    /// Legacy view of the four intermediate table offsets (GS, HS, DS, PS).
    /// Prefer [`Self::stage_ranges`] for new code.
    pub ps_offsets: [u32; 4],
}

impl<'a> UfxFile<'a> {
    /// Return the pixel shader, if present.
    #[must_use]
    pub fn pixel_shader(&self) -> Option<&Shader<'a>> {
        self.pixel_shaders.first()
    }

    /// Return the parsed container for `stage`, if that stage is present.
    #[must_use]
    pub fn shader(&self, stage: ShaderStage) -> Option<&Shader<'a>> {
        match stage {
            ShaderStage::RootSignature => self.root_signature.as_ref(),
            ShaderStage::Vertex => self.vertex_shader.as_ref(),
            ShaderStage::Geometry => self.geometry_shader.as_ref(),
            ShaderStage::Hull => self.hull_shader.as_ref(),
            ShaderStage::Domain => self.domain_shader.as_ref(),
            ShaderStage::Pixel => self.pixel_shader(),
            ShaderStage::Compute => self.compute_shader.as_ref(),
        }
    }

    /// Append-aligned byte stride consumed by `input_slot`.
    #[must_use]
    pub fn vertex_stride(&self, input_slot: u16) -> Option<u16> {
        self.vertex_inputs
            .iter()
            .filter(|input| input.input_slot == input_slot)
            .filter_map(|input| input.end_offset())
            .max()
    }
}

/// Check whether `data` starts with the UFXS magic bytes.
#[must_use]
pub fn is_ufx(data: &[u8]) -> bool {
    data.len() >= 4 && &data[0..4] == UFXS_MAGIC
}

/// Parse `data` as a UFXS file.
///
/// Returns an [`Error`] if the magic bytes, version-specific stage table,
/// vertex declaration, shader ranges, or declared DXBC stage types are invalid.
///
/// # Errors
///
/// Returns [`Error::TooShort`] or [`Error::TruncatedStageTable`] for a
/// truncated header, [`Error::BadMagic`] when the input does not begin with
/// the UFXS signature, a vertex-input error for an invalid declaration, and a
/// stage-specific error for invalid DXBC data.
pub fn parse(data: &[u8]) -> Result<UfxFile<'_>, Error> {
    if data.len() < UFXS_PREFIX_SIZE {
        return Err(Error::TooShort { len: data.len() });
    }
    if &data[0..4] != UFXS_MAGIC {
        let mut found = [0u8; 4];
        found.copy_from_slice(&data[0..4]);
        return Err(Error::BadMagic { found });
    }

    let version = read_u32(data, 0x04).ok_or(Error::TooShort { len: data.len() })?;
    let hash = read_u32(data, 0x08).ok_or(Error::TooShort { len: data.len() })?;
    let table_offset = stage_table_offset(version)?;
    let table_size = STAGE_COUNT * 8;
    let table_end = table_offset
        .checked_add(table_size)
        .ok_or(Error::UnsupportedVersion { version })?;
    if data.len() < table_end {
        return Err(Error::TruncatedStageTable {
            version,
            len: data.len(),
            needed: table_end,
        });
    }

    let mut ranges = [ShaderRange::default(); STAGE_COUNT];
    for (index, range) in ranges.iter_mut().enumerate() {
        let entry_offset = table_offset + index * 8;
        range.offset = read_u32(data, entry_offset).ok_or(Error::TruncatedStageTable {
            version,
            len: data.len(),
            needed: table_end,
        })?;
        range.size = read_u32(data, entry_offset + 4).ok_or(Error::TruncatedStageTable {
            version,
            len: data.len(),
            needed: table_end,
        })?;
    }
    let stage_ranges = StageRanges::from_array(ranges);
    let vertex_inputs = vertex_input::parse(data, table_end)?;

    let root_signature = extract_shader(
        data,
        ShaderStage::RootSignature,
        stage_ranges.root_signature,
    )?;
    let vertex_shader = extract_shader(data, ShaderStage::Vertex, stage_ranges.vertex)?;
    let geometry_shader = extract_shader(data, ShaderStage::Geometry, stage_ranges.geometry)?;
    let hull_shader = extract_shader(data, ShaderStage::Hull, stage_ranges.hull)?;
    let domain_shader = extract_shader(data, ShaderStage::Domain, stage_ranges.domain)?;
    let pixel_shaders = extract_shader(data, ShaderStage::Pixel, stage_ranges.pixel)?
        .into_iter()
        .collect();
    let compute_shader = extract_shader(data, ShaderStage::Compute, stage_ranges.compute)?;
    let ps_offsets = [
        stage_ranges.geometry.offset,
        stage_ranges.hull.offset,
        stage_ranges.domain.offset,
        stage_ranges.pixel.offset,
    ];

    Ok(UfxFile {
        version,
        hash,
        root_signature,
        vertex_shader,
        geometry_shader,
        hull_shader,
        domain_shader,
        pixel_shaders,
        compute_shader,
        stage_ranges,
        vertex_inputs,
        ps_offsets,
    })
}

fn stage_table_offset(version: u32) -> Result<usize, Error> {
    match version {
        5 => Ok(0x7C),
        6 => Ok(0x0C),
        7..=9 => Ok(0x10),
        _ => Err(Error::UnsupportedVersion { version }),
    }
}

fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    Some(u32::from_le_bytes(data.get(offset..end)?.try_into().ok()?))
}

/// Extract and validate a DXBC blob from a declared stage range.
fn extract_shader(
    data: &[u8],
    stage: ShaderStage,
    range: ShaderRange,
) -> Result<Option<Shader<'_>>, Error> {
    if !range.is_present() {
        return Ok(None);
    }

    let offset = usize::try_from(range.offset).map_err(|_| Error::InvalidShaderRange {
        stage,
        offset: range.offset,
        size: range.size,
        len: data.len(),
    })?;
    let size = usize::try_from(range.size).map_err(|_| Error::InvalidShaderRange {
        stage,
        offset: range.offset,
        size: range.size,
        len: data.len(),
    })?;
    let end = offset.checked_add(size).ok_or(Error::InvalidShaderRange {
        stage,
        offset: range.offset,
        size: range.size,
        len: data.len(),
    })?;
    let region = data.get(offset..end).ok_or(Error::InvalidShaderRange {
        stage,
        offset: range.offset,
        size: range.size,
        len: data.len(),
    })?;
    let mut container = dxbc::scan_dxbc(region)
        .into_iter()
        .next()
        .filter(|container| container.offset_in_file == 0 && container.total_size == range.size)
        .ok_or(Error::MissingDxbc {
            stage,
            offset: range.offset,
            size: range.size,
        })?;
    container.offset_in_file =
        container
            .offset_in_file
            .checked_add(offset)
            .ok_or(Error::InvalidShaderRange {
                stage,
                offset: range.offset,
                size: range.size,
                len: data.len(),
            })?;
    let shader = Shader::from_container(container);

    if stage == ShaderStage::RootSignature {
        let has_root_signature = shader
            .container()
            .chunks
            .iter()
            .any(|chunk| chunk.fourcc == *b"RTS0");
        if !has_root_signature {
            return Err(Error::UnexpectedShaderStage {
                expected: stage,
                actual: shader.program().map(|program| program.shader_type),
            });
        }
    } else {
        let actual = shader.program().map(|program| program.shader_type);
        if actual != stage.profile() {
            return Err(Error::UnexpectedShaderStage {
                expected: stage,
                actual,
            });
        }
    }

    Ok(Some(shader))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use alloc::vec::Vec;

    fn container(stage: ShaderStage) -> Vec<u8> {
        let chunk = if stage == ShaderStage::RootSignature {
            dxbc::chunks::WritableChunk {
                fourcc: *b"RTS0",
                data: Vec::new(),
            }
        } else {
            let shader_type = match stage {
                ShaderStage::Pixel => 0,
                ShaderStage::Vertex => 1,
                ShaderStage::Geometry => 2,
                ShaderStage::Hull => 3,
                ShaderStage::Domain => 4,
                ShaderStage::Compute => 5,
                ShaderStage::RootSignature => unreachable!(),
            };
            let version_token = (shader_type << 16) | (5 << 4);
            let mut data = Vec::new();
            data.extend_from_slice(&u32::to_le_bytes(version_token));
            data.extend_from_slice(&2u32.to_le_bytes());
            dxbc::chunks::WritableChunk {
                fourcc: *b"SHEX",
                data,
            }
        };
        dxbc::build_dxbc(&[chunk])
    }

    fn write_u32(data: &mut [u8], offset: usize, value: u32) {
        data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn synthetic_ufx(version: u32, stages: &[ShaderStage]) -> Vec<u8> {
        let table_offset = stage_table_offset(version).unwrap();
        let table_end = table_offset + STAGE_COUNT * 8;
        let vertex_input_header_end = table_end + 8;
        let mut data = vec![0; vertex_input_header_end];
        data[0..4].copy_from_slice(UFXS_MAGIC);
        write_u32(&mut data, 4, version);
        write_u32(&mut data, 8, 0x1234_5678);
        write_u32(
            &mut data,
            table_end,
            u32::try_from(vertex_input_header_end).unwrap(),
        );

        let mut present_ranges = [None; STAGE_COUNT];
        for (index, stage) in ShaderStage::ALL.iter().copied().enumerate() {
            if stages.contains(&stage) {
                let bytes = container(stage);
                let offset = u32::try_from(data.len()).unwrap();
                let size = u32::try_from(bytes.len()).unwrap();
                data.extend_from_slice(&bytes);
                present_ranges[index] = Some(ShaderRange { offset, size });
            }
        }

        let mut next_offset = u32::try_from(data.len()).unwrap();
        for index in (0..STAGE_COUNT).rev() {
            let range = present_ranges[index].unwrap_or(ShaderRange {
                offset: next_offset,
                size: 0,
            });
            if range.is_present() {
                next_offset = range.offset;
            }
            let entry = table_offset + index * 8;
            write_u32(&mut data, entry, range.offset);
            write_u32(&mut data, entry + 4, range.size);
        }
        data
    }

    #[test]
    fn all_retail_stage_table_layouts_decode() {
        for version in [5, 6, 7, 8, 9] {
            let data = synthetic_ufx(version, &ShaderStage::ALL);
            let file = parse(&data).unwrap();
            assert_eq!(file.version, version);
            assert_eq!(file.hash, 0x1234_5678);
            for stage in ShaderStage::ALL {
                assert!(file.stage_ranges.get(stage).is_present());
                assert!(file.shader(stage).is_some());
            }
            assert_eq!(file.pixel_shaders.len(), 1);
        }
    }

    #[test]
    fn absent_stages_remain_absent() {
        let data = synthetic_ufx(9, &[ShaderStage::RootSignature, ShaderStage::Compute]);
        let file = parse(&data).unwrap();
        assert!(file.root_signature.is_some());
        assert!(file.compute_shader.is_some());
        assert!(file.vertex_shader.is_none());
        assert!(file.pixel_shader().is_none());
    }

    #[test]
    fn decodes_vertex_inputs_and_runtime_offsets() {
        let mut data = synthetic_ufx(9, &[ShaderStage::RootSignature, ShaderStage::Vertex]);
        let table_end = stage_table_offset(9).unwrap() + STAGE_COUNT * 8;
        let input_offset = data.len();
        let records = [
            [3, 0, 6, 0, 0, 0, 0, 0, 0, 0],
            [1, 0, 5, 0, 4, 0, 0, 0, 0, 0],
            [1, 0, 1, 0, 5, 0, 0, 0, 0, 0],
            [3, 0, 14, 0, 8, 0, 0, 0, 0, 0],
        ];
        for record in records {
            data.extend_from_slice(&record);
        }
        write_u32(&mut data, table_end, u32::try_from(input_offset).unwrap());
        write_u32(&mut data, table_end + 4, 4);

        let file = parse(&data).unwrap();
        assert_eq!(file.vertex_inputs.len(), 4);
        assert_eq!(file.vertex_inputs[0].semantic, VertexSemantic::Position);
        assert_eq!(
            file.vertex_inputs[0].format,
            VertexFormat::R16G16B16A16Float
        );
        assert_eq!(file.vertex_inputs[1].byte_offset, 8);
        assert_eq!(file.vertex_inputs[2].byte_offset, 12);
        assert_eq!(file.vertex_inputs[3].semantic, VertexSemantic::Color);
        assert_eq!(file.vertex_inputs[3].format, VertexFormat::B8G8R8A8Unorm);
        assert_eq!(file.vertex_inputs[3].byte_offset, 20);
        assert_eq!(file.vertex_stride(0), Some(24));
    }

    #[test]
    fn rejects_vertex_inputs_beyond_runtime_capacity() {
        let mut data = synthetic_ufx(9, &[]);
        let table_end = stage_table_offset(9).unwrap() + STAGE_COUNT * 8;
        write_u32(&mut data, table_end + 4, 17);

        assert!(matches!(
            parse(&data),
            Err(Error::TooManyVertexInputs { count: 17 })
        ));
    }

    #[test]
    fn rejects_input_slots_beyond_runtime_capacity() {
        let mut data = synthetic_ufx(9, &[]);
        let table_end = stage_table_offset(9).unwrap() + STAGE_COUNT * 8;
        let input_offset = data.len();
        data.extend_from_slice(&[0, 0, 0, 0, 0, 0, 8, 0, 0, 0]);
        write_u32(&mut data, table_end, u32::try_from(input_offset).unwrap());
        write_u32(&mut data, table_end + 4, 1);

        assert!(matches!(
            parse(&data),
            Err(Error::InvalidVertexInputValue {
                input_index: 0,
                field: "input slot",
                value: 8,
            })
        ));
    }

    #[test]
    fn rejects_truncated_version_specific_table() {
        let mut data = vec![0; 0x48];
        data[0..4].copy_from_slice(UFXS_MAGIC);
        write_u32(&mut data, 4, 5);
        assert!(matches!(
            parse(&data),
            Err(Error::TruncatedStageTable {
                version: 5,
                needed: 0xB4,
                ..
            })
        ));
    }

    #[test]
    fn rejects_program_in_the_wrong_stage_slot() {
        let mut data = synthetic_ufx(9, &[ShaderStage::RootSignature, ShaderStage::Vertex]);
        let table_offset = stage_table_offset(9).unwrap();
        let vertex_offset = usize::try_from(read_u32(&data, table_offset + 8).unwrap()).unwrap();
        let shex_token_offset = vertex_offset + 0x2C;
        write_u32(&mut data, shex_token_offset, 5 << 4);
        assert!(matches!(
            parse(&data),
            Err(Error::UnexpectedShaderStage {
                expected: ShaderStage::Vertex,
                actual: Some("ps")
            })
        ));
    }
}
