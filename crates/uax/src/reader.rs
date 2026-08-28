//! Strict UAX reader for the packed x64 Granny animation graph.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::types::{
    Animation, CurveData, CurvePayload, PeriodicLoop, TextTrack, TextTrackEntry, TrackGroup,
    Transform, TransformTrack, VectorTrack, animation, curve_data_header, curve_type_name, curve2,
    file_info, periodic_loop, text_track, text_track_entry, track_group, transform,
    transform_track, variant, vector_track,
};
use crate::{Error, Result, UAX_CHUNK_ID, UAX_FILE_ID, UAX_FROM_FILENAME};
use ecf::Reader as EcfReader;

type D3U16Payload = (u16, [f32; 3], [f32; 3], Vec<u16>);
type D3U8Payload = (u16, [f32; 3], [f32; 3], Vec<u8>);

/// UAX file reader.
pub struct Reader;

impl Reader {
    /// Read the single animation from UAX file bytes.
    ///
    /// The engine's UAX format contains one animation. Files with malformed
    /// pointers, truncated arrays, mismatched curve descriptors, non-empty
    /// arbitrary extended-data variants, or unsupported represented structures
    /// are rejected instead of being partially defaulted. Ancillary Granny
    /// model and skeleton roots are intentionally ignored; use [`crate::UaxFile`]
    /// for a byte-preserving edit.
    ///
    /// # Errors
    ///
    /// Returns an error if the ECF container or Granny object graph is invalid,
    /// or if the file contains data the semantic animation API cannot preserve.
    pub fn read(data: &[u8]) -> Result<Animation> {
        let ecf = EcfReader::new(data)?;
        let file_id = ecf.header().id;
        if file_id != UAX_FILE_ID {
            return Err(Error::InvalidFileId(file_id));
        }

        let chunk_index = ecf
            .chunks()
            .iter()
            .position(|chunk| chunk.id == UAX_CHUNK_ID)
            .ok_or(Error::ChunkNotFound)?;
        let chunk = ecf.chunk_data(chunk_index)?;
        if chunk.len() < file_info::SIZE {
            return Err(Error::ChunkTooSmall(chunk.len(), file_info::SIZE));
        }

        ChunkReader::new(&chunk).parse_file_info()
    }
}

struct ChunkReader<'a> {
    data: &'a [u8],
}

impl<'a> ChunkReader<'a> {
    const TYPE_MEMBER_SIZE: usize = 44;

    fn new(data: &'a [u8]) -> Self {
        Self { data }
    }

    fn parse_file_info(&self) -> Result<Animation> {
        let from_file_name =
            self.required_string_field(0, file_info::FROM_FILE_NAME_PTR, "FromFileName")?;
        if !from_file_name.eq_ignore_ascii_case(UAX_FROM_FILENAME) {
            return Err(Error::InvalidFromFileName(from_file_name));
        }
        self.require_empty_variant(file_info::EXTENDED_DATA, "file_info")?;

        let root_groups = self.pointer_array(
            file_info::TRACK_GROUP_COUNT,
            file_info::TRACK_GROUPS_PTR,
            "file_info track groups",
        )?;
        let animation_count = self.i32_at(file_info::ANIMATION_COUNT, "animation count")?;
        if animation_count < 0 {
            return Err(Error::InvalidCount("animation count", animation_count));
        }
        if animation_count == 0 {
            return Err(Error::NoAnimations);
        }
        if animation_count != 1 {
            return Err(Error::UnsupportedAnimationCount(animation_count));
        }
        let (_, animation_array) = self.array(
            file_info::ANIMATION_COUNT,
            file_info::ANIMATIONS_PTR,
            8,
            "animation pointer array",
        )?;
        let animation_offset = self.required_ptr_at(animation_array, "first animation")?;
        self.range(animation_offset, animation::SIZE, "animation")?;
        self.require_empty_variant(
            self.relative(
                animation_offset,
                animation::EXTENDED_DATA,
                "animation extended data",
            )?,
            "animation",
        )?;

        let referenced_groups = self.pointer_array(
            self.relative(
                animation_offset,
                animation::TRACK_GROUP_COUNT,
                "animation track-group count",
            )?,
            self.relative(
                animation_offset,
                animation::TRACK_GROUPS_PTR,
                "animation track-group pointer",
            )?,
            "animation track groups",
        )?;
        if referenced_groups != root_groups {
            return Err(Error::TrackGroupReferenceMismatch);
        }

        let mut track_groups = Vec::with_capacity(referenced_groups.len());
        for offset in referenced_groups {
            track_groups.push(self.parse_track_group(offset)?);
        }

        Ok(Animation {
            name: self.optional_string_field(
                animation_offset,
                animation::NAME_PTR,
                "animation name",
            )?,
            duration: self.f32_field(
                animation_offset,
                animation::DURATION,
                "animation duration",
            )?,
            time_step: self.f32_field(
                animation_offset,
                animation::TIME_STEP,
                "animation time step",
            )?,
            oversampling: self.f32_field(
                animation_offset,
                animation::OVERSAMPLING,
                "animation oversampling",
            )?,
            track_groups,
            default_loop_count: self.i32_field(
                animation_offset,
                animation::DEFAULT_LOOP_COUNT,
                "animation default loop count",
            )?,
            flags: self.u32_field(animation_offset, animation::FLAGS, "animation flags")?,
        })
    }

