//! LGT stream reader.

use alloc::string::String;
use alloc::vec::Vec;

use nostdio::{Cursor, IoError, ReadLe};

use crate::records::{Camera, CameraClip, CameraRange, Frame, Light, LightFlags, Object, Scene};
use crate::{
    CAMERA_CLIP_REVISION, CAMERA_RANGE_REVISION, Error, LIGHT_BUFFERED_VERSION, MAX_RECORDS,
    Result, VERSION_FAMILY,
};

impl Scene {
    /// Decodes one complete little-endian `LightData::BScene` stream.
    ///
    /// # Errors
    ///
    /// Returns an error for an unsupported version, a truncated record, an
    /// invalid count, or a closing version word that is not equal to the
    /// opening version word.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let mut reader = Reader::new(data);
        let version = reader.u32()?;
        if version >> 8 != VERSION_FAMILY {
            return Err(Error::UnsupportedVersion(version));
        }
        let frame_count = reader.count("frames")?;
        let mut frames = Vec::with_capacity(frame_count);
        for _ in 0..frame_count {
            frames.push(read_frame(&mut reader, version)?);
        }
        let closing = reader.u32()?;
        if closing != version {
            return Err(Error::VersionFooter {
                opening: version,
                closing,
            });
        }
        Ok(Self { version, frames })
    }
}

fn read_frame(reader: &mut Reader<'_>, version: u32) -> Result<Frame> {
    let time = reader.f32()?;
    let light_count = reader.count("lights")?;
    let mut lights = Vec::with_capacity(light_count);
    for _ in 0..light_count {
        lights.push(read_light(reader, version)?);
    }
    let object_count = reader.count("objects")?;
    let mut objects = Vec::with_capacity(object_count);
    for _ in 0..object_count {
        objects.push(read_object(reader)?);
    }
    let camera_count = reader.count("cameras")?;
    let mut cameras = Vec::with_capacity(camera_count);
    for _ in 0..camera_count {
        cameras.push(read_camera(reader, version)?);
    }
    Ok(Frame {
        time,
        lights,
        objects,
        cameras,
    })
}

fn read_light(reader: &mut Reader<'_>, version: u32) -> Result<Light> {
    let (name, position, orientation) = read_base(reader)?;
    let mask = reader.string()?;
    let kind = reader.u8()?.into();
    let radius = reader.f32()?;
    let far_attenuation_start = reader.f32()?;
    let decay_distance = reader.f32()?;
    let decay_kind = reader.u8()?;
    let intensity = reader.f32()?;
    let color = reader.vec3()?;
    let spot_inner = reader.f32()?;
    let spot_outer = reader.f32()?;
    let mut flags = 0;
    for mask in [
        LightFlags::SHADOWS,
        LightFlags::DIFFUSE,
        LightFlags::SPECULAR,
        LightFlags::FOG,
        LightFlags::FOG_SHADOWS,
    ] {
        if reader.bool()? {
            flags |= mask;
        }
    }
    let fog_density = reader.f32()?;
    if version < LIGHT_BUFFERED_VERSION || reader.bool()? {
        flags |= LightFlags::LIGHT_BUFFERED;
    }
    Ok(Light {
        name,
        mask,
        kind,
        position,
        orientation,
        radius,
        far_attenuation_start,
        decay_distance,
        decay_kind,
        intensity,
        color,
        spot_inner,
        spot_outer,
        flags: LightFlags::from_bits(flags),
        fog_density,
    })
}

fn read_object(reader: &mut Reader<'_>) -> Result<Object> {
    let (name, position, orientation) = read_base(reader)?;
    let kind = reader.u8()?;
    let visibility = reader.f32()?;
    let sphere_radius = reader.f32()?;
    let byte_count = reader.count("object UDP bytes")?;
    let user_properties = reader.bytes(byte_count, "object UDP bytes")?.to_vec();
    Ok(Object {
        name,
        position,
        orientation,
        kind,
        visibility,
        sphere_radius,
        user_properties,
    })
}

fn read_camera(reader: &mut Reader<'_>, version: u32) -> Result<Camera> {
    let (name, position, orientation) = read_base(reader)?;
    let fov = reader.f32()?;
    let revision = version & 0xFF;
    let range = if revision >= CAMERA_RANGE_REVISION {
        Some(CameraRange {
            focal_depth: reader.f32()?,
            near_range: reader.f32()?,
            far_range: reader.f32()?,
        })
    } else {
        None
    };
    let clip = if revision >= CAMERA_CLIP_REVISION {
        Some(CameraClip {
            near_clip: reader.f32()?,
            far_clip: reader.f32()?,
        })
    } else {
        None
    };
    Ok(Camera {
        name,
        position,
        orientation,
        fov,
        range,
        clip,
    })
}

