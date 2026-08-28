//! Human-readable UGX information reporting.

use std::fs;
use std::path::Path;

use ugx::Reader as UgxReader;

pub(crate) fn cmd_info(input: &Path, no_verify: bool) -> Result<(), Box<dyn std::error::Error>> {
    let data = fs::read(input)?;
    let geom = if no_verify {
        ugx::UgxGeom::from_bytes_unchecked(&data)?
    } else {
        UgxReader::read(&data)?
    };

    println!("UGX File: {}", input.display());
    println!();

    print_bounds(&geom);
    print_materials(&geom);
    print_bones(&geom);
    print_sections(&geom);
    print_geom_summary(&geom);

    Ok(())
}

fn print_bounds(geom: &ugx::UgxGeom) {
    println!("Bounding Sphere:");
    println!(
        "  Center: [{:.3}, {:.3}, {:.3}]",
        geom.bounding_sphere.center[0],
        geom.bounding_sphere.center[1],
        geom.bounding_sphere.center[2]
    );
    println!("  Radius: {:.3}", geom.bounding_sphere.radius);
    println!();

    println!("Bounding Box:");
    println!(
        "  Min: [{:.3}, {:.3}, {:.3}]",
        geom.bounds.min[0], geom.bounds.min[1], geom.bounds.min[2]
    );
    println!(
        "  Max: [{:.3}, {:.3}, {:.3}]",
        geom.bounds.max[0], geom.bounds.max[1], geom.bounds.max[2]
    );
    println!();
}

fn print_materials(geom: &ugx::UgxGeom) {
    println!("Materials: {}", geom.materials.len());
    for (i, mat) in geom.materials.iter().enumerate() {
        match &mat.data {
            ugx::types::MaterialData::Hogan(hogan) => {
                println!("  [{i}] (Hogan)");
                for (j, perm) in hogan.shader_permutations.iter().enumerate() {
                    println!(
                        "      Permutation[{}]: {} (hash=0x{:08X})",
                        j, perm.name, perm.hash
                    );
                }
                println!(
                    "      ufx_version={}, blend_mode={}",
                    hogan.ufx_version, hogan.blend_mode
                );
                println!(
                    "      skinned={}, terrain_blending={}, shadow_requires_consts={}",
                    hogan.skinned, hogan.terrain_blending, hogan.shadow_requires_consts
                );
                println!(
                    "      vs_cb={} ps_cb={} hs_cb={} ds_cb={} gs_cb={} bytes",
                    hogan.vs_cb_data.len(),
                    hogan.ps_cb_data.len(),
                    hogan.hs_cb_data.len(),
                    hogan.ds_cb_data.len(),
                    hogan.gs_cb_data.len(),
                );
                println!("      textures: {}", hogan.textures);
            }
            ugx::types::MaterialData::Legacy(legacy) => {
                println!("  [{}] {} (legacy v{})", i, mat.name, mat.material_version);
                println!(
                    "      blend_type={}, opacity={}, flags=0x{:X}",
                    legacy.blend_type, legacy.opacity, legacy.flags
                );
                println!(
                    "      spec_power={}, env_fresnel={}, env_fresnel_power={}",
                    legacy.spec_power, legacy.env_fresnel, legacy.env_fresnel_power
                );
                for (t, maps) in legacy.maps.iter().enumerate() {
                    if !maps.is_empty() {
                        let names: Vec<&str> = maps.iter().map(|m| m.name.as_str()).collect();
                        println!(
                            "      {}: {}",
                            ugx::types::material::MapType::ALL[t].name(),
                            names.join(", ")
                        );
                    }
                }
            }
        }
    }
    println!();
}