    fn parse_track_group(&self, offset: usize) -> Result<TrackGroup> {
        self.range(offset, track_group::SIZE, "track group")?;
        self.require_empty_variant(
            self.relative(
                offset,
                track_group::EXTENDED_DATA,
                "track-group extended data",
            )?,
            "track group",
        )?;

        let vector_tracks = self.parse_vector_tracks(offset)?;
        let transform_tracks = self.parse_transform_tracks(offset)?;
        let transform_lod_errors = self.parse_transform_lod_errors(offset)?;
        let text_tracks = self.parse_text_tracks(offset)?;

        let periodic_loop_offset = self.relative(
            offset,
            track_group::PERIODIC_LOOP_PTR,
            "periodic-loop pointer",
        )?;
        let parsed_periodic_loop =
            match self.optional_ptr_at(periodic_loop_offset, "periodic loop")? {
                Some(pointer) => Some(self.parse_periodic_loop(pointer)?),
                None => None,
            };

        Ok(TrackGroup {
            name: self.optional_string_field(offset, track_group::NAME_PTR, "track-group name")?,
            vector_tracks,
            transform_tracks,
            transform_lod_errors,
            text_tracks,
            initial_placement: self.parse_transform(self.relative(
                offset,
                track_group::INITIAL_PLACEMENT,
                "initial placement",
            )?)?,
            flags: self.u32_field(offset, track_group::FLAGS, "accumulation flags")?,
            loop_translation: self.f32_array::<3>(
                self.relative(offset, track_group::LOOP_TRANSLATION, "loop translation")?,
                "loop translation",
            )?,
            periodic_loop: parsed_periodic_loop,
        })
    }

    fn parse_vector_tracks(&self, group: usize) -> Result<Vec<VectorTrack>> {
        let (count, array) = self.array(
            self.relative(group, track_group::VECTOR_TRACK_COUNT, "vector-track count")?,
            self.relative(
                group,
                track_group::VECTOR_TRACKS_PTR,
                "vector-track pointer",
            )?,
            vector_track::SIZE,
            "vector tracks",
        )?;
        let mut tracks = Vec::with_capacity(count);
        for index in 0..count {
            let offset = self.element_offset(array, index, vector_track::SIZE, "vector track")?;
            tracks.push(self.parse_vector_track(offset)?);
        }
        Ok(tracks)
    }

    fn parse_transform_tracks(&self, group: usize) -> Result<Vec<TransformTrack>> {
        let (count, array) = self.array(
            self.relative(
                group,
                track_group::TRANSFORM_TRACK_COUNT,
                "transform-track count",
            )?,
            self.relative(
                group,
                track_group::TRANSFORM_TRACKS_PTR,
                "transform-track pointer",
            )?,
            transform_track::SIZE,
            "transform tracks",
        )?;
        let mut tracks = Vec::with_capacity(count);
        for index in 0..count {
            let offset =
                self.element_offset(array, index, transform_track::SIZE, "transform track")?;
            tracks.push(self.parse_transform_track(offset)?);
        }
        Ok(tracks)
    }

