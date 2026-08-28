//! Granny type tree emission for UAX curve data and `file_info` schema.
//! Each type member is 44 bytes on disk. Terminated by a 44-byte zero entry.
//!
//! Two-pass design: build into a temp buffer to measure size, then copy
//! into the final chunk at a known offset. All internal pointers in the
//! temp buffer are relative to the temp buffer start and get rebased on copy.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use super::string_table::StringTable;

const STRIDE: usize = 44;

// Granny member type IDs
const I: u32 = 1; // Inline
const R: u32 = 2; // Reference
const RA: u32 = 3; // ReferenceToArray
const AR: u32 = 4; // ArrayOfReferences
const VR: u32 = 5; // VariantReference
const S: u32 = 8; // String
const TX: u32 = 9; // Transform
const F32: u32 = 10; // Real32
const U8: u32 = 12; // UInt8
const I16: u32 = 15; // Int16
const U16: u32 = 16; // UInt16
const I32: u32 = 19; // Int32

struct M {
    ty: u32,
    name: &'static str,
    aw: u32,
    sub: Option<Vec<M>>,
}

fn m(ty: u32, n: &'static str) -> M {
    M {
        ty,
        name: n,
        aw: 0,
        sub: None,
    }
}
fn ma(ty: u32, n: &'static str, aw: u32) -> M {
    M {
        ty,
        name: n,
        aw,
        sub: None,
    }
}
fn mr(ty: u32, n: &'static str, s: Vec<M>) -> M {
    M {
        ty,
        name: n,
        aw: 0,
        sub: Some(s),
    }
}

/// Emit type members into `tmp`. Returns offset (relative to tmp start) where this def begins.
/// String name pointers are recorded as fixups keyed by `(base + offset_within_tmp)`.
fn emit(tmp: &mut Vec<u8>, base: usize, st: &mut StringTable, ms: &[M]) -> usize {
    let start = tmp.len();
    let mut deferred: Vec<(usize, &[M])> = Vec::new();
    for member in ms {
        tmp.extend_from_slice(&member.ty.to_le_bytes()); // +0  MemberType
        let np = tmp.len();
        tmp.extend_from_slice(&0u64.to_le_bytes()); // +4  NamePtr (patched)
        if !member.name.is_empty() {
            st.add(base + np, String::from(member.name));
        }
        let rp = tmp.len();
        tmp.extend_from_slice(&0u64.to_le_bytes()); // +12 ReferenceTypePtr
        tmp.extend_from_slice(&member.aw.to_le_bytes()); // +20 ArrayWidth
        tmp.extend_from_slice(&[0u8; 20]); // +24 Extra[3]+Unused[2]
        if let Some(ref s) = member.sub
            && !s.is_empty()
        {
            deferred.push((rp, s.as_slice()));
        }
    }
    tmp.extend_from_slice(&[0u8; STRIDE]); // terminator
    // Emit nested types and patch reference pointers (relative to tmp start → rebased to base)
    for (rp, nested) in deferred {
        let nested_off = emit(tmp, base, st, nested);
        let abs = (base + nested_off) as u64;
        tmp[rp..rp + 8].copy_from_slice(&abs.to_le_bytes());
    }
    start
}

/// Build a type tree into a temporary buffer. Returns the temp bytes.
fn build_tree(base: usize, st: &mut StringTable, ms: &[M]) -> Vec<u8> {
    let mut tmp = Vec::new();
    emit(&mut tmp, base, st, ms);
    tmp
}

fn hdr(n: &'static str) -> M {
    mr(I, n, vec![m(U8, "Format"), m(U8, "Degree")])
}

fn curve_ms(fmt: u8) -> Vec<M> {
    match fmt {
        0 => vec![
            hdr("CurveDataHeader_DaKeyframes32f"),
            m(I16, "Dimension"),
            mr(RA, "Controls", vec![m(F32, "Real32")]),
        ],
        1 => vec![
            hdr("CurveDataHeader_DaK32fC32f"),
            m(I16, "Padding"),
            mr(RA, "Knots", vec![m(F32, "Real32")]),
            mr(RA, "Controls", vec![m(F32, "Real32")]),
        ],
        2 => vec![hdr("CurveDataHeader_DaIdentity"), m(I16, "Dimension")],
        3 => vec![
            hdr("CurveDataHeader_DaConstant32f"),
            m(I16, "Padding"),
            mr(RA, "Controls", vec![m(F32, "Real32")]),
        ],
        4 => vec![
            hdr("CurveDataHeader_D3Constant32f"),
            m(I16, "Padding"),
            ma(F32, "Controls", 3),
        ],
        5 => vec![
            hdr("CurveDataHeader_D4Constant32f"),
            m(I16, "Padding"),
            ma(F32, "Controls", 4),
        ],
        8 => vec![
            hdr("CurveDataHeader_D4nK16uC15u"),
            m(U16, "ScaleOffsetTableEntries"),
            m(F32, "OneOverKnotScale"),
            mr(RA, "KnotsControls", vec![m(U16, "UInt16")]),
        ],
        9 => vec![
            hdr("CurveDataHeader_D4nK8uC7u"),
            m(U16, "ScaleOffsetTableEntries"),
            m(F32, "OneOverKnotScale"),
            mr(RA, "KnotsControls", vec![m(U8, "UInt8")]),
        ],
        10 => vec![
            hdr("CurveDataHeader_D3K16uC16u"),
            m(U16, "OneOverKnotScaleTrunc"),
            ma(F32, "ControlScales", 3),
            ma(F32, "ControlOffsets", 3),
            mr(RA, "KnotsControls", vec![m(U16, "UInt16")]),
        ],
        11 => vec![
            hdr("CurveDataHeader_D3K8uC8u"),
            m(U16, "OneOverKnotScaleTrunc"),
            ma(F32, "ControlScales", 3),
            ma(F32, "ControlOffsets", 3),
            mr(RA, "KnotsControls", vec![m(U8, "UInt8")]),
        ],
        17 => vec![
            hdr("CurveDataHeader_D3I1K16uC16u"),
            m(U16, "OneOverKnotScaleTrunc"),
            ma(F32, "ControlScales", 3),
            ma(F32, "ControlOffsets", 3),
            mr(RA, "KnotsControls", vec![m(U16, "UInt16")]),
        ],
        18 => vec![
            hdr("CurveDataHeader_D3I1K8uC8u"),
            m(U16, "OneOverKnotScaleTrunc"),
            ma(F32, "ControlScales", 3),
            ma(F32, "ControlOffsets", 3),
            mr(RA, "KnotsControls", vec![m(U8, "UInt8")]),
        ],
        _ => vec![hdr("CurveDataHeader_Unknown")],
    }
}

