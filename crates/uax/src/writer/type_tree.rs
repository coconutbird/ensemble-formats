//! Embedded Granny curve type definitions.
//!
//! Every member is a packed 44-byte descriptor. Curve variants point at one
//! of these trees so `GrannyRebasePointers` can traverse referenced arrays.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use super::string_table::StringTable;
use crate::{Error, Result};

const MEMBER_SIZE: usize = 44;
const INLINE: u32 = 1;
const REFERENCE_TO_ARRAY: u32 = 3;
const REAL32: u32 = 10;
const UINT8: u32 = 12;
const INT16: u32 = 15;
const UINT16: u32 = 16;

struct Member {
    kind: u32,
    name: &'static str,
    array_width: u32,
    reference_type: Option<Vec<Member>>,
}

fn member(member_type: u32, name: &'static str) -> Member {
    Member {
        kind: member_type,
        name,
        array_width: 0,
        reference_type: None,
    }
}

fn array_member(member_type: u32, name: &'static str, array_width: u32) -> Member {
    Member {
        kind: member_type,
        name,
        array_width,
        reference_type: None,
    }
}

fn reference_member(member_type: u32, name: &'static str, members: Vec<Member>) -> Member {
    Member {
        kind: member_type,
        name,
        array_width: 0,
        reference_type: Some(members),
    }
}

fn header(name: &'static str) -> Member {
    reference_member(
        INLINE,
        name,
        vec![member(UINT8, "Format"), member(UINT8, "Degree")],
    )
}

fn real32_array(name: &'static str) -> Member {
    reference_member(REFERENCE_TO_ARRAY, name, vec![member(REAL32, "Real32")])
}

fn uint16_array(name: &'static str) -> Member {
    reference_member(REFERENCE_TO_ARRAY, name, vec![member(UINT16, "UInt16")])
}

fn uint8_array(name: &'static str) -> Member {
    reference_member(REFERENCE_TO_ARRAY, name, vec![member(UINT8, "UInt8")])
}

fn d3_members(name: &'static str, element_type: u32, element_name: &'static str) -> Vec<Member> {
    vec![
        header(name),
        member(UINT16, "OneOverKnotScaleTrunc"),
        array_member(REAL32, "ControlScales", 3),
        array_member(REAL32, "ControlOffsets", 3),
        reference_member(
            REFERENCE_TO_ARRAY,
            "KnotsControls",
            vec![member(element_type, element_name)],
        ),
    ]
}

fn curve_members(format: u8) -> Option<Vec<Member>> {
    let members = match format {
        0 => vec![
            header("CurveDataHeader_DaKeyframes32f"),
            member(INT16, "Dimension"),
            real32_array("Controls"),
        ],
        1 => vec![
            header("CurveDataHeader_DaK32fC32f"),
            member(INT16, "Padding"),
            real32_array("Knots"),
            real32_array("Controls"),
        ],
        2 => vec![
            header("CurveDataHeader_DaIdentity"),
            member(INT16, "Dimension"),
        ],
        3 => vec![
            header("CurveDataHeader_DaConstant32f"),
            member(INT16, "Padding"),
            real32_array("Controls"),
        ],
        4 => vec![
            header("CurveDataHeader_D3Constant32f"),
            member(INT16, "Padding"),
            array_member(REAL32, "Controls", 3),
        ],
        5 => vec![
            header("CurveDataHeader_D4Constant32f"),
            member(INT16, "Padding"),
            array_member(REAL32, "Controls", 4),
        ],
        6 => vec![
            header("CurveDataHeader_DaK16uC16u"),
            member(UINT16, "OneOverKnotScaleTrunc"),
            real32_array("ControlScaleOffsets"),
            uint16_array("KnotsControls"),
        ],
        7 => vec![
            header("CurveDataHeader_DaK8uC8u"),
            member(UINT16, "OneOverKnotScaleTrunc"),
            real32_array("ControlScaleOffsets"),
            uint8_array("KnotsControls"),
        ],
        8 => vec![
            header("CurveDataHeader_D4nK16uC15u"),
            member(UINT16, "ScaleOffsetTableEntries"),
            member(REAL32, "OneOverKnotScale"),
            uint16_array("KnotsControls"),
        ],
        9 => vec![
            header("CurveDataHeader_D4nK8uC7u"),
            member(UINT16, "ScaleOffsetTableEntries"),
            member(REAL32, "OneOverKnotScale"),
            uint8_array("KnotsControls"),
        ],
        10 => d3_members("CurveDataHeader_D3K16uC16u", UINT16, "UInt16"),
        11 => d3_members("CurveDataHeader_D3K8uC8u", UINT8, "UInt8"),
        12 => vec![
            header("CurveDataHeader_D9I1K16uC16u"),
            member(UINT16, "OneOverKnotScaleTrunc"),
            member(REAL32, "ControlScale"),
            member(REAL32, "ControlOffset"),
            uint16_array("KnotsControls"),
        ],
        13 => d3_members("CurveDataHeader_D9I3K16uC16u", UINT16, "UInt16"),
        14 => vec![
            header("CurveDataHeader_D9I1K8uC8u"),
            member(UINT16, "OneOverKnotScaleTrunc"),
            member(REAL32, "ControlScale"),
            member(REAL32, "ControlOffset"),
            uint8_array("KnotsControls"),
        ],
        15 => d3_members("CurveDataHeader_D9I3K8uC8u", UINT8, "UInt8"),
        16 => vec![
            header("CurveDataHeader_D3I1K32fC32f"),
            member(UINT16, "Padding"),
            array_member(REAL32, "ControlScales", 3),
            array_member(REAL32, "ControlOffsets", 3),
            real32_array("KnotsControls"),
        ],
        17 => d3_members("CurveDataHeader_D3I1K16uC16u", UINT16, "UInt16"),
        18 => d3_members("CurveDataHeader_D3I1K8uC8u", UINT8, "UInt8"),
        _ => return None,
    };
    Some(members)
}