    fn parse_transform_lod_errors(&self, group: usize) -> Result<Vec<f32>> {
        let (count, array) = self.array(
            self.relative(
                group,
                track_group::TRANSFORM_LOD_ERROR_COUNT,
                "LOD error count",
            )?,
            self.relative(
                group,
                track_group::TRANSFORM_LOD_ERRORS_PTR,
                "LOD error pointer",
            )?,
            4,
            "transform LOD errors",
        )?;
        let mut errors = Vec::with_capacity(count);
        for index in 0..count {
            let offset = self.element_offset(array, index, 4, "transform LOD error")?;
            errors.push(self.f32_at(offset, "transform LOD error")?);
        }
        Ok(errors)
    }

    fn parse_text_tracks(&self, group: usize) -> Result<Vec<TextTrack>> {
        let (count, array) = self.array(
            self.relative(group, track_group::TEXT_TRACK_COUNT, "text-track count")?,
            self.relative(group, track_group::TEXT_TRACKS_PTR, "text-track pointer")?,
            text_track::SIZE,
            "text tracks",
        )?;
        let mut tracks = Vec::with_capacity(count);
        for index in 0..count {
            let offset = self.element_offset(array, index, text_track::SIZE, "text track")?;
            tracks.push(self.parse_text_track(offset)?);
        }
        Ok(tracks)
    }

    fn parse_vector_track(&self, offset: usize) -> Result<VectorTrack> {
        self.range(offset, vector_track::SIZE, "vector track")?;
        Ok(VectorTrack {
            name: self.optional_string_field(
                offset,
                vector_track::NAME_PTR,
                "vector-track name",
            )?,
            track_key: self.u32_field(offset, vector_track::TRACK_KEY, "vector-track key")?,
            dimension: self.i32_field(offset, vector_track::DIMENSION, "vector-track dimension")?,
            value: self.parse_curve(self.relative(
                offset,
                vector_track::VALUE_CURVE,
                "vector-track value curve",
            )?)?,
        })
    }

    fn parse_transform_track(&self, offset: usize) -> Result<TransformTrack> {
        self.range(offset, transform_track::SIZE, "transform track")?;
        Ok(TransformTrack {
            name: self.optional_string_field(
                offset,
                transform_track::NAME_PTR,
                "transform-track name",
            )?,
            flags: self.i32_field(offset, transform_track::FLAGS, "transform-track flags")?,
            orientation: self.parse_curve(self.relative(
                offset,
                transform_track::ORIENTATION_CURVE,
                "orientation curve",
            )?)?,
            position: self.parse_curve(self.relative(
                offset,
                transform_track::POSITION_CURVE,
                "position curve",
            )?)?,
            scale_shear: self.parse_curve(self.relative(
                offset,
                transform_track::SCALE_SHEAR_CURVE,
                "scale/shear curve",
            )?)?,
        })
    }

    fn parse_text_track(&self, offset: usize) -> Result<TextTrack> {
        self.range(offset, text_track::SIZE, "text track")?;
        let (entry_count, entry_array) = self.array(
            self.relative(offset, text_track::ENTRY_COUNT, "text-track entry count")?,
            self.relative(offset, text_track::ENTRIES_PTR, "text-track entry pointer")?,
            text_track_entry::SIZE,
            "text-track entries",
        )?;
        let mut entries = Vec::with_capacity(entry_count);
        for index in 0..entry_count {
            let entry_offset = self.element_offset(
                entry_array,
                index,
                text_track_entry::SIZE,
                "text-track entry",
            )?;
            entries.push(TextTrackEntry {
                time_stamp: self.f32_field(
                    entry_offset,
                    text_track_entry::TIME_STAMP,
                    "text-track timestamp",
                )?,
                text: self.optional_string_field(
                    entry_offset,
                    text_track_entry::TEXT_PTR,
                    "text-track text",
                )?,
            });
        }
        Ok(TextTrack {
            name: self.optional_string_field(offset, text_track::NAME_PTR, "text-track name")?,
            entries,
        })
    }

