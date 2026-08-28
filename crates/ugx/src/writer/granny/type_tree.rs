//! `FileInfo` type tree builder — constructs the Granny2 schema describing
//! all structures in the chunk.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::types::{GrannyMemberType, GrannyTypeMember};

/// Helper: create a simple scalar/string member with no nested type.
fn tm(member_type: GrannyMemberType, name: &str) -> GrannyTypeMember {
    GrannyTypeMember {
        member_type,
        name: String::from(name),
        reference_type: None,
        array_width: 0,
        extra: [0; 3],
    }
}

/// Helper: create a member with a nested reference type.
fn tm_ref(
    member_type: GrannyMemberType,
    name: &str,
    nested: Vec<GrannyTypeMember>,
) -> GrannyTypeMember {
    GrannyTypeMember {
        member_type,
        name: String::from(name),
        reference_type: Some(nested),
        array_width: 0,
        extra: [0; 3],
    }
}

/// Helper: create a Real32 member with a specific array width.
fn tm_real32_array(name: &str, width: u32) -> GrannyTypeMember {
    GrannyTypeMember {
        member_type: GrannyMemberType::Real32,
        name: String::from(name),
        reference_type: None,
        array_width: width,
        extra: [0; 3],
    }
}

/// Helper: create an Int32 member with a specific array width.
fn tm_int32_array(name: &str, width: u32) -> GrannyTypeMember {
    GrannyTypeMember {
        member_type: GrannyMemberType::Int32,
        name: String::from(name),
        reference_type: None,
        array_width: width,
        extra: [0; 3],
    }
}

/// Build the bone type definition matching the engine's hardcoded
/// `GrannyBoneTypeDef` at `0x1414621D0`.
fn build_bone_type() -> Vec<GrannyTypeMember> {
    vec![
        tm(GrannyMemberType::StringMember, "Name"),
        tm(GrannyMemberType::Int32, "ParentIndex"),
        tm(GrannyMemberType::Transform, "Transform"),
        tm_real32_array("InverseWorldTransform", 16),
        tm(GrannyMemberType::Real32, "LODError"),
        tm(GrannyMemberType::VariantReference, "ExtendedData"),
    ]
}

/// Build the `bone_binding` type definition matching the engine's at `0x141460EA0`.
fn build_bone_binding_type() -> Vec<GrannyTypeMember> {
    vec![
        tm(GrannyMemberType::StringMember, "BoneName"),
        tm_real32_array("OBBMin", 3),
        tm_real32_array("OBBMax", 3),
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "TriangleIndices",
            vec![tm(GrannyMemberType::Int32, "Int32")],
        ),
    ]
}

/// Build the `VertexData` type definition matching the engine's at `0x14145E3E0`.
fn build_vertex_data_type() -> Vec<GrannyTypeMember> {
    vec![
        tm(GrannyMemberType::ReferenceToVariantArray, "Vertices"),
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "VertexComponentNames",
            vec![tm(GrannyMemberType::StringMember, "String")],
        ),
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "VertexAnnotationSets",
            vec![
                tm(GrannyMemberType::StringMember, "Name"),
                tm(
                    GrannyMemberType::ReferenceToVariantArray,
                    "VertexAnnotations",
                ),
                tm(GrannyMemberType::Int32, "IndicesMapFromVertexToAnnotation"),
                tm_ref(
                    GrannyMemberType::ReferenceToArray,
                    "VertexAnnotationIndices",
                    vec![tm(GrannyMemberType::Int32, "Int32")],
                ),
            ],
        ),
    ]
}

