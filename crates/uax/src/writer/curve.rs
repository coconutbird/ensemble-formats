//! Curve object layout and serialization.

use alloc::vec;
use alloc::vec::Vec;

use super::{add, checked_i32, put_f32, put_i16, put_i32, put_u16, put_u64};
use crate::types::{CurveData, CurvePayload, curve_data_header};
use crate::{Error, Result};

/// Planned locations for one curve object and its referenced arrays.
pub(super) struct CurveLayout {
    pub(super) object_offset: usize,
    pub(super) array_offsets: Vec<usize>,
}

/// Validate a format/payload pair before layout planning.
pub(super) fn validate(curve: &CurveData) -> Result<()> {
    match curve.payload.format() {
        Some(format) if format == curve.format => Ok(()),
        None => Err(Error::UnsupportedCurveFormat(curve.format)),
        _ => Err(Error::CurvePayloadMismatch {
            format: curve.format,
            payload: curve.payload.name(),
        }),
    }
}

fn byte_size(count: usize, element_size: usize, field: &'static str) -> Result<usize> {
    count
        .checked_mul(element_size)
        .ok_or(Error::SizeOverflow(field))
}

fn referenced_array_size(count: usize, element_size: usize, field: &'static str) -> Result<usize> {
    checked_i32(count, field)?;
    byte_size(count, element_size, field)
}

fn u16_array_size(count: usize) -> Result<usize> {
    referenced_array_size(count, 2, "u16 curve array")
}

fn u8_array_size(count: usize) -> Result<usize> {
    referenced_array_size(count, 1, "u8 curve array")
}

fn element_offset(
    base: usize,
    index: usize,
    element_size: usize,
    field: &'static str,
) -> Result<usize> {
    let relative = index
        .checked_mul(element_size)
        .ok_or(Error::SizeOverflow(field))?;
    base.checked_add(relative).ok_or(Error::SizeOverflow(field))
}

fn array_offset(layout: &CurveLayout, index: usize, field: &'static str) -> Result<usize> {
    layout
        .array_offsets
        .get(index)
        .copied()
        .ok_or(Error::SizeOverflow(field))
}

/// Return object size and referenced-array byte sizes for one curve.
pub(super) fn sizes(curve: &CurveData) -> Result<(usize, Vec<usize>)> {
    validate(curve)?;
    let result = match &curve.payload {
        CurvePayload::DaKeyframes32f { controls, .. }
        | CurvePayload::DaConstant32f { controls, .. } => (
            16,
            vec![referenced_array_size(controls.len(), 4, "f32 curve array")?],
        ),
        CurvePayload::DaK32fC32f {
            knots, controls, ..
        } => (
            28,
            vec![
                referenced_array_size(knots.len(), 4, "curve knot array")?,
                referenced_array_size(controls.len(), 4, "curve control array")?,
            ],
        ),
        CurvePayload::Identity { .. } => (4, vec![]),
        CurvePayload::D3Constant32f { .. } => (16, vec![]),
        CurvePayload::D4Constant32f { .. } => (20, vec![]),
        CurvePayload::DaK16uC16u {
            control_scale_offsets,
            knots_controls,
            ..
        } => (
            28,
            vec![
                referenced_array_size(control_scale_offsets.len(), 4, "curve scale/offset array")?,
                u16_array_size(knots_controls.len())?,
            ],
        ),
        CurvePayload::DaK8uC8u {
            control_scale_offsets,
            knots_controls,
            ..
        } => (
            28,
            vec![
                referenced_array_size(control_scale_offsets.len(), 4, "curve scale/offset array")?,
                u8_array_size(knots_controls.len())?,
            ],
        ),
        CurvePayload::D4nK16uC15u { knots_controls, .. } => {
            (20, vec![u16_array_size(knots_controls.len())?])
        }
        CurvePayload::D4nK8uC7u { knots_controls, .. } => {
            (20, vec![u8_array_size(knots_controls.len())?])
        }
        CurvePayload::D3K16uC16u { knots_controls, .. }
        | CurvePayload::D9I3K16uC16u { knots_controls, .. }
        | CurvePayload::D3I1K16uC16u { knots_controls, .. } => {
            (40, vec![u16_array_size(knots_controls.len())?])
        }
        CurvePayload::D3K8uC8u { knots_controls, .. }
        | CurvePayload::D9I3K8uC8u { knots_controls, .. }
        | CurvePayload::D3I1K8uC8u { knots_controls, .. } => {
            (40, vec![u8_array_size(knots_controls.len())?])
        }
        CurvePayload::D9I1K16uC16u { knots_controls, .. } => {
            (24, vec![u16_array_size(knots_controls.len())?])
        }
        CurvePayload::D9I1K8uC8u { knots_controls, .. } => {
            (24, vec![u8_array_size(knots_controls.len())?])
        }
        CurvePayload::D3I1K32fC32f { knots_controls, .. } => (
            40,
            vec![referenced_array_size(
                knots_controls.len(),
                4,
                "f32 curve array",
            )?],
        ),
        CurvePayload::Unknown { .. } => {
            return Err(Error::UnsupportedCurveFormat(curve.format));
        }
    };
    Ok(result)
}

