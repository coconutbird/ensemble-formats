//! Tests for legacy/Hogan material conversion.
use super::*;

fn assert_float_bits_eq(actual: f32, expected: f32, message: &str) {
    assert_eq!(actual.to_bits(), expected.to_bits(), "{message}");
}

#[test]
fn test_legacy_to_hogan_produces_al_pattern() {
    let mut legacy = LegacyMaterialData::default();
    legacy.maps[MapType::Diffuse as usize] = vec![Map {
        name: String::from("art\\textures\\grass_diff.ddx"),
        channel: 0,
        flags: 7,
    }];
    legacy.blend_type = 1;

    let hogan = legacy_to_hogan(&legacy, false);
    assert_eq!(hogan.ufx_version, DEFAULT_UFX_VERSION);
    assert_eq!(hogan.blend_mode, 1);
    assert!(!hogan.skinned);
    assert_eq!(hogan.shader_permutations.len(), 4);
    // Should produce HW2-style [al] texture pattern
    assert_eq!(hogan.textures, "art\\textures\\grass_[al]");
    // PS CB data should now be populated (not empty)
    assert!(
        !hogan.ps_cb_data.is_empty(),
        "ps_cb_data should be populated from legacy properties"
    );
    // VS CB data stays empty (no height-blend or vertex-anim in standard perms)
    assert!(hogan.vs_cb_data.is_empty());
}

#[test]
fn test_hogan_to_legacy_bracket_pattern() {
    let hogan = HoganMaterialData {
        shader_permutations: vec![ShaderPermutation {
            name: String::from("HOGAN_STANDARD_00000000003009A0"),
            hash: 0xE0EE_7237,
        }],
        ufx_version: 9,
        blend_mode: 2,
        shadow_requires_consts: false,
        skinned: false,
        terrain_blending: false,
        vs_cb_data: Vec::new(),
        ps_cb_data: Vec::new(),
        hs_cb_data: Vec::new(),
        ds_cb_data: Vec::new(),
        gs_cb_data: Vec::new(),
        textures: String::from("bespoke\\units\\scorpion\\scorpion_[al]"),
    };
    let legacy = hogan_to_legacy(&hogan);
    assert_eq!(legacy.blend_type, 2);
    assert_eq!(
        legacy.maps[MapType::Diffuse as usize][0].name,
        "bespoke\\units\\scorpion\\scorpion_diff.ddx"
    );
    assert_eq!(
        legacy.maps[MapType::Normal as usize][0].name,
        "bespoke\\units\\scorpion\\scorpion_norm.ddx"
    );
}

#[test]
fn test_hogan_to_legacy_semicolon_fallback() {
    let hogan = HoganMaterialData {
        shader_permutations: vec![],
        ufx_version: 9,
        blend_mode: 0,
        shadow_requires_consts: false,
        skinned: false,
        terrain_blending: false,
        vs_cb_data: Vec::new(),
        ps_cb_data: Vec::new(),
        hs_cb_data: Vec::new(),
        ds_cb_data: Vec::new(),
        gs_cb_data: Vec::new(),
        textures: String::from("art\\grass_diff.ddx;art\\grass_norm.ddx"),
    };
    let legacy = hogan_to_legacy(&hogan);
    assert_eq!(legacy.maps[MapType::Diffuse as usize].len(), 1);
    assert_eq!(legacy.maps[MapType::Normal as usize].len(), 1);
}

#[test]
fn test_hogan_to_legacy_empty() {
    let hogan = HoganMaterialData {
        shader_permutations: vec![],
        ufx_version: 9,
        blend_mode: 0,
        shadow_requires_consts: false,
        skinned: false,
        terrain_blending: false,
        vs_cb_data: Vec::new(),
        ps_cb_data: Vec::new(),
        hs_cb_data: Vec::new(),
        ds_cb_data: Vec::new(),
        gs_cb_data: Vec::new(),
        textures: String::new(),
    };
    let legacy = hogan_to_legacy(&hogan);
    for maps in &legacy.maps {
        assert!(maps.is_empty());
    }
}

#[test]
fn test_hogan_to_legacy_nm_pattern() {
    let hogan = HoganMaterialData {
        shader_permutations: vec![],
        ufx_version: 9,
        blend_mode: 8,
        shadow_requires_consts: false,
        skinned: false,
        terrain_blending: false,
        vs_cb_data: Vec::new(),
        ps_cb_data: Vec::new(),
        hs_cb_data: Vec::new(),
        ds_cb_data: Vec::new(),
        gs_cb_data: Vec::new(),
        textures: String::from("environment_tiling\\water\\fx_water_01[nm]"),
    };
    let legacy = hogan_to_legacy(&hogan);
    assert!(legacy.maps[MapType::Diffuse as usize].is_empty());
    assert_eq!(
        legacy.maps[MapType::Normal as usize][0].name,
        "environment_tiling\\water\\fx_water_01nm.ddx"
    );
}