    fn parse_periodic_loop(&self, offset: usize) -> Result<PeriodicLoop> {
        self.range(offset, periodic_loop::SIZE, "periodic loop")?;
        Ok(PeriodicLoop {
            radius: self.f32_field(offset, periodic_loop::RADIUS, "periodic-loop radius")?,
            d_angle: self.f32_field(offset, periodic_loop::D_ANGLE, "periodic-loop angle")?,
            d_z: self.f32_field(offset, periodic_loop::D_Z, "periodic-loop Z delta")?,
            basis_x: self.f32_array::<3>(
                self.relative(offset, periodic_loop::BASIS_X, "periodic-loop basis X")?,
                "periodic-loop basis X",
            )?,
            basis_y: self.f32_array::<3>(
                self.relative(offset, periodic_loop::BASIS_Y, "periodic-loop basis Y")?,
                "periodic-loop basis Y",
            )?,
            axis: self.f32_array::<3>(
                self.relative(offset, periodic_loop::AXIS, "periodic-loop axis")?,
                "periodic-loop axis",
            )?,
        })
    }

    fn parse_transform(&self, offset: usize) -> Result<Transform> {
        self.range(offset, transform::SIZE, "transform")?;
        Ok(Transform {
            flags: self.u32_field(offset, transform::FLAGS, "transform flags")?,
            position: self.f32_array::<3>(
                self.relative(offset, transform::POSITION, "transform position")?,
                "transform position",
            )?,
            orientation: self.f32_array::<4>(
                self.relative(offset, transform::ORIENTATION, "transform orientation")?,
                "transform orientation",
            )?,
            scale_shear: self.f32_array::<9>(
                self.relative(offset, transform::SCALE_SHEAR, "transform scale/shear")?,
                "transform scale/shear",
            )?,
        })
    }

    fn parse_curve(&self, offset: usize) -> Result<CurveData> {
        self.range(offset, curve2::SIZE, "curve variant")?;
        let type_offset = self.required_ptr_at(
            self.relative(offset, curve2::TYPE_PTR, "curve type pointer")?,
            "curve type",
        )?;
        let object_offset = self.required_ptr_at(
            self.relative(offset, curve2::OBJECT_PTR, "curve object pointer")?,
            "curve object",
        )?;
        self.range(object_offset, curve_data_header::SIZE, "curve header")?;
        let format = self.u8_at(object_offset, "curve format")?;
        let degree = self.u8_at(
            self.relative(object_offset, curve_data_header::DEGREE, "curve degree")?,
            "curve degree",
        )?;
        self.validate_curve_type(type_offset, format)?;
        let payload_offset =
            self.relative(object_offset, curve_data_header::SIZE, "curve payload")?;
        let payload = self.parse_curve_payload(format, payload_offset)?;
        Ok(CurveData {
            format,
            degree,
            payload,
        })
    }

    fn validate_curve_type(&self, type_offset: usize, format: u8) -> Result<()> {
        let expected = curve_type_name(format).ok_or(Error::UnsupportedCurveFormat(format))?;
        self.range(type_offset, Self::TYPE_MEMBER_SIZE, "curve type member")?;
        let member_type = self.u32_at(type_offset, "curve root member type")?;
        let actual = if member_type == 1 {
            let name_field = self.relative(type_offset, 4, "curve type name pointer")?;
            let name_offset = self.required_ptr_at(name_field, "curve type name")?;
            self.cstring_at(name_offset)?
        } else {
            "<invalid curve root member>".to_string()
        };
        if actual != expected {
            return Err(Error::InvalidCurveType {
                format,
                expected,
                actual,
            });
        }
        Ok(())
    }

    fn parse_curve_payload(&self, format: u8, offset: usize) -> Result<CurvePayload> {
        match format {
            0..=9 => self.parse_low_curve_payload(format, offset),
            10..=18 => self.parse_high_curve_payload(format, offset),
            _ => Err(Error::UnsupportedCurveFormat(format)),
        }
    }