fn emit_members(
    output: &mut Vec<u8>,
    output_base: usize,
    strings: &mut StringTable,
    members: &[Member],
) -> Result<usize> {
    let start = output.len();
    let mut deferred = Vec::new();
    for item in members {
        output.extend_from_slice(&item.kind.to_le_bytes());
        let name_pointer = output.len();
        output.extend_from_slice(&0_u64.to_le_bytes());
        if !item.name.is_empty() {
            let fixup = output_base
                .checked_add(name_pointer)
                .ok_or(Error::SizeOverflow("curve type name pointer"))?;
            strings.add(fixup, String::from(item.name));
        }
        let reference_pointer = output.len();
        output.extend_from_slice(&0_u64.to_le_bytes());
        output.extend_from_slice(&item.array_width.to_le_bytes());
        output.extend_from_slice(&[0; 20]);
        if let Some(reference_type) = &item.reference_type {
            deferred.push((reference_pointer, reference_type.as_slice()));
        }
    }
    output.extend_from_slice(&[0; MEMBER_SIZE]);

    for (pointer_position, reference_type) in deferred {
        let reference_offset = emit_members(output, output_base, strings, reference_type)?;
        let absolute_offset = output_base
            .checked_add(reference_offset)
            .ok_or(Error::SizeOverflow("curve reference type pointer"))?;
        let pointer = u64::try_from(absolute_offset)
            .map_err(|_| Error::SizeOverflow("curve reference type pointer"))?;
        let pointer_end = pointer_position
            .checked_add(8)
            .ok_or(Error::SizeOverflow("curve reference type fixup"))?;
        output
            .get_mut(pointer_position..pointer_end)
            .ok_or(Error::SizeOverflow("curve reference type fixup"))?
            .copy_from_slice(&pointer.to_le_bytes());
    }
    Ok(start)
}

fn build_tree(base: usize, strings: &mut StringTable, members: &[Member]) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    emit_members(&mut output, base, strings, members)?;
    Ok(output)
}

/// Return the serialized byte size of one curve type tree.
pub(super) fn curve_type_tree_size(format: u8) -> Result<usize> {
    let members = curve_members(format).ok_or(Error::UnsupportedCurveFormat(format))?;
    let mut strings = StringTable::new();
    Ok(build_tree(0, &mut strings, &members)?.len())
}

/// Write one curve type tree at `offset`.
pub(super) fn write_curve_type_tree(
    output: &mut [u8],
    strings: &mut StringTable,
    format: u8,
    offset: usize,
) -> Result<()> {
    let members = curve_members(format).ok_or(Error::UnsupportedCurveFormat(format))?;
    let tree = build_tree(offset, strings, &members)?;
    let end = offset
        .checked_add(tree.len())
        .ok_or(Error::SizeOverflow("curve type tree"))?;
    let destination = output
        .get_mut(offset..end)
        .ok_or(Error::SizeOverflow("curve type tree buffer"))?;
    destination.copy_from_slice(&tree);
    Ok(())
}