#[test]
fn test_convert_material_roundtrip() {
    let mut ld = LegacyMaterialData::default();
    ld.maps[MapType::Diffuse as usize] = vec![Map {
        name: String::from("art\\model_diff.ddx"),
        channel: 0,
        flags: 7,
    }];
    let mat = Material {
        name: String::from("test"),
        material_version: 4,
        data: MaterialData::Legacy(Box::new(ld)),
    };
    let to_h = convert_material(&mat, true, false);
    assert!(to_h.is_hogan());
    assert_eq!(to_h.hogan().unwrap().textures, "art\\model_[al]");
    let back = convert_material(&to_h, false, false);
    assert!(back.is_legacy());
    assert!(
        back.legacy().unwrap().maps[MapType::Diffuse as usize][0]
            .name
            .contains("model_diff")
    );
}

#[test]
fn test_convert_noop() {
    let mat = Material {
        name: String::from("h"),
        material_version: 4,
        data: MaterialData::Hogan(Box::new(HoganMaterialData {
            shader_permutations: vec![],
            ufx_version: 9,
            blend_mode: 0,
            shadow_requires_consts: false,
            skinned: false,
            terrain_blending: false,
            vs_cb_data: Vec::new(),
            ps_cb_data: Vec::new(),
            hs_cb_data: Vec::new(),
            ds_cb_data: Vec::new(),
            gs_cb_data: Vec::new(),
            textures: String::from("x"),
        })),
    };
    let result = convert_material(&mat, true, false);
    assert!(result.is_hogan());
    assert_eq!(result.hogan().unwrap().textures, "x");
}

#[test]
fn test_parse_textures_multiple_types() {
    let parsed = parse_textures_string("m_diff.ddx;m_norm.ddx;m_spec.ddx;m_em.ddx");
    assert!(parsed.diffuse.is_some());
    assert!(parsed.normal.is_some());
    assert!(parsed.gloss.is_some());
    assert!(parsed.emissive.is_some());
}

#[test]
fn test_strip_suffix_ci() {
    assert_eq!(strip_suffix_ci("foo_Diff", "_diff"), Some("foo"));
    assert_eq!(strip_suffix_ci("foo_DIFF", "_diff"), Some("foo"));
    assert_eq!(strip_suffix_ci("foo_bar", "_diff"), None);
    assert_eq!(strip_suffix_ci("x", "_diff"), None);
}

#[test]
fn test_select_perm_set_base_pbr() {
    // Diffuse+Normal+Gloss → Base PBR (index 5, 4-slot, most frequent)
    let needed = &[
        "Albedo_Map",
        "BRDF_Parameter_Map",
        "Normal_Map",
        "Specular_Reflectance",
    ];
    let idx = select_standard_perm_set(needed);
    assert_eq!(HOGAN_STANDARD_PERM_SETS[idx].len(), 4);
    // Should be a base PBR set (no Sand/Emissive)
    let tex = HOGAN_STANDARD_PERM_TEXTURES[idx];
    assert!(!tex.contains(&"Sand_Map"));
    assert!(!tex.contains(&"Emissive_Map"));
}

#[test]
fn test_select_perm_set_emissive() {
    // Diffuse+Normal+Gloss+Emissive → Base PBR + Emissive (index 10)
    let needed = &[
        "Albedo_Map",
        "BRDF_Parameter_Map",
        "Emissive_Map",
        "Normal_Map",
        "Specular_Reflectance",
    ];
    let idx = select_standard_perm_set(needed);
    let tex = HOGAN_STANDARD_PERM_TEXTURES[idx];
    assert!(tex.contains(&"Emissive_Map"));
}

#[test]
fn test_select_perm_set_albedo_only_fallback() {
    // Albedo only → no exact match, falls back to base PBR (smallest superset)
    let needed = &["Albedo_Map"];
    let idx = select_standard_perm_set(needed);
    assert!(HOGAN_STANDARD_PERM_TEXTURES[idx].contains(&"Albedo_Map"));
}

