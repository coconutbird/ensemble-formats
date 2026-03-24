//! Mesh primitive construction for glTF export.
//!
//! Converts UGX vertex/index data into glTF accessors, buffer views, and primitives.

use gltf_json as json;
use json::validation::Checked::Valid;

use ugx::UnpackedVertex;

/// Create a mesh primitive from vertices and indices.
#[allow(clippy::too_many_arguments)]
pub(crate) fn create_primitive(
    vertices: &[UnpackedVertex],
    indices: &[u16],
    material_index: i32,
    buffer_data: &mut Vec<u8>,
    accessors: &mut Vec<json::Accessor>,
    buffer_views: &mut Vec<json::buffer::View>,
    has_materials: bool,
    has_skeleton: bool,
    bone_count: usize,
    rigid_bone_index: i32,
) -> json::mesh::Primitive {
    let mut attributes = std::collections::BTreeMap::new();

    // Calculate bounds for position accessor
    let mut min_pos = [f32::MAX; 3];
    let mut max_pos = [f32::MIN; 3];
    for v in vertices {
        for i in 0..3 {
            min_pos[i] = min_pos[i].min(v.position[i]);
            max_pos[i] = max_pos[i].max(v.position[i]);
        }
    }

    // Write positions
    // Pad to 4-byte boundary for float alignment
    while !buffer_data.len().is_multiple_of(4) {
        buffer_data.push(0);
    }
    let pos_view_idx = buffer_views.len() as u32;
    let pos_offset = buffer_data.len();
    for v in vertices {
        buffer_data.extend_from_slice(&v.position[0].to_le_bytes());
        buffer_data.extend_from_slice(&v.position[1].to_le_bytes());
        buffer_data.extend_from_slice(&v.position[2].to_le_bytes());
    }
    let pos_byte_length = buffer_data.len() - pos_offset;

    buffer_views.push(json::buffer::View {
        buffer: json::Index::new(0),
        byte_length: json::validation::USize64(pos_byte_length as u64),
        byte_offset: Some(json::validation::USize64(pos_offset as u64)),
        byte_stride: Some(json::buffer::Stride(12)),
        extensions: None,
        extras: json::Extras::default(),
        name: None,
        target: Some(Valid(json::buffer::Target::ArrayBuffer)),
    });

    let pos_accessor_idx = accessors.len() as u32;
    accessors.push(json::Accessor {
        buffer_view: Some(json::Index::new(pos_view_idx)),
        byte_offset: Some(json::validation::USize64(0)),
        count: json::validation::USize64(vertices.len() as u64),
        component_type: Valid(json::accessor::GenericComponentType(
            json::accessor::ComponentType::F32,
        )),
        extensions: None,
        extras: json::Extras::default(),
        type_: Valid(json::accessor::Type::Vec3),
        min: Some(json::Value::from(min_pos.to_vec())),
        max: Some(json::Value::from(max_pos.to_vec())),
        name: None,
        normalized: false,
        sparse: None,
    });
    attributes.insert(
        Valid(json::mesh::Semantic::Positions),
        json::Index::new(pos_accessor_idx),
    );

    // Write normals (normalized to unit length for glTF compliance)
    let norm_view_idx = buffer_views.len() as u32;
    let norm_offset = buffer_data.len();
    for v in vertices {
        let len =
            (v.normal[0] * v.normal[0] + v.normal[1] * v.normal[1] + v.normal[2] * v.normal[2])
                .sqrt();
        let (nx, ny, nz) = if len > 1e-6 {
            (v.normal[0] / len, v.normal[1] / len, v.normal[2] / len)
        } else {
            (0.0, 1.0, 0.0)
        };
        buffer_data.extend_from_slice(&nx.to_le_bytes());
        buffer_data.extend_from_slice(&ny.to_le_bytes());
        buffer_data.extend_from_slice(&nz.to_le_bytes());
    }
    let norm_byte_length = buffer_data.len() - norm_offset;

    buffer_views.push(json::buffer::View {
        buffer: json::Index::new(0),
        byte_length: json::validation::USize64(norm_byte_length as u64),
        byte_offset: Some(json::validation::USize64(norm_offset as u64)),
        byte_stride: Some(json::buffer::Stride(12)),
        extensions: None,
        extras: json::Extras::default(),
        name: None,
        target: Some(Valid(json::buffer::Target::ArrayBuffer)),
    });

    let norm_accessor_idx = accessors.len() as u32;
    accessors.push(json::Accessor {
        buffer_view: Some(json::Index::new(norm_view_idx)),
        byte_offset: Some(json::validation::USize64(0)),
        count: json::validation::USize64(vertices.len() as u64),
        component_type: Valid(json::accessor::GenericComponentType(
            json::accessor::ComponentType::F32,
        )),
        extensions: None,
        extras: json::Extras::default(),
        type_: Valid(json::accessor::Type::Vec3),
        min: None,
        max: None,
        name: None,
        normalized: false,
        sparse: None,
    });
    attributes.insert(
        Valid(json::mesh::Semantic::Normals),
        json::Index::new(norm_accessor_idx),
    );

    // Write UV sets (TEXCOORD_0, TEXCOORD_1, ...)
    let max_texcoords = vertices.iter().map(|v| v.num_texcoords).max().unwrap_or(0);
    for uv_set in 0..max_texcoords {
        let uv_view_idx = buffer_views.len() as u32;
        let uv_offset = buffer_data.len();
        for v in vertices {
            // glTF and DirectX both use V=0 at top (no flip needed).
            // Note: the Python Blender script flips V for Blender's convention,
            // but that's Blender-specific — glTF matches DX convention already.
            buffer_data.extend_from_slice(&v.texcoords[uv_set][0].to_le_bytes());
            buffer_data.extend_from_slice(&v.texcoords[uv_set][1].to_le_bytes());
        }
        let uv_byte_length = buffer_data.len() - uv_offset;

        buffer_views.push(json::buffer::View {
            buffer: json::Index::new(0),
            byte_length: json::validation::USize64(uv_byte_length as u64),
            byte_offset: Some(json::validation::USize64(uv_offset as u64)),
            byte_stride: Some(json::buffer::Stride(8)),
            extensions: None,
            extras: json::Extras::default(),
            name: None,
            target: Some(Valid(json::buffer::Target::ArrayBuffer)),
        });

        let uv_accessor_idx = accessors.len() as u32;
        accessors.push(json::Accessor {
            buffer_view: Some(json::Index::new(uv_view_idx)),
            byte_offset: Some(json::validation::USize64(0)),
            count: json::validation::USize64(vertices.len() as u64),
            component_type: Valid(json::accessor::GenericComponentType(
                json::accessor::ComponentType::F32,
            )),
            extensions: None,
            extras: json::Extras::default(),
            type_: Valid(json::accessor::Type::Vec2),
            min: None,
            max: None,
            name: None,
            normalized: false,
            sparse: None,
        });
        attributes.insert(
            Valid(json::mesh::Semantic::TexCoords(uv_set as u32)),
            json::Index::new(uv_accessor_idx),
        );
    }

    // Write tangents (vec4: xyz + handedness in w)
    let has_tangents = vertices
        .iter()
        .any(|v| v.tangent[0] != 0.0 || v.tangent[1] != 0.0 || v.tangent[2] != 0.0);
    if has_tangents {
        let tangent_view_idx = buffer_views.len() as u32;
        let tangent_offset = buffer_data.len();
        for v in vertices {
            // glTF requires unit-length tangent xyz. UGX tangents may not be
            // normalized (e.g. HWDE stores them at length 0.5).
            let len = (v.tangent[0] * v.tangent[0]
                + v.tangent[1] * v.tangent[1]
                + v.tangent[2] * v.tangent[2])
                .sqrt();
            let (tx, ty, tz) = if len > 1e-6 {
                (v.tangent[0] / len, v.tangent[1] / len, v.tangent[2] / len)
            } else {
                (1.0, 0.0, 0.0)
            };
            buffer_data.extend_from_slice(&tx.to_le_bytes());
            buffer_data.extend_from_slice(&ty.to_le_bytes());
            buffer_data.extend_from_slice(&tz.to_le_bytes());
            // glTF tangent.w is handedness: +1 or -1.
            // UGX stores this in tangent[3]; default to 1.0 if unset.
            let w = if v.tangent[3] == 0.0 {
                1.0f32
            } else {
                v.tangent[3]
            };
            buffer_data.extend_from_slice(&w.to_le_bytes());
        }
        let tangent_byte_length = buffer_data.len() - tangent_offset;

        buffer_views.push(json::buffer::View {
            buffer: json::Index::new(0),
            byte_length: json::validation::USize64(tangent_byte_length as u64),
            byte_offset: Some(json::validation::USize64(tangent_offset as u64)),
            byte_stride: Some(json::buffer::Stride(16)),
            extensions: None,
            extras: json::Extras::default(),
            name: None,
            target: Some(Valid(json::buffer::Target::ArrayBuffer)),
        });

        let tangent_accessor_idx = accessors.len() as u32;
        accessors.push(json::Accessor {
            buffer_view: Some(json::Index::new(tangent_view_idx)),
            byte_offset: Some(json::validation::USize64(0)),
            count: json::validation::USize64(vertices.len() as u64),
            component_type: Valid(json::accessor::GenericComponentType(
                json::accessor::ComponentType::F32,
            )),
            extensions: None,
            extras: json::Extras::default(),
            type_: Valid(json::accessor::Type::Vec4),
            min: None,
            max: None,
            name: None,
            normalized: false,
            sparse: None,
        });
        attributes.insert(
            Valid(json::mesh::Semantic::Tangents),
            json::Index::new(tangent_accessor_idx),
        );
    }

    // Write bone indices and weights if we have a skeleton
    //
    // TODO: Bone remap — when section.bone_remap is non-empty, vertex bone indices
    // are section-local and need to be remapped to global skeleton indices using the
    // remap table before writing to glTF JOINTS_0. Currently we write indices as-is
    // (converting from 1-based to 0-based), which is correct only when global_bones
    // is true or bone_remap is empty. To fix: pass bone_remap into create_primitive,
    // and when non-empty, do `global_idx = bone_remap[local_idx]` before the 1-based
    // to 0-based conversion. Need a real skinned UGX file with per-section bone
    // remaps to verify.
    if has_skeleton && bone_count > 0 {
        let max_bone_idx = (bone_count - 1) as u16;
        let use_u16_joints = bone_count > 256;
        // rigid_bone_index can be INT_MAX (0x7FFFFFFF) meaning "no rigid bone".
        // Default to bone 0 when invalid.
        let rigid_idx: u16 = if rigid_bone_index >= 0 && (rigid_bone_index as usize) < bone_count {
            rigid_bone_index as u16
        } else {
            0
        };

        // JOINTS_0 - bone indices as u8 (<=256 bones) or u16 (>256 bones)
        // Pad to 2-byte boundary if using u16
        if use_u16_joints && !buffer_data.len().is_multiple_of(2) {
            buffer_data.push(0);
        }
        let joints_view_idx = buffer_views.len() as u32;
        let joints_offset = buffer_data.len();
        for v in vertices {
            let weight_sum: f32 = v.bone_weights.iter().sum();
            let joint_indices = if weight_sum == 0.0 {
                // Rigid vertex (no skin data) - bind to section's rigid bone
                [rigid_idx, 0, 0, 0]
            } else {
                let mut indices = v.bone_indices;
                for idx in &mut indices {
                    // Vertex bone indices are 1-based in UGX data (0 = no bone);
                    // convert to 0-based for glTF joint indices.
                    if *idx > 0 {
                        *idx -= 1;
                    }
                    if *idx > max_bone_idx {
                        *idx = 0;
                    }
                }
                indices
            };
            if use_u16_joints {
                for &idx in &joint_indices {
                    buffer_data.extend_from_slice(&idx.to_le_bytes());
                }
            } else {
                for &idx in &joint_indices {
                    buffer_data.push(idx as u8);
                }
            }
        }
        let joints_byte_length = buffer_data.len() - joints_offset;
        let joints_stride = if use_u16_joints { 8 } else { 4 };

        buffer_views.push(json::buffer::View {
            buffer: json::Index::new(0),
            byte_length: json::validation::USize64(joints_byte_length as u64),
            byte_offset: Some(json::validation::USize64(joints_offset as u64)),
            byte_stride: Some(json::buffer::Stride(joints_stride)),
            extensions: None,
            extras: json::Extras::default(),
            name: None,
            target: Some(Valid(json::buffer::Target::ArrayBuffer)),
        });

        let joints_component_type = if use_u16_joints {
            json::accessor::ComponentType::U16
        } else {
            json::accessor::ComponentType::U8
        };
        let joints_accessor_idx = accessors.len() as u32;
        accessors.push(json::Accessor {
            buffer_view: Some(json::Index::new(joints_view_idx)),
            byte_offset: Some(json::validation::USize64(0)),
            count: json::validation::USize64(vertices.len() as u64),
            component_type: Valid(json::accessor::GenericComponentType(joints_component_type)),
            extensions: None,
            extras: json::Extras::default(),
            type_: Valid(json::accessor::Type::Vec4),
            min: None,
            max: None,
            name: None,
            normalized: false,
            sparse: None,
        });
        attributes.insert(
            Valid(json::mesh::Semantic::Joints(0)),
            json::Index::new(joints_accessor_idx),
        );

        // WEIGHTS_0 - bone weights as floats
        // Pad to 4-byte boundary for float alignment
        while !buffer_data.len().is_multiple_of(4) {
            buffer_data.push(0);
        }
        let weights_view_idx = buffer_views.len() as u32;
        let weights_offset = buffer_data.len();
        for v in vertices {
            let mut weights = v.bone_weights;
            let sum: f32 = weights.iter().sum();
            if sum == 0.0 {
                // Rigid vertex - 100% weight on rigid bone
                weights[0] = 1.0;
            } else if (sum - 1.0).abs() > 0.001 {
                for w in &mut weights {
                    *w /= sum;
                }
            }
            buffer_data.extend_from_slice(&weights[0].to_le_bytes());
            buffer_data.extend_from_slice(&weights[1].to_le_bytes());
            buffer_data.extend_from_slice(&weights[2].to_le_bytes());
            buffer_data.extend_from_slice(&weights[3].to_le_bytes());
        }
        let weights_byte_length = buffer_data.len() - weights_offset;

        buffer_views.push(json::buffer::View {
            buffer: json::Index::new(0),
            byte_length: json::validation::USize64(weights_byte_length as u64),
            byte_offset: Some(json::validation::USize64(weights_offset as u64)),
            byte_stride: Some(json::buffer::Stride(16)),
            extensions: None,
            extras: json::Extras::default(),
            name: None,
            target: Some(Valid(json::buffer::Target::ArrayBuffer)),
        });

        let weights_accessor_idx = accessors.len() as u32;
        accessors.push(json::Accessor {
            buffer_view: Some(json::Index::new(weights_view_idx)),
            byte_offset: Some(json::validation::USize64(0)),
            count: json::validation::USize64(vertices.len() as u64),
            component_type: Valid(json::accessor::GenericComponentType(
                json::accessor::ComponentType::F32,
            )),
            extensions: None,
            extras: json::Extras::default(),
            type_: Valid(json::accessor::Type::Vec4),
            min: None,
            max: None,
            name: None,
            normalized: false,
            sparse: None,
        });
        attributes.insert(
            Valid(json::mesh::Semantic::Weights(0)),
            json::Index::new(weights_accessor_idx),
        );
    }

    // Write vertex colors (COLOR_0) if any vertex has non-zero diffuse
    let has_colors = vertices.iter().any(|v| {
        v.diffuse[0] != 0.0 || v.diffuse[1] != 0.0 || v.diffuse[2] != 0.0 || v.diffuse[3] != 0.0
    });
    if has_colors {
        // Pad to 4-byte boundary for float alignment
        while !buffer_data.len().is_multiple_of(4) {
            buffer_data.push(0);
        }
        let color_view_idx = buffer_views.len() as u32;
        let color_offset = buffer_data.len();
        for v in vertices {
            buffer_data.extend_from_slice(&v.diffuse[0].to_le_bytes());
            buffer_data.extend_from_slice(&v.diffuse[1].to_le_bytes());
            buffer_data.extend_from_slice(&v.diffuse[2].to_le_bytes());
            buffer_data.extend_from_slice(&v.diffuse[3].to_le_bytes());
        }
        let color_byte_length = buffer_data.len() - color_offset;

        buffer_views.push(json::buffer::View {
            buffer: json::Index::new(0),
            byte_length: json::validation::USize64(color_byte_length as u64),
            byte_offset: Some(json::validation::USize64(color_offset as u64)),
            byte_stride: Some(json::buffer::Stride(16)),
            extensions: None,
            extras: json::Extras::default(),
            name: None,
            target: Some(Valid(json::buffer::Target::ArrayBuffer)),
        });

        let color_accessor_idx = accessors.len() as u32;
        accessors.push(json::Accessor {
            buffer_view: Some(json::Index::new(color_view_idx)),
            byte_offset: Some(json::validation::USize64(0)),
            count: json::validation::USize64(vertices.len() as u64),
            component_type: Valid(json::accessor::GenericComponentType(
                json::accessor::ComponentType::F32,
            )),
            extensions: None,
            extras: json::Extras::default(),
            type_: Valid(json::accessor::Type::Vec4),
            min: None,
            max: None,
            name: None,
            normalized: false,
            sparse: None,
        });
        attributes.insert(
            Valid(json::mesh::Semantic::Colors(0)),
            json::Index::new(color_accessor_idx),
        );
    }

    // Write indices
    // Pad to 2-byte boundary for u16 alignment
    if !buffer_data.len().is_multiple_of(2) {
        buffer_data.push(0);
    }
    let idx_view_idx = buffer_views.len() as u32;
    let idx_offset = buffer_data.len();
    for idx in indices {
        buffer_data.extend_from_slice(&idx.to_le_bytes());
    }
    let idx_byte_length = buffer_data.len() - idx_offset;

    buffer_views.push(json::buffer::View {
        buffer: json::Index::new(0),
        byte_length: json::validation::USize64(idx_byte_length as u64),
        byte_offset: Some(json::validation::USize64(idx_offset as u64)),
        byte_stride: None,
        extensions: None,
        extras: json::Extras::default(),
        name: None,
        target: Some(Valid(json::buffer::Target::ElementArrayBuffer)),
    });

    let idx_accessor_idx = accessors.len() as u32;
    accessors.push(json::Accessor {
        buffer_view: Some(json::Index::new(idx_view_idx)),
        byte_offset: Some(json::validation::USize64(0)),
        count: json::validation::USize64(indices.len() as u64),
        component_type: Valid(json::accessor::GenericComponentType(
            json::accessor::ComponentType::U16,
        )),
        extensions: None,
        extras: json::Extras::default(),
        type_: Valid(json::accessor::Type::Scalar),
        min: None,
        max: None,
        name: None,
        normalized: false,
        sparse: None,
    });

    let material = if has_materials && material_index >= 0 {
        Some(json::Index::new(material_index as u32))
    } else {
        None
    };

    json::mesh::Primitive {
        attributes,
        extensions: None,
        extras: json::Extras::default(),
        indices: Some(json::Index::new(idx_accessor_idx)),
        material,
        mode: Valid(json::mesh::Mode::Triangles),
        targets: None,
    }
}