    fn parse_low_curve_payload(&self, format: u8, offset: usize) -> Result<CurvePayload> {
        match format {
            0 => Ok(CurvePayload::DaKeyframes32f {
                dimension: self.i16_at(offset, "DaKeyframes32f dimension")?,
                controls: self.f32_ref_array(
                    self.relative(offset, 2, "DaKeyframes32f controls")?,
                    "DaKeyframes32f controls",
                )?,
            }),
            1 => Ok(CurvePayload::DaK32fC32f {
                padding: self.i16_at(offset, "DaK32fC32f padding")?,
                knots: self.f32_ref_array(
                    self.relative(offset, 2, "DaK32fC32f knots")?,
                    "DaK32fC32f knots",
                )?,
                controls: self.f32_ref_array(
                    self.relative(offset, 14, "DaK32fC32f controls")?,
                    "DaK32fC32f controls",
                )?,
            }),
            2 => Ok(CurvePayload::Identity {
                dimension: self.i16_at(offset, "DaIdentity dimension")?,
            }),
            3 => Ok(CurvePayload::DaConstant32f {
                padding: self.i16_at(offset, "DaConstant32f padding")?,
                controls: self.f32_ref_array(
                    self.relative(offset, 2, "DaConstant32f controls")?,
                    "DaConstant32f controls",
                )?,
            }),
            4 => Ok(CurvePayload::D3Constant32f {
                padding: self.i16_at(offset, "D3Constant32f padding")?,
                controls: self.f32_array::<3>(
                    self.relative(offset, 2, "D3Constant32f controls")?,
                    "D3Constant32f controls",
                )?,
            }),
            5 => Ok(CurvePayload::D4Constant32f {
                padding: self.i16_at(offset, "D4Constant32f padding")?,
                controls: self.f32_array::<4>(
                    self.relative(offset, 2, "D4Constant32f controls")?,
                    "D4Constant32f controls",
                )?,
            }),
            6 => Ok(CurvePayload::DaK16uC16u {
                one_over_knot_scale_trunc: self.u16_at(offset, "DaK16uC16u knot scale")?,
                control_scale_offsets: self.f32_ref_array(
                    self.relative(offset, 2, "DaK16uC16u scale offsets")?,
                    "DaK16uC16u scale offsets",
                )?,
                knots_controls: self.u16_ref_array(
                    self.relative(offset, 14, "DaK16uC16u knots/controls")?,
                    "DaK16uC16u knots/controls",
                )?,
            }),
            7 => Ok(CurvePayload::DaK8uC8u {
                one_over_knot_scale_trunc: self.u16_at(offset, "DaK8uC8u knot scale")?,
                control_scale_offsets: self.f32_ref_array(
                    self.relative(offset, 2, "DaK8uC8u scale offsets")?,
                    "DaK8uC8u scale offsets",
                )?,
                knots_controls: self.u8_ref_array(
                    self.relative(offset, 14, "DaK8uC8u knots/controls")?,
                    "DaK8uC8u knots/controls",
                )?,
            }),
            8 => Ok(CurvePayload::D4nK16uC15u {
                scale_offset_table_entries: self.u16_at(offset, "D4nK16uC15u table entries")?,
                one_over_knot_scale: self.f32_at(
                    self.relative(offset, 2, "D4nK16uC15u knot scale")?,
                    "D4nK16uC15u knot scale",
                )?,
                knots_controls: self.u16_ref_array(
                    self.relative(offset, 6, "D4nK16uC15u knots/controls")?,
                    "D4nK16uC15u knots/controls",
                )?,
            }),
            9 => Ok(CurvePayload::D4nK8uC7u {
                scale_offset_table_entries: self.u16_at(offset, "D4nK8uC7u table entries")?,
                one_over_knot_scale: self.f32_at(
                    self.relative(offset, 2, "D4nK8uC7u knot scale")?,
                    "D4nK8uC7u knot scale",
                )?,
                knots_controls: self.u8_ref_array(
                    self.relative(offset, 6, "D4nK8uC7u knots/controls")?,
                    "D4nK8uC7u knots/controls",
                )?,
            }),
            _ => Err(Error::UnsupportedCurveFormat(format)),
        }
    }