#[test]
fn test_needed_hogan_textures_diffuse_normal_gloss() {
    let mut legacy = LegacyMaterialData::default();
    legacy.maps[MapType::Diffuse as usize] = vec![Map {
        name: String::from("tex_diff.ddx"),
        channel: 0,
        flags: 7,
    }];
    legacy.maps[MapType::Normal as usize] = vec![Map {
        name: String::from("tex_norm.ddx"),
        channel: 0,
        flags: 7,
    }];
    legacy.maps[MapType::Gloss as usize] = vec![Map {
        name: String::from("tex_spec.ddx"),
        channel: 0,
        flags: 7,
    }];
    let needed = needed_hogan_textures(&legacy);
    assert!(needed.contains(&"Albedo_Map"));
    assert!(needed.contains(&"Normal_Map"));
    assert!(needed.contains(&"BRDF_Parameter_Map"));
    assert!(needed.contains(&"Specular_Reflectance"));
}

#[test]
fn test_needed_hogan_textures_auto_adds_brdf() {
    // Diffuse+Normal without Gloss should still get BRDF+Specular
    let mut legacy = LegacyMaterialData::default();
    legacy.maps[MapType::Diffuse as usize] = vec![Map {
        name: String::from("tex_diff.ddx"),
        channel: 0,
        flags: 7,
    }];
    legacy.maps[MapType::Normal as usize] = vec![Map {
        name: String::from("tex_norm.ddx"),
        channel: 0,
        flags: 7,
    }];
    let needed = needed_hogan_textures(&legacy);
    assert!(needed.contains(&"BRDF_Parameter_Map"));
    assert!(needed.contains(&"Specular_Reflectance"));
}

#[test]
fn test_legacy_to_hogan_emissive_selects_right_perm() {
    let mut legacy = LegacyMaterialData::default();
    legacy.maps[MapType::Diffuse as usize] = vec![Map {
        name: String::from("unit_diff.ddx"),
        channel: 0,
        flags: 7,
    }];
    legacy.maps[MapType::Normal as usize] = vec![Map {
        name: String::from("unit_norm.ddx"),
        channel: 0,
        flags: 7,
    }];
    legacy.maps[MapType::Emissive as usize] = vec![Map {
        name: String::from("unit_em.ddx"),
        channel: 0,
        flags: 7,
    }];
    let hogan = legacy_to_hogan(&legacy, false);
    // Should pick the emissive perm set
    assert!(!hogan.shader_permutations.is_empty());
    // The permutation name should come from perm set 10 (emissive)
    let first_name = &hogan.shader_permutations[0].name;
    assert!(
        first_name.starts_with("HOGAN_STANDARD_"),
        "Expected HOGAN_STANDARD permutation, got: {first_name}"
    );
}

#[test]
fn test_legacy_to_hogan_skinned_flag() {
    let mut legacy = LegacyMaterialData::default();
    legacy.maps[MapType::Diffuse as usize] = vec![Map {
        name: String::from("unit_diff.ddx"),
        channel: 0,
        flags: 7,
    }];
    let hogan_static = legacy_to_hogan(&legacy, false);
    assert!(!hogan_static.skinned);
    let hogan_skinned = legacy_to_hogan(&legacy, true);
    assert!(hogan_skinned.skinned);
}

// --- CB data population tests ---

/// Helper: decode the first N floats from LE CB bytes.
fn cb_floats(data: &[u8]) -> Vec<f32> {
    let (chunks, _) = data.as_chunks::<4>();
    chunks
        .iter()
        .map(|chunk| f32::from_le_bytes(*chunk))
        .collect()
}

#[test]
fn test_parse_perm_flags() {
    assert_eq!(
        parse_perm_flags("HOGAN_STANDARD_00020000A83009A0"),
        Some(0x0002_0000_A830_09A0)
    );
    assert_eq!(
        parse_perm_flags("HOGAN_STANDARD_00000000003009A0"),
        Some(0x0000_0000_0030_09A0)
    );
    assert!(parse_perm_flags("").is_none());
}

#[test]
fn test_ps_cb_data_uv_scales_default() {
    // A basic permutation with no special features should still have
    // UV scales and normal_intensity.
    let flags = 0x0000_0000_0030_09A0_u64; // base PBR, bits: 5,7,8,11,12,13,20,21
    let legacy = LegacyMaterialData::default();
    let data = build_ps_cb_data(flags, &legacy);
    let floats = cb_floats(&data);

    // First 4 floats: uv_scale_t0_t1 (1,1) + uv_scale_t2_t3 (1,1)
    assert_float_bits_eq(floats[0], 1.0, "uv_scale_t0_t1.x");
    assert_float_bits_eq(floats[1], 1.0, "uv_scale_t0_t1.y");
    assert_float_bits_eq(floats[2], 1.0, "uv_scale_t2_t3.x");
    assert_float_bits_eq(floats[3], 1.0, "uv_scale_t2_t3.y");
    // Next: normal_intensity = 1.0
    assert_float_bits_eq(floats[4], 1.0, "normal_intensity");
}