/// Build the `TriTopology` type definition matching the engine's at `0x14145F1A0`.
fn build_tri_topology_type() -> Vec<GrannyTypeMember> {
    let int32_elem = vec![tm(GrannyMemberType::Int32, "Int32")];
    let int16_elem = vec![tm(GrannyMemberType::Int16, "Int16")];
    vec![
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "Groups",
            vec![
                tm(GrannyMemberType::Int32, "MaterialIndex"),
                tm(GrannyMemberType::Int32, "TriFirst"),
                tm(GrannyMemberType::Int32, "TriCount"),
            ],
        ),
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "Indices",
            int32_elem.clone(),
        ),
        tm_ref(GrannyMemberType::ReferenceToArray, "Indices16", int16_elem),
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "VertexToVertexMap",
            int32_elem.clone(),
        ),
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "VertexToTriangleMap",
            int32_elem.clone(),
        ),
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "SideToNeighborMap",
            int32_elem.clone(),
        ),
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "PolygonIndexStarts",
            int32_elem.clone(),
        ),
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "PolygonIndices",
            int32_elem.clone(),
        ),
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "BonesForTriangle",
            int32_elem.clone(),
        ),
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "TriangleToBoneIndices",
            int32_elem,
        ),
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "TriAnnotationSets",
            vec![
                tm(GrannyMemberType::StringMember, "Name"),
                tm(GrannyMemberType::ReferenceToVariantArray, "TriAnnotations"),
                tm(GrannyMemberType::Int32, "IndicesMapFromTriToAnnotation"),
                tm_ref(
                    GrannyMemberType::ReferenceToArray,
                    "TriAnnotationIndices",
                    vec![tm(GrannyMemberType::Int32, "Int32")],
                ),
            ],
        ),
    ]
}

/// Build the mesh type definition matching the engine's at `0x141461090`.
fn build_mesh_type() -> Vec<GrannyTypeMember> {
    vec![
        tm(GrannyMemberType::StringMember, "Name"),
        tm_ref(
            GrannyMemberType::Reference,
            "PrimaryVertexData",
            build_vertex_data_type(),
        ),
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "MorphTargets",
            vec![
                tm(GrannyMemberType::StringMember, "ScalarName"),
                tm_ref(
                    GrannyMemberType::Reference,
                    "VertexData",
                    build_vertex_data_type(),
                ),
                tm(GrannyMemberType::Int32, "DataIsDeltas"),
            ],
        ),
        tm_ref(
            GrannyMemberType::Reference,
            "PrimaryTopology",
            build_tri_topology_type(),
        ),
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "MaterialBindings",
            vec![tm_ref(
                GrannyMemberType::Reference,
                "Material",
                build_material_type(),
            )],
        ),
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "BoneBindings",
            build_bone_binding_type(),
        ),
        tm(GrannyMemberType::VariantReference, "ExtendedData"),
    ]
}

/// Build the skeleton type definition matching the engine's at `0x141462310`.
fn build_skeleton_type() -> Vec<GrannyTypeMember> {
    vec![
        tm(GrannyMemberType::StringMember, "Name"),
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "Bones",
            build_bone_type(),
        ),
        tm(GrannyMemberType::Int32, "LODType"),
        tm(GrannyMemberType::VariantReference, "ExtendedData"),
    ]
}

/// Build the model type definition matching the engine's at `0x14145C980`.
fn build_model_type() -> Vec<GrannyTypeMember> {
    vec![
        tm(GrannyMemberType::StringMember, "Name"),
        tm_ref(
            GrannyMemberType::Reference,
            "Skeleton",
            build_skeleton_type(),
        ),
        tm(GrannyMemberType::Transform, "InitialPlacement"),
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "MeshBindings",
            vec![tm_ref(
                GrannyMemberType::Reference,
                "Mesh",
                build_mesh_type(),
            )],
        ),
        tm(GrannyMemberType::VariantReference, "ExtendedData"),
    ]
}