fn print_bones(geom: &ugx::UgxGeom) {
    println!("Bones: {}", geom.bones.len());
    for (i, bone) in geom.bones.iter().enumerate() {
        let parent = if bone.parent_index >= 0 {
            format!("parent={}", bone.parent_index)
        } else {
            "root".to_string()
        };
        println!("  [{}] {} ({})", i, bone.name, parent);
        // Print matrix for first 3 bones
        if i < 3 {
            let m = &bone.model_to_bone.rows;
            println!("      Matrix (model_to_bone):");
            println!(
                "        [{:8.4}, {:8.4}, {:8.4}, {:8.4}]",
                m[0][0], m[0][1], m[0][2], m[0][3]
            );
            println!(
                "        [{:8.4}, {:8.4}, {:8.4}, {:8.4}]",
                m[1][0], m[1][1], m[1][2], m[1][3]
            );
            println!(
                "        [{:8.4}, {:8.4}, {:8.4}, {:8.4}]",
                m[2][0], m[2][1], m[2][2], m[2][3]
            );
            println!(
                "        [{:8.4}, {:8.4}, {:8.4}, {:8.4}]",
                m[3][0], m[3][1], m[3][2], m[3][3]
            );
        }
    }
    println!();
}

fn print_sections(geom: &ugx::UgxGeom) {
    println!("Sections: {}", geom.sections.len());
    for (i, section) in geom.sections.iter().enumerate() {
        println!(
            "  [{}] Material={}, Verts={}, Tris={}, VB={}bytes, VertSize={}",
            i,
            section.material_index,
            section.num_verts,
            section.num_tris,
            section.vb_bytes,
            section.vert_size
        );
        println!(
            "       MaxBones={}, RigidBoneIdx={}, RigidOnly={}, GlobalBones={}",
            section.max_bones, section.rigid_bone_index, section.rigid_only, section.global_bones
        );
        if section.bone_remap.is_empty() {
            println!("       BoneRemap: (empty)");
        } else {
            println!(
                "       BoneRemap[{}]: {:?}",
                section.bone_remap.len(),
                section.bone_remap
            );
        }
        if let Some(ref packer) = section.base_vert_packer {
            println!("       PackOrder: {}", packer.pack_order);
            println!("       PosType: {:?}", packer.pos_type);
            println!("       NormType: {:?}", packer.normal_type);
            println!("       TangentType: {:?}", packer.tangent_type);
            println!("       UV[0]Type: {:?}", packer.uv_types[0]);
            println!("       IndicesType: {:?}", packer.indices_type);
            println!("       WeightsType: {:?}", packer.weights_type);
        } else {
            println!("       (HW2 format — no UnivertPacker)");
        }

        // Print first 3 unpacked vertices for debugging
        if section.num_verts > 0
            && let Ok(verts) = geom.unpack_section_vertices(i)
        {
            println!("       First 3 vertices:");
            for (vi, v) in verts.iter().take(3).enumerate() {
                println!(
                    "         [{}] pos=[{:.3}, {:.3}, {:.3}] bones=[{},{},{},{}] weights=[{:.3},{:.3},{:.3},{:.3}]",
                    vi,
                    v.position[0],
                    v.position[1],
                    v.position[2],
                    v.bone_indices[0],
                    v.bone_indices[1],
                    v.bone_indices[2],
                    v.bone_indices[3],
                    v.bone_weights[0],
                    v.bone_weights[1],
                    v.bone_weights[2],
                    v.bone_weights[3]
                );
            }
        }
    }
    println!();
}

fn print_geom_summary(geom: &ugx::UgxGeom) {
    println!("Summary:");
    println!("  Total Vertices: {}", geom.total_vertices());
    println!("  Total Triangles: {}", geom.total_triangles());
    println!("  Vertex Buffer: {} bytes", geom.vertex_buffer.len());
    println!("  Index Buffer: {} indices", geom.index_buffer.len());
    println!("  Rigid Only: {}", geom.rigid_only);
    println!("  All Sections Rigid: {}", geom.flags.all_sections_rigid);
    println!(
        "  All Sections Skinned: {}",
        geom.flags.all_sections_skinned
    );
}
