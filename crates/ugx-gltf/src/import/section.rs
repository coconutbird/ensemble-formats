//! UGX section binding classification and explicit authoring overrides.

use ugx::{Bone, Error, Result, UnpackedVertex};

use crate::extras::{MeshExtrasJson, SectionModeJson};

use super::checked_i32;
use super::mesh::detect_global_bones;

pub(super) struct SectionKind {
    pub global_bones: bool,
    pub rigid_only: bool,
    pub bone_index: i32,
    pub max_bones: i32,
}

pub(super) fn apply_forced_bone(
    vertices: &mut [UnpackedVertex],
    extras: Option<&MeshExtrasJson>,
    bones: &[Bone],
) -> Result<()> {
    let Some(name) = extras
        .map(|value| value.force_bone.as_str())
        .filter(|name| !name.is_empty())
    else {
        return Ok(());
    };
    let bone_index = resolve_bone_index(name, bones)?;
    let packed_index =
        u16::try_from(bone_index).map_err(|_| Error::SizeOverflow("forced skin bone index"))?;
    for vertex in vertices {
        vertex.bone_indices = [packed_index; 4];
        vertex.bone_weights = [1.0, 0.0, 0.0, 0.0];
    }
    Ok(())
}

fn resolve_bone_index(name: &str, bones: &[Bone]) -> Result<usize> {
    bones
        .iter()
        .position(|bone| bone.name == name)
        .ok_or_else(|| Error::UnsupportedFormat(format!("UGX binding bone '{name}' was not found")))
}

fn section_bone_index(
    extras: &MeshExtrasJson,
    parent_bone_index: Option<usize>,
    bones: &[Bone],
) -> Result<i32> {
    if !extras.binding_bone.is_empty() {
        return checked_i32(
            resolve_bone_index(&extras.binding_bone, bones)?,
            "section binding bone index",
        );
    }
    if let Some(index) = parent_bone_index {
        return checked_i32(index, "section binding bone index");
    }
    if bones.len() == 1 {
        return Ok(0);
    }
    Err(Error::UnsupportedFormat(
        "A rigid/global section must name its UGX binding bone".into(),
    ))
}

fn section_max_bones(vertices: &[UnpackedVertex], requested: Option<i32>) -> Result<i32> {
    let actual = vertices
        .iter()
        .map(|vertex| {
            vertex
                .bone_weights
                .iter()
                .filter(|&&weight| weight > 0.0)
                .count()
        })
        .max()
        .unwrap_or(1)
        .max(1);
    let actual = checked_i32(actual, "maximum bone influences")?;
    let Some(requested) = requested else {
        return Ok(actual);
    };
    if !(1..=4).contains(&requested) {
        return Err(Error::UnsupportedFormat(
            "UGX MaxBones must be between 1 and 4".into(),
        ));
    }
    if requested < actual {
        return Err(Error::UnsupportedFormat(format!(
            "UGX MaxBones {requested} is smaller than the {actual} influences used by the mesh"
        )));
    }
    Ok(requested)
}

pub(super) fn classify_section(
    vertices: &[UnpackedVertex],
    has_skin: bool,
    mesh_has_skin: bool,
    parent_bone_index: Option<usize>,
    has_bones: bool,
    extras: Option<&MeshExtrasJson>,
    bones: &[Bone],
) -> Result<SectionKind> {
    let mode = extras.map_or(SectionModeJson::Auto, |value| value.section_mode);
    if mode == SectionModeJson::Skinned {
        if !has_skin {
            return Err(Error::UnsupportedFormat(
                "A forced skinned UGX section needs skin weights or a Force Bone override".into(),
            ));
        }
        return Ok(SectionKind {
            global_bones: false,
            rigid_only: false,
            bone_index: i32::MAX,
            max_bones: section_max_bones(vertices, extras.and_then(|value| value.max_bones))?,
        });
    }
    if matches!(mode, SectionModeJson::Rigid | SectionModeJson::Global) {
        let extras = extras.ok_or_else(|| {
            Error::UnsupportedFormat("Forced section binding metadata is missing".into())
        })?;
        return Ok(SectionKind {
            global_bones: mode == SectionModeJson::Global,
            rigid_only: mode == SectionModeJson::Rigid,
            bone_index: section_bone_index(extras, parent_bone_index, bones)?,
            max_bones: 1,
        });
    }
    if !mesh_has_skin && let Some(bone_index) = parent_bone_index {
        return Ok(SectionKind {
            global_bones: true,
            rigid_only: true,
            bone_index: checked_i32(bone_index, "rigid bone index")?,
            max_bones: 1,
        });
    }
    if has_skin && mesh_has_skin {
        return Ok(SectionKind {
            global_bones: false,
            rigid_only: false,
            bone_index: i32::MAX,
            max_bones: section_max_bones(vertices, extras.and_then(|value| value.max_bones))?,
        });
    }
    if has_bones && !has_skin {
        return Ok(SectionKind {
            global_bones: false,
            rigid_only: true,
            bone_index: 0,
            max_bones: 1,
        });
    }
    let (global_bones, rigid_only, bone_index, max_bones) = detect_global_bones(vertices, has_skin);
    Ok(SectionKind {
        global_bones,
        rigid_only,
        bone_index,
        max_bones,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bone(name: &str) -> Bone {
        Bone {
            name: name.into(),
            parent_index: -1,
            model_to_bone: ugx::Matrix4x4::identity(),
        }
    }

    #[test]
    fn forced_skinned_binding_uses_named_bone_and_preserved_capacity() {
        let bones = [bone("GrannyRootBone"), bone("AttachBone")];
        let extras = MeshExtrasJson {
            section_mode: SectionModeJson::Skinned,
            force_bone: "AttachBone".into(),
            max_bones: Some(4),
            ..MeshExtrasJson::default()
        };
        let mut vertices = [UnpackedVertex::default()];

        apply_forced_bone(&mut vertices, Some(&extras), &bones).unwrap();
        let kind =
            classify_section(&vertices, true, true, None, true, Some(&extras), &bones).unwrap();

        assert_eq!(vertices[0].bone_indices, [1; 4]);
        assert_eq!(
            vertices[0].bone_weights.map(f32::to_bits),
            [1.0_f32.to_bits(), 0, 0, 0]
        );
        assert!(!kind.rigid_only);
        assert!(!kind.global_bones);
        assert_eq!(kind.max_bones, 4);
    }
}