/// Build the `ArtToolInfo` type definition matching the engine's at `0x141461330`.
fn build_art_tool_info_type() -> Vec<GrannyTypeMember> {
    vec![
        tm(GrannyMemberType::StringMember, "FromArtToolName"),
        tm(GrannyMemberType::Int32, "ArtToolMajorRevision"),
        tm(GrannyMemberType::Int32, "ArtToolMinorRevision"),
        tm(GrannyMemberType::Int32, "ArtToolPointerSize"),
        tm(GrannyMemberType::Real32, "UnitsPerMeter"),
        tm_real32_array("Origin", 3),
        tm_real32_array("RightVector", 3),
        tm_real32_array("UpVector", 3),
        tm_real32_array("BackVector", 3),
        tm(GrannyMemberType::VariantReference, "ExtendedData"),
    ]
}

/// Build the `ExporterInfo` type definition matching the engine's at `0x1414611F0`.
fn build_exporter_info_type() -> Vec<GrannyTypeMember> {
    vec![
        tm(GrannyMemberType::StringMember, "ExporterName"),
        tm(GrannyMemberType::Int32, "ExporterMajorRevision"),
        tm(GrannyMemberType::Int32, "ExporterMinorRevision"),
        tm(GrannyMemberType::Int32, "ExporterCustomization"),
        tm(GrannyMemberType::Int32, "ExporterBuildNumber"),
        tm(GrannyMemberType::VariantReference, "ExtendedData"),
    ]
}

/// Build the Texture type definition matching the engine's at `0x1414623F0`.
fn build_texture_type() -> Vec<GrannyTypeMember> {
    vec![
        tm(GrannyMemberType::StringMember, "FromFileName"),
        tm(GrannyMemberType::Int32, "TextureType"),
        tm(GrannyMemberType::Int32, "Width"),
        tm(GrannyMemberType::Int32, "Height"),
        tm(GrannyMemberType::Int32, "Encoding"),
        tm(GrannyMemberType::Int32, "SubFormat"),
        tm_ref(
            GrannyMemberType::Inline,
            "Layout",
            vec![
                tm(GrannyMemberType::Int32, "BytesPerPixel"),
                tm_int32_array("ShiftForComponent", 4),
                tm_int32_array("BitsForComponent", 4),
            ],
        ),
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "Images",
            vec![tm_ref(
                GrannyMemberType::ReferenceToArray,
                "MIPLevels",
                vec![
                    tm(GrannyMemberType::Int32, "Stride"),
                    tm_ref(
                        GrannyMemberType::ReferenceToArray,
                        "PixelBytes",
                        vec![tm(GrannyMemberType::UInt8, "UInt8")],
                    ),
                ],
            )],
        ),
        tm(GrannyMemberType::VariantReference, "ExtendedData"),
    ]
}

/// Build the Material type definition matching the engine's at `0x141461E60`.
fn build_material_type() -> Vec<GrannyTypeMember> {
    vec![
        tm(GrannyMemberType::StringMember, "Name"),
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "Maps",
            vec![
                tm(GrannyMemberType::StringMember, "Usage"),
                // Material.Maps[].Map is a circular Reference back to Material.
                // We break the cycle by omitting the nested type (empty ref).
                tm_ref(GrannyMemberType::Reference, "Map", Vec::new()),
            ],
        ),
        tm_ref(GrannyMemberType::Reference, "Texture", build_texture_type()),
        tm(GrannyMemberType::VariantReference, "ExtendedData"),
    ]
}

/// Build the `TrackGroup` type definition matching the engine's at `0x14145CDA0`.
fn build_track_group_type() -> Vec<GrannyTypeMember> {
    vec![
        tm(GrannyMemberType::StringMember, "Name"),
        // VectorTracks, TransformTracks, etc. have deep nesting into curve data.
        // We include the member names but use empty nested refs for curves.
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "VectorTracks",
            Vec::new(),
        ),
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "TransformTracks",
            Vec::new(),
        ),
        tm_ref(
            GrannyMemberType::ReferenceToArray,
            "TransformLODErrors",
            Vec::new(),
        ),
        tm_ref(GrannyMemberType::ReferenceToArray, "TextTracks", Vec::new()),
        tm(GrannyMemberType::Transform, "InitialPlacement"),
        tm(GrannyMemberType::Int32, "AccumulationFlags"),
        tm_real32_array("LoopTranslation", 3),
        tm_ref(GrannyMemberType::Reference, "PeriodicLoop", Vec::new()),
        tm(GrannyMemberType::VariantReference, "ExtendedData"),
    ]
}