fn fi_ms() -> Vec<M> {
    vec![
        mr(
            R,
            "ArtToolInfo",
            vec![
                m(S, "FromArtToolName"),
                m(I32, "ArtToolMajorRevision"),
                m(I32, "ArtToolMinorRevision"),
                m(I32, "ArtToolPointerSize"),
                m(F32, "UnitsPerMeter"),
                ma(F32, "Origin", 3),
                ma(F32, "RightVector", 3),
                ma(F32, "UpVector", 3),
                ma(F32, "BackVector", 3),
                m(VR, "ExtendedData"),
            ],
        ),
        mr(
            R,
            "ExporterInfo",
            vec![
                m(S, "ExporterName"),
                m(I32, "ExporterMajorRevision"),
                m(I32, "ExporterMinorRevision"),
                m(I32, "ExporterCustomization"),
                m(I32, "ExporterBuildNumber"),
                m(VR, "ExtendedData"),
            ],
        ),
        m(S, "FromFileName"),
        mr(
            AR,
            "Textures",
            vec![m(S, "FromFileName"), m(VR, "ExtendedData")],
        ),
        mr(AR, "Materials", vec![m(S, "Name"), m(VR, "ExtendedData")]),
        mr(AR, "Skeletons", vec![m(S, "Name"), m(VR, "ExtendedData")]),
        mr(AR, "VertexDatas", vec![m(VR, "ExtendedData")]),
        mr(AR, "TriTopologies", vec![m(VR, "ExtendedData")]),
        mr(AR, "Meshes", vec![m(S, "Name"), m(VR, "ExtendedData")]),
        mr(AR, "Models", vec![m(S, "Name"), m(VR, "ExtendedData")]),
        mr(
            AR,
            "TrackGroups",
            vec![
                m(S, "Name"),
                mr(RA, "VectorTracks", vec![]),
                mr(
                    RA,
                    "TransformTracks",
                    vec![
                        m(S, "Name"),
                        m(I32, "Flags"),
                        m(VR, "OrientationCurve"),
                        m(VR, "PositionCurve"),
                        m(VR, "ScaleShearCurve"),
                    ],
                ),
                mr(RA, "TransformLODErrors", vec![m(F32, "Real32")]),
                mr(RA, "TextTracks", vec![]),
                m(TX, "InitialPlacement"),
                m(I32, "AccumulationFlags"),
                ma(F32, "LoopTranslation", 3),
                mr(R, "PeriodicLoop", vec![]),
                m(VR, "ExtendedData"),
            ],
        ),
        mr(
            AR,
            "Animations",
            vec![
                m(S, "Name"),
                m(F32, "Duration"),
                m(F32, "TimeStep"),
                m(F32, "Oversampling"),
                mr(AR, "TrackGroups", vec![]),
                m(I32, "DefaultLoopCount"),
                m(I32, "Flags"),
                m(VR, "ExtendedData"),
            ],
        ),
        m(VR, "ExtendedData"),
    ]
}

// ============================================================================
// Public API
// ============================================================================

/// Compute the byte size of a curve format's type tree.
pub(super) fn curve_type_tree_size(fmt: u8) -> usize {
    let ms = curve_ms(fmt);
    let mut st = StringTable::new();
    build_tree(0, &mut st, &ms).len()
}

/// Approximate size of the `file_info` type tree.
pub(super) const FILE_INFO_TYPE_TREE_SIZE: usize = 8000; // generous upper bound; actual copy truncates

/// Write a curve type tree at `offset` in `buf`. `buf` must be large enough.
pub(super) fn write_curve_type_tree(
    buf: &mut [u8],
    strings: &mut StringTable,
    fmt: u8,
    offset: usize,
) {
    let ms = curve_ms(fmt);
    let tree = build_tree(offset, strings, &ms);
    buf[offset..offset + tree.len()].copy_from_slice(&tree);
}

/// Write the `file_info` type tree at `offset` in `buf` and return the actual size written.
pub(super) fn write_file_info_type_tree(
    buf: &mut Vec<u8>,
    strings: &mut StringTable,
    offset: usize,
) {
    let ms = fi_ms();
    let tree = build_tree(offset, strings, &ms);
    let end = offset + tree.len();
    if end > buf.len() {
        buf.resize(end, 0);
    }
    buf[offset..end].copy_from_slice(&tree);
}