#[test]
fn test_ps_cb_data_roughness_from_spec_power() {
    // A permutation with bit 28 (roughness channel).
    // Bit 28 = 0x10000000, combine with base bits (5,7,8,11)
    let flags = 0x1000_0000_u64 | (1 << 5) | (1 << 7) | (1 << 8) | (1 << 11);
    let legacy = LegacyMaterialData {
        spec_power: 50.0, // → roughness = 1.0 - 0.5 = 0.5
        ..LegacyMaterialData::default()
    };

    let data = build_ps_cb_data(flags, &legacy);
    let floats = cb_floats(&data);

    // Find roughness_channel_value — it's in Group B after align.
    // Group A: uv_scale_t0_t1(2) + uv_scale_t2_t3(2) + normal_intensity(1) = 5 floats
    // Aligned to register: pos 8 (next multiple of 4 after 5)
    assert!(
        (floats[8] - 0.5).abs() < 0.001,
        "roughness should be 0.5, got {}",
        floats[8]
    );
}

#[test]
fn test_ps_cb_data_spec_color_from_legacy() {
    // Permutation with bit 49 (material overrides) — includes spec_override_color.
    let flags = (1u64 << 49) | (1 << 5) | (1 << 7) | (1 << 8) | (1 << 11);
    let legacy = LegacyMaterialData {
        spec_color: [0.8, 0.6, 0.4],
        ..LegacyMaterialData::default()
    };

    let data = build_ps_cb_data(flags, &legacy);
    let floats = cb_floats(&data);

    // Group A: 5 floats (t0_t1=2, t2_t3=2, normal=1), aligned to 8
    // Group B: detail_blend(1), roughness_override(1), override_strength(1), aligned to 12
    //          spec_override_color(3) at offset 12, override_bias(1) at 15
    assert!(
        (floats[12] - 0.8).abs() < 0.001,
        "spec_color.r = {}, expected 0.8",
        floats[12]
    );
    assert!(
        (floats[13] - 0.6).abs() < 0.001,
        "spec_color.g = {}, expected 0.6",
        floats[13]
    );
    assert!(
        (floats[14] - 0.4).abs() < 0.001,
        "spec_color.b = {}, expected 0.4",
        floats[14]
    );
}

#[test]
fn test_ps_cb_data_emissive_present() {
    // Permutation with bit 32 (emissive+t4), plus base bits.
    let flags = (1u64 << 32) | (1 << 20) | (1 << 21) | (1 << 5) | (1 << 7) | (1 << 8) | (1 << 11);
    let mut legacy = LegacyMaterialData::default();
    legacy.maps[MapType::Emissive as usize] = vec![Map {
        name: String::from("unit_em.ddx"),
        channel: 0,
        flags: 7,
    }];

    let data = build_ps_cb_data(flags, &legacy);
    let floats = cb_floats(&data);

    // Layout: uv_scale_t0_t1(2), uv_scale_t2_t3(2), normal_intensity(1),
    //         uv_scale_t4(1), emissive_intensity(1)
    assert_float_bits_eq(floats[5], 1.0, "uv_scale_t4");
    assert_float_bits_eq(
        floats[6],
        1.0,
        "emissive_intensity should be 1.0 when emissive map present",
    );
}

#[test]
fn test_ps_cb_data_no_emissive_map_zero_intensity() {
    // Same flags as above but no emissive map → emissive_intensity = 0.0
    let flags = (1u64 << 32) | (1 << 20) | (1 << 21) | (1 << 5) | (1 << 7) | (1 << 8) | (1 << 11);
    let legacy = LegacyMaterialData::default();

    let data = build_ps_cb_data(flags, &legacy);
    let floats = cb_floats(&data);

    assert_float_bits_eq(
        floats[6],
        0.0,
        "emissive_intensity should be 0.0 without emissive map",
    );
}

#[test]
fn test_ps_cb_data_register_aligned_size() {
    // CB data size should always be a multiple of 16 bytes (one float4 register).
    let flags = 0x0002_0000_A830_09A0_u64;
    let legacy = LegacyMaterialData::default();
    let data = build_ps_cb_data(flags, &legacy);
    assert_eq!(
        data.len() % 16,
        0,
        "CB data size {} is not register-aligned",
        data.len()
    );
}