fn write_ref_header(
    output: &mut [u8],
    offset: usize,
    data_offset: usize,
    count: usize,
) -> Result<()> {
    put_i32(output, offset, checked_i32(count, "curve element count")?)?;
    let pointer = if count == 0 {
        0
    } else {
        u64::try_from(data_offset).map_err(|_| Error::SizeOverflow("curve array pointer"))?
    };
    put_u64(output, add(offset, 4, "curve reference pointer")?, pointer)
}

fn write_f32_ref_array(
    output: &mut [u8],
    header_offset: usize,
    data_offset: usize,
    values: &[f32],
) -> Result<()> {
    write_ref_header(output, header_offset, data_offset, values.len())?;
    for (index, value) in values.iter().enumerate() {
        put_f32(
            output,
            element_offset(data_offset, index, 4, "f32 curve array")?,
            *value,
        )?;
    }
    Ok(())
}

fn write_u16_ref_array(
    output: &mut [u8],
    header_offset: usize,
    data_offset: usize,
    values: &[u16],
) -> Result<()> {
    write_ref_header(output, header_offset, data_offset, values.len())?;
    for (index, value) in values.iter().enumerate() {
        put_u16(
            output,
            element_offset(data_offset, index, 2, "u16 curve array")?,
            *value,
        )?;
    }
    Ok(())
}

fn write_u8_ref_array(
    output: &mut [u8],
    header_offset: usize,
    data_offset: usize,
    values: &[u8],
) -> Result<()> {
    write_ref_header(output, header_offset, data_offset, values.len())?;
    if !values.is_empty() {
        let end = data_offset
            .checked_add(values.len())
            .ok_or(Error::SizeOverflow("u8 curve array"))?;
        output
            .get_mut(data_offset..end)
            .ok_or(Error::SizeOverflow("u8 curve array buffer"))?
            .copy_from_slice(values);
    }
    Ok(())
}

fn write_f32_values(output: &mut [u8], offset: usize, values: &[f32]) -> Result<()> {
    for (index, value) in values.iter().enumerate() {
        put_f32(
            output,
            element_offset(offset, index, 4, "f32 curve values")?,
            *value,
        )?;
    }
    Ok(())
}

fn write_d3_prefix(
    output: &mut [u8],
    offset: usize,
    knot_scale: u16,
    control_scales: &[f32; 3],
    control_offsets: &[f32; 3],
) -> Result<()> {
    put_u16(output, offset, knot_scale)?;
    write_f32_values(
        output,
        add(offset, 2, "curve control scales")?,
        control_scales,
    )?;
    write_f32_values(
        output,
        add(offset, 14, "curve control offsets")?,
        control_offsets,
    )
}

fn mismatch(curve: &CurveData) -> Error {
    Error::CurvePayloadMismatch {
        format: curve.format,
        payload: curve.payload.name(),
    }
}

fn write_low_formats(
    output: &mut [u8],
    layout: &CurveLayout,
    curve: &CurveData,
    payload: usize,
) -> Result<()> {
    match curve.format {
        0..=5 => write_basic_low_formats(output, layout, curve, payload),
        6..=9 => write_compressed_low_formats(output, layout, curve, payload),
        _ => Err(mismatch(curve)),
    }
}