fn read_base(reader: &mut Reader<'_>) -> Result<(String, [f32; 3], [f32; 4])> {
    Ok((reader.string()?, reader.vec3()?, reader.vec4()?))
}

/// Little-endian reader that reports the offset and field of a truncation.
struct Reader<'a> {
    cursor: Cursor<&'a [u8]>,
}

impl<'a> Reader<'a> {
    const fn new(data: &'a [u8]) -> Self {
        Self {
            cursor: Cursor::new(data),
        }
    }

    fn data(&self) -> &'a [u8] {
        self.cursor.get_ref()
    }

    /// Returns the byte offset of the next read.
    ///
    /// The cursor only moves forward inside the data, so the value always fits.
    fn offset(&self) -> usize {
        usize::try_from(self.cursor.position()).unwrap_or(usize::MAX)
    }

    fn scalar<T>(
        &mut self,
        field: &'static str,
        read: impl FnOnce(&mut Cursor<&'a [u8]>) -> core::result::Result<T, IoError>,
    ) -> Result<T> {
        let offset = self.offset();
        read(&mut self.cursor).map_err(|_| Error::Truncated { offset, field })
    }

    fn bytes(&mut self, count: usize, field: &'static str) -> Result<&'a [u8]> {
        let offset = self.offset();
        let truncated = || Error::Truncated { offset, field };
        let end = offset.checked_add(count).ok_or_else(truncated)?;
        let value = self.data().get(offset..end).ok_or_else(truncated)?;
        self.cursor
            .set_position(u64::try_from(end).map_err(|_| truncated())?);
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8> {
        self.scalar("u8", ReadLe::read_u8_le)
    }

    fn bool(&mut self) -> Result<bool> {
        Ok(self.u8()? != 0)
    }

    fn u32(&mut self) -> Result<u32> {
        self.scalar("u32", ReadLe::read_u32_le)
    }

    fn i32(&mut self) -> Result<i32> {
        self.scalar("i32", ReadLe::read_i32_le)
    }

    fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_bits(self.u32()?))
    }

    fn vec3(&mut self) -> Result<[f32; 3]> {
        Ok([self.f32()?, self.f32()?, self.f32()?])
    }

    fn vec4(&mut self) -> Result<[f32; 4]> {
        Ok([self.f32()?, self.f32()?, self.f32()?, self.f32()?])
    }

    /// Reads a signed count. The count must not exceed [`MAX_RECORDS`] or the data length.
    fn count(&mut self, field: &'static str) -> Result<usize> {
        let value = self.i32()?;
        match usize::try_from(value) {
            Ok(count) if count <= MAX_RECORDS && count <= self.data().len() => Ok(count),
            _ => Err(Error::InvalidCount { field, value }),
        }
    }

    /// Reads a `u32` length and the string bytes. Invalid UTF-8 becomes U+FFFD.
    fn string(&mut self) -> Result<String> {
        let raw_length = self.u32()?;
        let Ok(length) = usize::try_from(raw_length) else {
            return Err(Error::InvalidCount {
                field: "string bytes",
                value: i32::MAX,
            });
        };
        if length > self.data().len() {
            return Err(Error::InvalidCount {
                field: "string bytes",
                value: i32::try_from(raw_length).unwrap_or(i32::MAX),
            });
        }
        Ok(String::from_utf8_lossy(self.bytes(length, "string bytes")?).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec;
    use alloc::vec::Vec;

    use crate::{CameraClip, CameraRange, Error, LightKind, Scene};

    #[test]
    fn decodes_version_109_light_and_footer() {
        let scene = Scene::from_bytes(&scene_bytes(0xCCDD_0109, true)).unwrap();
        assert_eq!(scene.version, 0xCCDD_0109);
        assert_eq!(scene.frames.len(), 1);
        let light = &scene.frames[0].lights[0];
        assert_eq!(light.name, "test");
        assert_eq!(light.kind, LightKind::Omni);
        assert!(light.flags.shadows());
        assert!(light.flags.diffuse());
        assert!(light.flags.specular());
        assert!(!light.flags.fog());
        assert!(!light.flags.fog_shadows());
        assert!(light.flags.light_buffered());
        assert_eq!(
            light.position.map(f32::to_bits),
            [64.0_f32, 128.0, 192.0].map(f32::to_bits)
        );
    }

    #[test]
    fn version_107_defaults_light_buffering_on() {
        let scene = Scene::from_bytes(&scene_bytes(0xCCDD_0107, false)).unwrap();
        assert!(scene.frames[0].lights[0].flags.light_buffered());
    }

    #[test]
    fn rejects_mismatched_footer() {
        let mut bytes = scene_bytes(0xCCDD_0109, true);
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        assert!(matches!(
            Scene::from_bytes(&bytes),
            Err(Error::VersionFooter { .. })
        ));
    }

    #[test]
    fn camera_fields_follow_the_version_revision() {
        for (version, range, clip) in [
            (0xCCDD_0107, false, false),
            (0xCCDD_0109, true, false),
            (0xCCDD_010A, true, true),
        ] {
            let scene = Scene::from_bytes(&object_camera_bytes(version)).unwrap();
            let frame = &scene.frames[0];
            assert_eq!(frame.objects[0].name, "box");
            assert_eq!(frame.objects[0].user_properties, vec![7, 8]);
            let camera = &frame.cameras[0];
            assert_eq!(camera.name, "cam");
            assert_eq!(
                camera.range,
                range.then_some(CameraRange {
                    focal_depth: 1.0,
                    near_range: 2.0,
                    far_range: 3.0,
                })
            );
            assert_eq!(
                camera.clip,
                clip.then_some(CameraClip {
                    near_clip: 4.0,
                    far_clip: 5.0,
                })
            );
        }
    }

    fn scene_bytes(version: u32, include_light_buffered: bool) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_u32(&mut bytes, version);
        push_i32(&mut bytes, 1);
        push_f32(&mut bytes, 0.0);
        push_i32(&mut bytes, 1);
        push_string(&mut bytes, "test");
        for value in [64.0, 128.0, 192.0, 0.0, 0.0, 0.0, 1.0] {
            push_f32(&mut bytes, value);
        }
        push_string(&mut bytes, "");
        bytes.push(2);
        for value in [256.0, 0.25, 64.0] {
            push_f32(&mut bytes, value);
        }
        bytes.push(0);
        push_f32(&mut bytes, 2.0);
        for value in [0.5, 0.25, 0.125, 0.1, 0.2] {
            push_f32(&mut bytes, value);
        }
        bytes.extend([1, 1, 1, 0, 0]);
        push_f32(&mut bytes, 1.0);
        if include_light_buffered {
            bytes.push(1);
        }
        push_i32(&mut bytes, 0);
        push_i32(&mut bytes, 0);
        push_u32(&mut bytes, version);
        bytes
    }

    fn object_camera_bytes(version: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_u32(&mut bytes, version);
        push_i32(&mut bytes, 1);
        push_f32(&mut bytes, 0.0);
        push_i32(&mut bytes, 0);
        push_i32(&mut bytes, 1);
        push_string(&mut bytes, "box");
        for value in [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0] {
            push_f32(&mut bytes, value);
        }
        bytes.push(1);
        push_f32(&mut bytes, 1.0);
        push_f32(&mut bytes, 8.0);
        push_i32(&mut bytes, 2);
        bytes.extend([7, 8]);
        push_i32(&mut bytes, 1);
        push_string(&mut bytes, "cam");
        for value in [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.5] {
            push_f32(&mut bytes, value);
        }
        if version & 0xFF >= 8 {
            for value in [1.0, 2.0, 3.0] {
                push_f32(&mut bytes, value);
            }
        }
        if version & 0xFF >= 10 {
            for value in [4.0, 5.0] {
                push_f32(&mut bytes, value);
            }
        }
        push_u32(&mut bytes, version);
        bytes
    }

    fn push_string(bytes: &mut Vec<u8>, value: &str) {
        push_u32(bytes, u32::try_from(value.len()).unwrap());
        bytes.extend(value.as_bytes());
    }

    fn push_u32(bytes: &mut Vec<u8>, value: u32) {
        bytes.extend(value.to_le_bytes());
    }

    fn push_i32(bytes: &mut Vec<u8>, value: i32) {
        bytes.extend(value.to_le_bytes());
    }

    fn push_f32(bytes: &mut Vec<u8>, value: f32) {
        push_u32(bytes, value.to_bits());
    }
}