    fn parse_high_curve_payload(&self, format: u8, offset: usize) -> Result<CurvePayload> {
        match format {
            10 => self
                .parse_d3_u16(offset)
                .map(|(scale, scales, offsets, data)| CurvePayload::D3K16uC16u {
                    one_over_knot_scale_trunc: scale,
                    control_scales: scales,
                    control_offsets: offsets,
                    knots_controls: data,
                }),
            11 => self
                .parse_d3_u8(offset)
                .map(|(scale, scales, offsets, data)| CurvePayload::D3K8uC8u {
                    one_over_knot_scale_trunc: scale,
                    control_scales: scales,
                    control_offsets: offsets,
                    knots_controls: data,
                }),
            12 => Ok(CurvePayload::D9I1K16uC16u {
                one_over_knot_scale_trunc: self.u16_at(offset, "D9I1K16uC16u knot scale")?,
                control_scale: self.f32_at(
                    self.relative(offset, 2, "D9I1K16uC16u control scale")?,
                    "D9I1K16uC16u control scale",
                )?,
                control_offset: self.f32_at(
                    self.relative(offset, 6, "D9I1K16uC16u control offset")?,
                    "D9I1K16uC16u control offset",
                )?,
                knots_controls: self.u16_ref_array(
                    self.relative(offset, 10, "D9I1K16uC16u knots/controls")?,
                    "D9I1K16uC16u knots/controls",
                )?,
            }),
            13 => self
                .parse_d3_u16(offset)
                .map(
                    |(scale, scales, offsets, data)| CurvePayload::D9I3K16uC16u {
                        one_over_knot_scale_trunc: scale,
                        control_scales: scales,
                        control_offsets: offsets,
                        knots_controls: data,
                    },
                ),
            14 => Ok(CurvePayload::D9I1K8uC8u {
                one_over_knot_scale_trunc: self.u16_at(offset, "D9I1K8uC8u knot scale")?,
                control_scale: self.f32_at(
                    self.relative(offset, 2, "D9I1K8uC8u control scale")?,
                    "D9I1K8uC8u control scale",
                )?,
                control_offset: self.f32_at(
                    self.relative(offset, 6, "D9I1K8uC8u control offset")?,
                    "D9I1K8uC8u control offset",
                )?,
                knots_controls: self.u8_ref_array(
                    self.relative(offset, 10, "D9I1K8uC8u knots/controls")?,
                    "D9I1K8uC8u knots/controls",
                )?,
            }),
            15 => self
                .parse_d3_u8(offset)
                .map(|(scale, scales, offsets, data)| CurvePayload::D9I3K8uC8u {
                    one_over_knot_scale_trunc: scale,
                    control_scales: scales,
                    control_offsets: offsets,
                    knots_controls: data,
                }),
            16 => Ok(CurvePayload::D3I1K32fC32f {
                padding: self.u16_at(offset, "D3I1K32fC32f padding")?,
                control_scales: self.f32_array::<3>(
                    self.relative(offset, 2, "D3I1K32fC32f scales")?,
                    "D3I1K32fC32f scales",
                )?,
                control_offsets: self.f32_array::<3>(
                    self.relative(offset, 14, "D3I1K32fC32f offsets")?,
                    "D3I1K32fC32f offsets",
                )?,
                knots_controls: self.f32_ref_array(
                    self.relative(offset, 26, "D3I1K32fC32f knots/controls")?,
                    "D3I1K32fC32f knots/controls",
                )?,
            }),
            17 => self
                .parse_d3_u16(offset)
                .map(
                    |(scale, scales, offsets, data)| CurvePayload::D3I1K16uC16u {
                        one_over_knot_scale_trunc: scale,
                        control_scales: scales,
                        control_offsets: offsets,
                        knots_controls: data,
                    },
                ),
            18 => self
                .parse_d3_u8(offset)
                .map(|(scale, scales, offsets, data)| CurvePayload::D3I1K8uC8u {
                    one_over_knot_scale_trunc: scale,
                    control_scales: scales,
                    control_offsets: offsets,
                    knots_controls: data,
                }),
            _ => Err(Error::UnsupportedCurveFormat(format)),
        }
    }

    fn parse_d3_u16(&self, offset: usize) -> Result<D3U16Payload> {
        Ok((
            self.u16_at(offset, "u16 curve knot scale")?,
            self.f32_array::<3>(
                self.relative(offset, 2, "u16 curve scales")?,
                "u16 curve scales",
            )?,
            self.f32_array::<3>(
                self.relative(offset, 14, "u16 curve offsets")?,
                "u16 curve offsets",
            )?,
            self.u16_ref_array(
                self.relative(offset, 26, "u16 curve knots/controls")?,
                "u16 curve knots/controls",
            )?,
        ))
    }