fn write_basic_low_formats(
    output: &mut [u8],
    layout: &CurveLayout,
    curve: &CurveData,
    payload: usize,
) -> Result<()> {
    match &curve.payload {
        CurvePayload::DaKeyframes32f {
            dimension,
            controls,
        } => {
            put_i16(output, payload, *dimension)?;
            write_f32_ref_array(
                output,
                add(payload, 2, "DaKeyframes32f controls")?,
                array_offset(layout, 0, "DaKeyframes32f controls")?,
                controls,
            )?;
        }
        CurvePayload::DaK32fC32f {
            padding,
            knots,
            controls,
        } => {
            put_i16(output, payload, *padding)?;
            write_f32_ref_array(
                output,
                add(payload, 2, "DaK32fC32f knots")?,
                array_offset(layout, 0, "DaK32fC32f knots")?,
                knots,
            )?;
            write_f32_ref_array(
                output,
                add(payload, 14, "DaK32fC32f controls")?,
                array_offset(layout, 1, "DaK32fC32f controls")?,
                controls,
            )?;
        }
        CurvePayload::Identity { dimension } => put_i16(output, payload, *dimension)?,
        CurvePayload::DaConstant32f { padding, controls } => {
            put_i16(output, payload, *padding)?;
            write_f32_ref_array(
                output,
                add(payload, 2, "DaConstant32f controls")?,
                array_offset(layout, 0, "DaConstant32f controls")?,
                controls,
            )?;
        }
        CurvePayload::D3Constant32f { padding, controls } => {
            put_i16(output, payload, *padding)?;
            write_f32_values(output, add(payload, 2, "D3Constant32f controls")?, controls)?;
        }
        CurvePayload::D4Constant32f { padding, controls } => {
            put_i16(output, payload, *padding)?;
            write_f32_values(output, add(payload, 2, "D4Constant32f controls")?, controls)?;
        }
        _ => return Err(mismatch(curve)),
    }
    Ok(())
}

fn write_compressed_low_formats(
    output: &mut [u8],
    layout: &CurveLayout,
    curve: &CurveData,
    payload: usize,
) -> Result<()> {
    match &curve.payload {
        CurvePayload::DaK16uC16u {
            one_over_knot_scale_trunc,
            control_scale_offsets,
            knots_controls,
        } => {
            put_u16(output, payload, *one_over_knot_scale_trunc)?;
            write_f32_ref_array(
                output,
                add(payload, 2, "DaK16uC16u scale offsets")?,
                array_offset(layout, 0, "DaK16uC16u scale offsets")?,
                control_scale_offsets,
            )?;
            write_u16_ref_array(
                output,
                add(payload, 14, "DaK16uC16u knots/controls")?,
                array_offset(layout, 1, "DaK16uC16u knots/controls")?,
                knots_controls,
            )?;
        }
        CurvePayload::DaK8uC8u {
            one_over_knot_scale_trunc,
            control_scale_offsets,
            knots_controls,
        } => {
            put_u16(output, payload, *one_over_knot_scale_trunc)?;
            write_f32_ref_array(
                output,
                add(payload, 2, "DaK8uC8u scale offsets")?,
                array_offset(layout, 0, "DaK8uC8u scale offsets")?,
                control_scale_offsets,
            )?;
            write_u8_ref_array(
                output,
                add(payload, 14, "DaK8uC8u knots/controls")?,
                array_offset(layout, 1, "DaK8uC8u knots/controls")?,
                knots_controls,
            )?;
        }
        CurvePayload::D4nK16uC15u {
            scale_offset_table_entries,
            one_over_knot_scale,
            knots_controls,
        } => {
            put_u16(output, payload, *scale_offset_table_entries)?;
            put_f32(
                output,
                add(payload, 2, "D4nK16uC15u knot scale")?,
                *one_over_knot_scale,
            )?;
            write_u16_ref_array(
                output,
                add(payload, 6, "D4nK16uC15u knots/controls")?,
                array_offset(layout, 0, "D4nK16uC15u knots/controls")?,
                knots_controls,
            )?;
        }
        CurvePayload::D4nK8uC7u {
            scale_offset_table_entries,
            one_over_knot_scale,
            knots_controls,
        } => {
            put_u16(output, payload, *scale_offset_table_entries)?;
            put_f32(
                output,
                add(payload, 2, "D4nK8uC7u knot scale")?,
                *one_over_knot_scale,
            )?;
            write_u8_ref_array(
                output,
                add(payload, 6, "D4nK8uC7u knots/controls")?,
                array_offset(layout, 0, "D4nK8uC7u knots/controls")?,
                knots_controls,
            )?;
        }
        _ => return Err(mismatch(curve)),
    }
    Ok(())
}

fn write_high_formats(
    output: &mut [u8],
    layout: &CurveLayout,
    curve: &CurveData,
    payload: usize,
) -> Result<()> {
    match curve.format {
        10 | 11 | 13 | 15 | 17 | 18 => write_d3_formats(output, layout, curve, payload),
        12 | 14 | 16 => write_scalar_high_formats(output, layout, curve, payload),
        _ => Err(mismatch(curve)),
    }
}