/// Build the Animation type definition matching the engine's at `0x141462890`.
fn build_animation_type() -> Vec<GrannyTypeMember> {
    vec![
        tm(GrannyMemberType::StringMember, "Name"),
        tm(GrannyMemberType::Real32, "Duration"),
        tm(GrannyMemberType::Real32, "TimeStep"),
        tm(GrannyMemberType::Real32, "Oversampling"),
        tm_ref(
            GrannyMemberType::ArrayOfReferences,
            "TrackGroups",
            build_track_group_type(),
        ),
        tm(GrannyMemberType::Int32, "DefaultLoopCount"),
        tm(GrannyMemberType::Int32, "Flags"),
        tm(GrannyMemberType::VariantReference, "ExtendedData"),
    ]
}

/// Build the complete `FileInfo` type definition tree matching the engine's
/// hardcoded `GrannyFileInfoTypeDef` at `0x14145C7D0` → `0x141461B60`.
///
/// This constructs the Granny2 schema matching the `file_info` struct layout:
/// ```text
/// struct file_info {
///     art_tool_info *ArtToolInfo;       // Reference
///     exporter_info *ExporterInfo;      // Reference
///     char const *FromFileName;         // String
///     texture **Textures;              // ArrayOfReferences
///     material **Materials;            // ArrayOfReferences
///     skeleton **Skeletons;            // ArrayOfReferences
///     vertex_data **VertexDatas;       // ArrayOfReferences
///     tri_topology **TriTopologies;    // ArrayOfReferences
///     mesh **Meshes;                   // ArrayOfReferences
///     model **Models;                  // ArrayOfReferences
///     track_group **TrackGroups;       // ArrayOfReferences
///     animation **Animations;          // ArrayOfReferences
///     variant ExtendedData;            // VariantReference
/// };
/// ```
pub(super) fn build_file_info_type_tree() -> Vec<GrannyTypeMember> {
    vec![
        tm_ref(
            GrannyMemberType::Reference,
            "ArtToolInfo",
            build_art_tool_info_type(),
        ),
        tm_ref(
            GrannyMemberType::Reference,
            "ExporterInfo",
            build_exporter_info_type(),
        ),
        tm(GrannyMemberType::StringMember, "FromFileName"),
        tm_ref(
            GrannyMemberType::ArrayOfReferences,
            "Textures",
            build_texture_type(),
        ),
        tm_ref(
            GrannyMemberType::ArrayOfReferences,
            "Materials",
            build_material_type(),
        ),
        tm_ref(
            GrannyMemberType::ArrayOfReferences,
            "Skeletons",
            build_skeleton_type(),
        ),
        tm_ref(
            GrannyMemberType::ArrayOfReferences,
            "VertexDatas",
            build_vertex_data_type(),
        ),
        tm_ref(
            GrannyMemberType::ArrayOfReferences,
            "TriTopologies",
            build_tri_topology_type(),
        ),
        tm_ref(
            GrannyMemberType::ArrayOfReferences,
            "Meshes",
            build_mesh_type(),
        ),
        tm_ref(
            GrannyMemberType::ArrayOfReferences,
            "Models",
            build_model_type(),
        ),
        tm_ref(
            GrannyMemberType::ArrayOfReferences,
            "TrackGroups",
            build_track_group_type(),
        ),
        tm_ref(
            GrannyMemberType::ArrayOfReferences,
            "Animations",
            build_animation_type(),
        ),
        tm(GrannyMemberType::VariantReference, "ExtendedData"),
    ]
}