    fn parse_d3_u8(&self, offset: usize) -> Result<D3U8Payload> {
        Ok((
            self.u16_at(offset, "u8 curve knot scale")?,
            self.f32_array::<3>(
                self.relative(offset, 2, "u8 curve scales")?,
                "u8 curve scales",
            )?,
            self.f32_array::<3>(
                self.relative(offset, 14, "u8 curve offsets")?,
                "u8 curve offsets",
            )?,
            self.u8_ref_array(
                self.relative(offset, 26, "u8 curve knots/controls")?,
                "u8 curve knots/controls",
            )?,
        ))
    }

    fn require_empty_variant(&self, offset: usize, field: &'static str) -> Result<()> {
        self.range(offset, variant::SIZE, field)?;
        if self.u64_at(offset, field)? != 0
            || self.u64_at(self.relative(offset, variant::OBJECT_PTR, field)?, field)? != 0
        {
            return Err(Error::UnsupportedExtendedData(field));
        }
        Ok(())
    }

    fn pointer_array(
        &self,
        count_offset: usize,
        ptr_offset: usize,
        field: &'static str,
    ) -> Result<Vec<usize>> {
        let (count, array_offset) = self.array(count_offset, ptr_offset, 8, field)?;
        let mut pointers = Vec::with_capacity(count);
        for index in 0..count {
            let item_offset = self.element_offset(array_offset, index, 8, field)?;
            pointers.push(self.required_ptr_at(item_offset, field)?);
        }
        Ok(pointers)
    }

    fn array(
        &self,
        count_offset: usize,
        ptr_offset: usize,
        element_size: usize,
        field: &'static str,
    ) -> Result<(usize, usize)> {
        let signed_count = self.i32_at(count_offset, field)?;
        let count =
            usize::try_from(signed_count).map_err(|_| Error::InvalidCount(field, signed_count))?;
        if count == 0 {
            return Ok((0, 0));
        }
        let pointer = self.required_ptr_at(ptr_offset, field)?;
        let size = count
            .checked_mul(element_size)
            .ok_or(Error::SizeOverflow(field))?;
        self.range(pointer, size, field)?;
        Ok((count, pointer))
    }

    fn f32_ref_array(&self, offset: usize, field: &'static str) -> Result<Vec<f32>> {
        let (count, pointer) = self.array(offset, self.relative(offset, 4, field)?, 4, field)?;
        let mut values = Vec::with_capacity(count);
        for index in 0..count {
            values.push(self.f32_at(self.element_offset(pointer, index, 4, field)?, field)?);
        }
        Ok(values)
    }

    fn u16_ref_array(&self, offset: usize, field: &'static str) -> Result<Vec<u16>> {
        let (count, pointer) = self.array(offset, self.relative(offset, 4, field)?, 2, field)?;
        let mut values = Vec::with_capacity(count);
        for index in 0..count {
            values.push(self.u16_at(self.element_offset(pointer, index, 2, field)?, field)?);
        }
        Ok(values)
    }

    fn u8_ref_array(&self, offset: usize, field: &'static str) -> Result<Vec<u8>> {
        let (count, pointer) = self.array(offset, self.relative(offset, 4, field)?, 1, field)?;
        Ok(self.range(pointer, count, field)?.to_vec())
    }

    fn optional_string_field(
        &self,
        base: usize,
        relative: usize,
        field: &'static str,
    ) -> Result<Option<String>> {
        let pointer_field = self.relative(base, relative, field)?;
        self.optional_ptr_at(pointer_field, field)?
            .map(|pointer| self.cstring_at(pointer))
            .transpose()
    }

    fn required_string_field(
        &self,
        base: usize,
        relative: usize,
        field: &'static str,
    ) -> Result<String> {
        let pointer_field = self.relative(base, relative, field)?;
        let pointer = self.required_ptr_at(pointer_field, field)?;
        self.cstring_at(pointer)
    }

    fn cstring_at(&self, offset: usize) -> Result<String> {
        let bytes = self.data.get(offset..).ok_or(Error::InvalidPointerOffset(
            u64::try_from(offset).unwrap_or(u64::MAX),
            self.data.len(),
        ))?;
        let end = bytes
            .iter()
            .position(|byte| *byte == 0)
            .ok_or(Error::StringReadError(
                u64::try_from(offset).unwrap_or(u64::MAX),
            ))?;
        String::from_utf8(bytes[..end].to_vec())
            .map_err(|_| Error::StringReadError(u64::try_from(offset).unwrap_or(u64::MAX)))
    }