fn write_d3_formats(
    output: &mut [u8],
    layout: &CurveLayout,
    curve: &CurveData,
    payload: usize,
) -> Result<()> {
    match &curve.payload {
        CurvePayload::D3K16uC16u {
            one_over_knot_scale_trunc,
            control_scales,
            control_offsets,
            knots_controls,
        }
        | CurvePayload::D9I3K16uC16u {
            one_over_knot_scale_trunc,
            control_scales,
            control_offsets,
            knots_controls,
        }
        | CurvePayload::D3I1K16uC16u {
            one_over_knot_scale_trunc,
            control_scales,
            control_offsets,
            knots_controls,
        } => {
            write_d3_prefix(
                output,
                payload,
                *one_over_knot_scale_trunc,
                control_scales,
                control_offsets,
            )?;
            write_u16_ref_array(
                output,
                add(payload, 26, "u16 curve knots/controls")?,
                array_offset(layout, 0, "u16 curve knots/controls")?,
                knots_controls,
            )?;
        }
        CurvePayload::D3K8uC8u {
            one_over_knot_scale_trunc,
            control_scales,
            control_offsets,
            knots_controls,
        }
        | CurvePayload::D9I3K8uC8u {
            one_over_knot_scale_trunc,
            control_scales,
            control_offsets,
            knots_controls,
        }
        | CurvePayload::D3I1K8uC8u {
            one_over_knot_scale_trunc,
            control_scales,
            control_offsets,
            knots_controls,
        } => {
            write_d3_prefix(
                output,
                payload,
                *one_over_knot_scale_trunc,
                control_scales,
                control_offsets,
            )?;
            write_u8_ref_array(
                output,
                add(payload, 26, "u8 curve knots/controls")?,
                array_offset(layout, 0, "u8 curve knots/controls")?,
                knots_controls,
            )?;
        }
        _ => return Err(mismatch(curve)),
    }
    Ok(())
}

fn write_scalar_high_formats(
    output: &mut [u8],
    layout: &CurveLayout,
    curve: &CurveData,
    payload: usize,
) -> Result<()> {
    match &curve.payload {
        CurvePayload::D9I1K16uC16u {
            one_over_knot_scale_trunc,
            control_scale,
            control_offset,
            knots_controls,
        } => {
            put_u16(output, payload, *one_over_knot_scale_trunc)?;
            put_f32(
                output,
                add(payload, 2, "D9I1K16uC16u control scale")?,
                *control_scale,
            )?;
            put_f32(
                output,
                add(payload, 6, "D9I1K16uC16u control offset")?,
                *control_offset,
            )?;
            write_u16_ref_array(
                output,
                add(payload, 10, "D9I1K16uC16u knots/controls")?,
                array_offset(layout, 0, "D9I1K16uC16u knots/controls")?,
                knots_controls,
            )?;
        }
        CurvePayload::D9I1K8uC8u {
            one_over_knot_scale_trunc,
            control_scale,
            control_offset,
            knots_controls,
        } => {
            put_u16(output, payload, *one_over_knot_scale_trunc)?;
            put_f32(
                output,
                add(payload, 2, "D9I1K8uC8u control scale")?,
                *control_scale,
            )?;
            put_f32(
                output,
                add(payload, 6, "D9I1K8uC8u control offset")?,
                *control_offset,
            )?;
            write_u8_ref_array(
                output,
                add(payload, 10, "D9I1K8uC8u knots/controls")?,
                array_offset(layout, 0, "D9I1K8uC8u knots/controls")?,
                knots_controls,
            )?;
        }
        CurvePayload::D3I1K32fC32f {
            padding,
            control_scales,
            control_offsets,
            knots_controls,
        } => {
            put_u16(output, payload, *padding)?;
            write_f32_values(
                output,
                add(payload, 2, "D3I1K32fC32f control scales")?,
                control_scales,
            )?;
            write_f32_values(
                output,
                add(payload, 14, "D3I1K32fC32f control offsets")?,
                control_offsets,
            )?;
            write_f32_ref_array(
                output,
                add(payload, 26, "D3I1K32fC32f knots/controls")?,
                array_offset(layout, 0, "D3I1K32fC32f knots/controls")?,
                knots_controls,
            )?;
        }
        _ => return Err(mismatch(curve)),
    }
    Ok(())
}

/// Serialize one planned curve object.
pub(super) fn write(output: &mut [u8], layout: &CurveLayout, curve: &CurveData) -> Result<()> {
    validate(curve)?;
    let object = layout.object_offset;
    *output
        .get_mut(object)
        .ok_or(Error::SizeOverflow("curve format byte"))? = curve.format;
    *output
        .get_mut(add(object, curve_data_header::DEGREE, "curve degree byte")?)
        .ok_or(Error::SizeOverflow("curve degree byte"))? = curve.degree;
    let payload = add(object, curve_data_header::SIZE, "curve payload")?;
    match curve.format {
        0..=9 => write_low_formats(output, layout, curve, payload),
        10..=18 => write_high_formats(output, layout, curve, payload),
        _ => Err(Error::UnsupportedCurveFormat(curve.format)),
    }
}