    fn optional_ptr_at(&self, offset: usize, field: &'static str) -> Result<Option<usize>> {
        let raw = self.u64_at(offset, field)?;
        if raw == 0 {
            return Ok(None);
        }
        let pointer =
            usize::try_from(raw).map_err(|_| Error::InvalidPointerOffset(raw, self.data.len()))?;
        if pointer >= self.data.len() {
            return Err(Error::InvalidPointerOffset(raw, self.data.len()));
        }
        Ok(Some(pointer))
    }

    fn required_ptr_at(&self, offset: usize, field: &'static str) -> Result<usize> {
        self.optional_ptr_at(offset, field)?
            .ok_or(Error::NullPointer(field))
    }

    fn element_offset(
        &self,
        base: usize,
        index: usize,
        size: usize,
        field: &'static str,
    ) -> Result<usize> {
        let relative = index.checked_mul(size).ok_or(Error::SizeOverflow(field))?;
        self.relative(base, relative, field)
    }

    fn relative(&self, base: usize, relative: usize, field: &'static str) -> Result<usize> {
        let offset = base
            .checked_add(relative)
            .ok_or(Error::SizeOverflow(field))?;
        if offset > self.data.len() {
            return Err(Error::InvalidRange {
                field,
                offset,
                size: 0,
                chunk_size: self.data.len(),
            });
        }
        Ok(offset)
    }

    fn range(&self, offset: usize, size: usize, field: &'static str) -> Result<&'a [u8]> {
        let end = offset.checked_add(size).ok_or(Error::SizeOverflow(field))?;
        self.data.get(offset..end).ok_or(Error::InvalidRange {
            field,
            offset,
            size,
            chunk_size: self.data.len(),
        })
    }

    fn u8_at(&self, offset: usize, field: &'static str) -> Result<u8> {
        Ok(self.range(offset, 1, field)?[0])
    }

    fn u16_at(&self, offset: usize, field: &'static str) -> Result<u16> {
        let bytes: [u8; 2] = self
            .range(offset, 2, field)?
            .try_into()
            .map_err(|_| Error::UnexpectedEof)?;
        Ok(u16::from_le_bytes(bytes))
    }

    fn i16_at(&self, offset: usize, field: &'static str) -> Result<i16> {
        Ok(self.u16_at(offset, field)?.cast_signed())
    }

    fn u32_at(&self, offset: usize, field: &'static str) -> Result<u32> {
        let bytes: [u8; 4] = self
            .range(offset, 4, field)?
            .try_into()
            .map_err(|_| Error::UnexpectedEof)?;
        Ok(u32::from_le_bytes(bytes))
    }

    fn i32_at(&self, offset: usize, field: &'static str) -> Result<i32> {
        Ok(self.u32_at(offset, field)?.cast_signed())
    }

    fn u64_at(&self, offset: usize, field: &'static str) -> Result<u64> {
        let bytes: [u8; 8] = self
            .range(offset, 8, field)?
            .try_into()
            .map_err(|_| Error::UnexpectedEof)?;
        Ok(u64::from_le_bytes(bytes))
    }

    fn f32_at(&self, offset: usize, field: &'static str) -> Result<f32> {
        Ok(f32::from_bits(self.u32_at(offset, field)?))
    }

    fn f32_field(&self, base: usize, relative: usize, field: &'static str) -> Result<f32> {
        self.f32_at(self.relative(base, relative, field)?, field)
    }

    fn u32_field(&self, base: usize, relative: usize, field: &'static str) -> Result<u32> {
        self.u32_at(self.relative(base, relative, field)?, field)
    }

    fn i32_field(&self, base: usize, relative: usize, field: &'static str) -> Result<i32> {
        self.i32_at(self.relative(base, relative, field)?, field)
    }

    fn f32_array<const N: usize>(&self, offset: usize, field: &'static str) -> Result<[f32; N]> {
        let byte_size = N.checked_mul(4).ok_or(Error::SizeOverflow(field))?;
        self.range(offset, byte_size, field)?;
        let mut values = [0.0; N];
        for (index, value) in values.iter_mut().enumerate() {
            *value = self.f32_at(self.element_offset(offset, index, 4, field)?, field)?;
        }
        Ok(values)
    }
}

#[cfg(test)]
mod tests;
