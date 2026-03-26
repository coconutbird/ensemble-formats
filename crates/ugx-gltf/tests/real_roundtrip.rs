//! Real-file write→read roundtrip tests using game data.
//!
//! Opens ERA archives (HW1) or reads loose files (HW2) from game directories
//! configured via environment variables, then performs a full
//! read → export → import → write → re-read cycle to verify the writer
//! produces structurally valid UGX bytes for both versions.
//!
//! Set `HW1_GAME_DIR` and `HW2_GAME_DIR` in `.env` to enable these tests.
//! They are silently skipped when the dirs are absent.

use test_utils::prelude::*;
use ugx_gltf::{GltfExportOptions, GltfImportOptions, export_to_gltf, import_from_gltf};

/// Max UGX files to test per source (keeps test time reasonable).
const MAX_FILES: usize = 20;

// ---------------------------------------------------------------------------
// Roundtrip helper — only meaningful with real game data
// ---------------------------------------------------------------------------

enum RoundtripResult {
    Ok,
    /// Reader couldn't parse the original file (not a writer bug).
    ReadSkip(String),
    /// The roundtrip pipeline failed (writer/re-read bug).
    Fail(String),
}

fn roundtrip_ugx_bytes(label: &str, data: &[u8], version: ugx::UgxVersion) -> RoundtripResult {
    let original = match ugx::Reader::read(data) {
        Ok(g) => g,
        Err(e) => return RoundtripResult::ReadSkip(format!("{label}: read: {e}")),
    };

    macro_rules! fail {
        ($($t:tt)*) => { return RoundtripResult::Fail(format!($($t)*)) };
    }

    let export_opts = GltfExportOptions {
        embed_buffers: false,
        include_materials: true,
        include_skeleton: true,
    };

    let export = match export_to_gltf(&original, &export_opts) {
        Ok(e) => e,
        Err(e) => fail!("{label}: export: {e}"),
    };

    let import_opts = GltfImportOptions {
        version,
        include_skeleton: true,
        include_materials: true,
    };

    let imported = match import_from_gltf(&export.json, export.buffer.as_deref(), &import_opts) {
        Ok(g) => g,
        Err(e) => fail!("{label}: import: {e}"),
    };

    let ugx_bytes = match ugx::Writer::write(&imported, version) {
        Ok(b) => b,
        Err(e) => fail!("{label}: write: {e}"),
    };

    let re_read = match ugx::Reader::read(&ugx_bytes) {
        Ok(g) => g,
        Err(e) => fail!("{label}: re-read: {e}"),
    };

    // Structural checks
    if re_read.sections.len() != original.sections.len() {
        fail!(
            "{label}: section count: {} vs {}",
            re_read.sections.len(),
            original.sections.len()
        );
    }

    for (si, (o, r)) in original
        .sections
        .iter()
        .zip(re_read.sections.iter())
        .enumerate()
    {
        if r.num_verts != o.num_verts {
            fail!(
                "{label}: sec {si} verts: {} vs {}",
                r.num_verts,
                o.num_verts
            );
        }
        if r.num_tris != o.num_tris {
            fail!("{label}: sec {si} tris: {} vs {}", r.num_tris, o.num_tris);
        }
    }

    if re_read.bones.len() != original.bones.len() {
        fail!(
            "{label}: bones: {} vs {}",
            re_read.bones.len(),
            original.bones.len()
        );
    }

    // Material roundtrip
    if re_read.materials.len() != original.materials.len() {
        fail!(
            "{label}: material count: {} vs {}",
            re_read.materials.len(),
            original.materials.len()
        );
    }

    for (mi, (om, rm)) in original
        .materials
        .iter()
        .zip(re_read.materials.iter())
        .enumerate()
    {
        if rm.name != om.name {
            fail!(
                "{label}: material {mi} name: {:?} vs {:?}",
                rm.name,
                om.name
            );
        }

        if rm.blend_type != om.blend_type {
            fail!(
                "{label}: material {mi} blend_type: {} vs {}",
                rm.blend_type,
                om.blend_type
            );
        }

        // Opacity survives as u8 (0–255) so allow ±1/255 tolerance
        if (rm.opacity - om.opacity).abs() > (2.0 / 255.0) {
            fail!(
                "{label}: material {mi} opacity: {} vs {}",
                rm.opacity,
                om.opacity
            );
        }

        // Check texture map names survive for each map type
        for mt in ugx::MapType::ALL {
            let idx = mt as usize;
            if rm.maps[idx].len() != om.maps[idx].len() {
                fail!(
                    "{label}: material {mi} map {:?} count: {} vs {}",
                    mt,
                    rm.maps[idx].len(),
                    om.maps[idx].len()
                );
            }

            for (ti, (otex, rtex)) in om.maps[idx].iter().zip(rm.maps[idx].iter()).enumerate() {
                if rtex.name != otex.name {
                    fail!(
                        "{label}: material {mi} map {:?}[{ti}] name: {:?} vs {:?}",
                        mt,
                        rtex.name,
                        otex.name
                    );
                }
            }
        }
    }

    // Vertex positions
    for si in 0..original.sections.len() {
        let ov = match original.unpack_section_vertices(si) {
            Ok(v) => v,
            Err(e) => fail!("{label}: unpack orig sec {si}: {e}"),
        };

        let rv = match re_read.unpack_section_vertices(si) {
            Ok(v) => v,
            Err(e) => fail!("{label}: unpack reread sec {si}: {e}"),
        };

        if rv.len() != ov.len() {
            fail!(
                "{label}: sec {si} unpacked verts: {} vs {}",
                rv.len(),
                ov.len()
            );
        }

        for (vi, (a, b)) in ov.iter().zip(rv.iter()).enumerate() {
            // Positions
            for c in 0..3 {
                if (a.position[c] - b.position[c]).abs() > 0.05 {
                    fail!(
                        "{label}: sec {si} vert {vi} pos[{c}]: {} vs {}",
                        a.position[c],
                        b.position[c]
                    );
                }
            }

            // Normals — compare direction (normalized).
            // Original data may have non-unit normals (Dec3N edge cases),
            // while glTF normalizes to unit length during roundtrip.
            {
                let a_len = (a.normal[0] * a.normal[0]
                    + a.normal[1] * a.normal[1]
                    + a.normal[2] * a.normal[2])
                    .sqrt();
                let b_len = (b.normal[0] * b.normal[0]
                    + b.normal[1] * b.normal[1]
                    + b.normal[2] * b.normal[2])
                    .sqrt();
                if a_len > 1e-6 && b_len > 1e-6 {
                    for c in 0..3 {
                        let an = a.normal[c] / a_len;
                        let bn = b.normal[c] / b_len;
                        if (an - bn).abs() > 0.02 {
                            fail!(
                                "{label}: sec {si} vert {vi} normal_dir[{c}]: {} vs {} (raw: {} vs {})",
                                an,
                                bn,
                                a.normal[c],
                                b.normal[c]
                            );
                        }
                    }
                }
            }

            // Tangents — compare DIRECTION only (normalized), not magnitude.
            // Original data may store tangents at non-unit length (e.g. 0.5
            // via basis_scale), while the glTF roundtrip normalises to unit
            // length. The game engine only cares about direction.
            {
                let a_len = (a.tangent[0] * a.tangent[0]
                    + a.tangent[1] * a.tangent[1]
                    + a.tangent[2] * a.tangent[2])
                    .sqrt();
                let b_len = (b.tangent[0] * b.tangent[0]
                    + b.tangent[1] * b.tangent[1]
                    + b.tangent[2] * b.tangent[2])
                    .sqrt();
                if a_len > 1e-6 && b_len > 1e-6 {
                    for c in 0..3 {
                        let an = a.tangent[c] / a_len;
                        let bn = b.tangent[c] / b_len;
                        if (an - bn).abs() > 0.02 {
                            fail!(
                                "{label}: sec {si} vert {vi} tangent_dir[{c}]: {} vs {}",
                                an,
                                bn
                            );
                        }
                    }
                }
            }

            // UV coordinates (HalfFloat2 has ~1/1024 precision for values 0-1)
            let uv_count = a.num_texcoords.min(b.num_texcoords);
            for uvi in 0..uv_count {
                for c in 0..2 {
                    if (a.texcoords[uvi][c] - b.texcoords[uvi][c]).abs() > 0.01 {
                        fail!(
                            "{label}: sec {si} vert {vi} uv{uvi}[{c}]: {} vs {}",
                            a.texcoords[uvi][c],
                            b.texcoords[uvi][c]
                        );
                    }
                }
            }

            // Bone weights & indices.
            //
            // When the roundtrip detects a global_bones section (all vertices
            // bound to a single bone), it drops explicit skin data and uses
            // rigid_bone_index instead.  The re-read vertices then have zero
            // weights, which is semantically equivalent (weight = 1.0 on the
            // rigid bone).  Skip per-vertex weight/index comparison in that
            // case — the section-level rigid_bone_index already encodes it.
            let section_is_global =
                re_read.sections[si].global_bones || re_read.sections[si].rigid_only;

            if !section_is_global {
                for c in 0..4 {
                    if (a.bone_weights[c] - b.bone_weights[c]).abs() > 0.01 {
                        fail!(
                            "{label}: sec {si} vert {vi} bone_weight[{c}]: {} vs {}",
                            a.bone_weights[c],
                            b.bone_weights[c]
                        );
                    }
                }
            }

            // Bone indices — only check for slots with non-zero weight
            // (and only when not global_bones, where indices are implicit)
            if !section_is_global {
                for c in 0..4 {
                    if a.bone_weights[c] > 0.0 && a.bone_indices[c] != b.bone_indices[c] {
                        fail!(
                            "{label}: sec {si} vert {vi} bone_idx[{c}]: {} vs {} (weight={})",
                            a.bone_indices[c],
                            b.bone_indices[c],
                            a.bone_weights[c]
                        );
                    }
                }
            }
        }
    }

    // Section material_index mapping
    for (si, (os, rs)) in original
        .sections
        .iter()
        .zip(re_read.sections.iter())
        .enumerate()
    {
        if rs.material_index != os.material_index {
            fail!(
                "{label}: sec {si} material_index: {} vs {}",
                rs.material_index,
                os.material_index
            );
        }
    }

    // Bone hierarchy
    for (bi, (ob, rb)) in original.bones.iter().zip(re_read.bones.iter()).enumerate() {
        if rb.name != ob.name {
            fail!("{label}: bone {bi} name: {:?} vs {:?}", rb.name, ob.name);
        }

        if rb.parent_index != ob.parent_index {
            fail!(
                "{label}: bone {bi} parent: {} vs {}",
                rb.parent_index,
                ob.parent_index
            );
        }
    }

    // Granny bones (inverse world matrices)
    if re_read.granny_bones.len() != original.granny_bones.len() {
        fail!(
            "{label}: granny_bones count: {} vs {}",
            re_read.granny_bones.len(),
            original.granny_bones.len()
        );
    }

    for (bi, (og, rg)) in original
        .granny_bones
        .iter()
        .zip(re_read.granny_bones.iter())
        .enumerate()
    {
        if rg.name != og.name {
            fail!(
                "{label}: granny_bone {bi} name: {:?} vs {:?}",
                rg.name,
                og.name
            );
        }

        if rg.parent_index != og.parent_index {
            fail!(
                "{label}: granny_bone {bi} parent: {} vs {}",
                rg.parent_index,
                og.parent_index
            );
        }

        for row in 0..4 {
            for col in 0..4 {
                if (og.inverse_world_matrix.rows[row][col] - rg.inverse_world_matrix.rows[row][col])
                    .abs()
                    > 1e-4
                {
                    fail!(
                        "{label}: granny_bone {bi} mat[{row}][{col}]: {} vs {}",
                        og.inverse_world_matrix.rows[row][col],
                        rg.inverse_world_matrix.rows[row][col]
                    );
                }
            }
        }
    }

    // Material flags and UVW velocity
    for (mi, (om, rm)) in original
        .materials
        .iter()
        .zip(re_read.materials.iter())
        .enumerate()
    {
        if rm.flags != om.flags {
            fail!("{label}: material {mi} flags: {} vs {}", rm.flags, om.flags);
        }
        for (ti, (ov, rv)) in om
            .uvw_velocity
            .iter()
            .zip(rm.uvw_velocity.iter())
            .enumerate()
        {
            for c in 0..3 {
                if (ov[c] - rv[c]).abs() > 1e-4 {
                    fail!(
                        "{label}: material {mi} uvw_vel[{ti}][{c}]: {} vs {}",
                        ov[c],
                        rv[c]
                    );
                }
            }
        }
    }

    RoundtripResult::Ok
}

// ---------------------------------------------------------------------------
// HW1 — scan all .era files, extract UGX entries, roundtrip as v4
// ---------------------------------------------------------------------------

#[test]
fn test_hw1_era_roundtrip() {
    let game_dir = match load_game_dir("HW1_GAME_DIR") {
        Some(d) => d,
        None => return,
    };

    let era_paths = find_files_flat(&game_dir, "era");
    if era_paths.is_empty() {
        eprintln!("No .era files in {} — skipping", game_dir.display());
        return;
    }

    eprintln!(
        "Found {} ERA files in {}",
        era_paths.len(),
        game_dir.display()
    );

    let mut tested = 0usize;
    let mut skipped = 0usize;
    let mut errors = Vec::new();

    for era_path in &era_paths {
        let mut archive = match open_era(era_path) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("  SKIP {}: {e}", era_path.display());
                continue;
            }
        };

        let ugx_entries = find_entries_in_era(&archive, ".ugx");
        if ugx_entries.is_empty() {
            continue;
        }

        eprintln!(
            "  {} — {} UGX files",
            era_path.file_name().unwrap_or_default().to_string_lossy(),
            ugx_entries.len()
        );

        for (idx, filename) in &ugx_entries {
            if tested >= MAX_FILES {
                break;
            }

            let data = match archive.read_entry(*idx) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("    SKIP {filename}: decompress: {e}");
                    skipped += 1;
                    continue;
                }
            };

            eprint!("    {filename} ... ");
            match roundtrip_ugx_bytes(filename, &data, ugx::UgxVersion::Hw1) {
                RoundtripResult::Ok => {
                    eprintln!("OK");
                    tested += 1;
                }
                RoundtripResult::ReadSkip(e) => {
                    eprintln!("SKIP (reader)");
                    skipped += 1;
                    eprintln!("      {e}");
                }
                RoundtripResult::Fail(e) => {
                    eprintln!("FAIL");
                    errors.push(e);
                }
            }
        }

        if tested >= MAX_FILES {
            break;
        }
    }

    eprintln!(
        "\nHW1: {tested} ok, {skipped} skipped (reader), {} FAILED",
        errors.len()
    );

    assert!(tested > 0, "No HW1 UGX files found across any ERA");
    assert!(
        errors.is_empty(),
        "Writer roundtrip failures:\n{}",
        errors.join("\n")
    );
}

// ---------------------------------------------------------------------------
// HW2 — read loose .ugx files, roundtrip as v6
// ---------------------------------------------------------------------------

#[test]
fn test_hw2_loose_roundtrip() {
    let game_dir = match load_game_dir("HW2_GAME_DIR") {
        Some(d) => d,
        None => return,
    };

    let ugx_files = find_files_by_ext(&game_dir, "ugx");
    assert!(!ugx_files.is_empty(), "No UGX under {}", game_dir.display());

    eprintln!(
        "Found {} UGX files, testing up to {MAX_FILES}",
        ugx_files.len()
    );

    let mut tested = 0usize;
    let mut skipped = 0usize;
    let mut errors = Vec::new();
    for path in ugx_files.iter().take(MAX_FILES) {
        let label = path.display().to_string();
        let data = match std::fs::read(path) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("  SKIP {label}: {e}");
                skipped += 1;
                continue;
            }
        };

        eprint!(
            "  {} ... ",
            path.file_name().unwrap_or_default().to_string_lossy()
        );

        match roundtrip_ugx_bytes(&label, &data, ugx::UgxVersion::Hw2) {
            RoundtripResult::Ok => {
                eprintln!("OK");
                tested += 1;
            }
            RoundtripResult::ReadSkip(e) => {
                eprintln!("SKIP (reader)");
                skipped += 1;
                eprintln!("    {e}");
            }
            RoundtripResult::Fail(e) => {
                eprintln!("FAIL");
                errors.push(e);
            }
        }
    }

    eprintln!(
        "\nHW2: {tested} ok, {skipped} skipped (reader), {} FAILED",
        errors.len()
    );
    assert!(tested > 0, "No HW2 files tested");
    assert!(
        errors.is_empty(),
        "Writer roundtrip failures:\n{}",
        errors.join("\n")
    );
}

/// Diagnostic: survey pack orders across all real game files.
/// Run with: cargo test -p ugx-gltf --test real_roundtrip survey_pack_orders -- --ignored --nocapture
#[test]
#[ignore]
fn survey_pack_orders() {
    load_dotenv();
    use std::collections::BTreeMap;

    let mut pack_orders: BTreeMap<String, (usize, Vec<String>)> = BTreeMap::new();
    let mut add = |key: String, example: String| {
        let entry = pack_orders.entry(key).or_insert((0, Vec::new()));
        entry.0 += 1;
        if entry.1.len() < 3 {
            entry.1.push(example);
        }
    };

    // HW1 — scan all ERA files
    if let Some(hw1_dir) = load_game_dir("HW1_GAME_DIR") {
        let era_paths = find_files_flat(&hw1_dir, "era");
        for era_path in &era_paths {
            let mut archive = match open_era(era_path) {
                Ok(a) => a,
                Err(_) => continue,
            };
            let ugx_entries = find_entries_in_era(&archive, ".ugx");
            for (idx, filename) in &ugx_entries {
                let Ok(data) = archive.read_entry(*idx) else {
                    continue;
                };
                let Ok(geom) = ugx::UgxGeom::from_bytes(&data) else {
                    continue;
                };
                for (si, sec) in geom.sections.iter().enumerate() {
                    if let Some(ref packer) = sec.base_vert_packer {
                        let key = format!("HW1 pack_order={}", packer.pack_order);
                        add(key, format!("{}:s{}", filename, si));
                    }
                }
            }
        }
    }

    // HW2 — scan loose files (sample 500)
    if let Some(hw2_dir) = load_game_dir("HW2_GAME_DIR") {
        let ugx_files = find_files_by_ext(&hw2_dir, "ugx");
        for path in ugx_files.iter().take(500) {
            let Ok(data) = std::fs::read(path) else {
                continue;
            };
            let Ok(geom) = ugx::UgxGeom::from_bytes(&data) else {
                continue;
            };
            for (si, sec) in geom.sections.iter().enumerate() {
                let key = format!(
                    "HW2 vert_size={:2} rigid={} global_bones={}",
                    sec.vert_size, sec.rigid_only as u8, sec.global_bones as u8
                );
                let fname = path.file_name().unwrap().to_string_lossy().to_string();
                add(key, format!("{}:s{}", fname, si));
            }
        }
    }

    for (key, (count, examples)) in &pack_orders {
        eprintln!(
            "{}: {} sections  (e.g. {})",
            key,
            count,
            examples.join(", ")
        );
    }
}

/// Diagnostic: for files with B+X pack order, dump basis_scale values.
/// Run with: cargo test -p ugx-gltf --test real_roundtrip survey_basis_scale -- --ignored --nocapture
#[test]
#[ignore]
fn survey_basis_scale() {
    load_dotenv();

    let mut sections_with_bx = 0usize;
    let mut sections_with_b_no_x = 0usize;
    let mut sections_with_a = 0usize;
    let mut sections_with_n_only = 0usize;
    let mut scale_values: Vec<(String, f32, f32)> = Vec::new();

    if let Some(hw1_dir) = load_game_dir("HW1_GAME_DIR") {
        let era_paths = find_files_flat(&hw1_dir, "era");
        for era_path in &era_paths {
            let mut archive = match open_era(era_path) {
                Ok(a) => a,
                Err(_) => continue,
            };
            let ugx_entries = find_entries_in_era(&archive, ".ugx");
            for (idx, filename) in &ugx_entries {
                let Ok(data) = archive.read_entry(*idx) else {
                    continue;
                };
                let Ok(geom) = ugx::UgxGeom::from_bytes(&data) else {
                    continue;
                };
                for (si, sec) in geom.sections.iter().enumerate() {
                    let Some(ref packer) = sec.base_vert_packer else {
                        continue;
                    };
                    let po = &packer.pack_order;
                    if po.contains('X') {
                        sections_with_bx += 1;
                        if let Ok(verts) = geom.unpack_section_vertices(si) {
                            for v in verts.iter().take(3) {
                                if scale_values.len() < 40 {
                                    scale_values.push((
                                        format!("{}:s{}", filename, si),
                                        v.tangent[3],
                                        v.binormal[3],
                                    ));
                                }
                            }
                        }
                    } else if po.contains('B') {
                        sections_with_b_no_x += 1;
                    } else if po.contains('A') {
                        sections_with_a += 1;
                    } else {
                        sections_with_n_only += 1;
                    }
                }
            }
        }
    }

    eprintln!("HW1 sections with B+X (basis+scale): {}", sections_with_bx);
    eprintln!("HW1 sections with B (no X): {}", sections_with_b_no_x);
    eprintln!("HW1 sections with A (tangent-only): {}", sections_with_a);
    eprintln!(
        "HW1 sections with N only (no tangent): {}",
        sections_with_n_only
    );
    eprintln!();
    for (name, ts, bs) in &scale_values {
        eprintln!(
            "  {} tangent_scale={:.4} binormal_scale={:.4}",
            name, ts, bs
        );
    }
}

/// Diagnostic: measure normal/tangent magnitude distribution across real files.
/// Checks whether Dec3N quantization causes non-unit magnitudes and by how much.
/// Also compares original vs roundtripped magnitudes to quantify data loss.
/// Run with: cargo test -p ugx-gltf --test real_roundtrip survey_magnitudes -- --ignored --nocapture
#[test]
#[ignore]
fn survey_magnitudes() {
    load_dotenv();
    use ugx_gltf::{GltfExportOptions, GltfImportOptions, export_to_gltf, import_from_gltf};

    struct Stats {
        count: usize,
        min_mag: f32,
        max_mag: f32,
        sum_deviation: f64, // sum of |mag - 1.0|
        max_deviation: f32,
        non_unit_count: usize, // |mag - 1.0| > 0.01
    }

    impl Stats {
        fn new() -> Self {
            Self {
                count: 0,
                min_mag: f32::MAX,
                max_mag: f32::MIN,
                sum_deviation: 0.0,
                max_deviation: 0.0,
                non_unit_count: 0,
            }
        }
        fn record(&mut self, mag: f32) {
            self.count += 1;
            self.min_mag = self.min_mag.min(mag);
            self.max_mag = self.max_mag.max(mag);
            let dev = (mag - 1.0).abs();
            self.sum_deviation += dev as f64;
            self.max_deviation = self.max_deviation.max(dev);
            if dev > 0.01 {
                self.non_unit_count += 1;
            }
        }
        fn avg_deviation(&self) -> f64 {
            if self.count == 0 {
                0.0
            } else {
                self.sum_deviation / self.count as f64
            }
        }
    }

    fn mag3(v: &[f32; 3]) -> f32 {
        (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
    }
    fn mag3_from4(v: &[f32; 4]) -> f32 {
        (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
    }

    let mut orig_normal = Stats::new();
    let mut orig_tangent = Stats::new();
    let mut rt_normal = Stats::new();
    let mut rt_tangent = Stats::new();
    let mut direction_err_normal = Stats::new();
    let mut direction_err_tangent = Stats::new();
    let mut files_tested = 0usize;

    let export_opts = GltfExportOptions {
        embed_buffers: false,
        include_materials: false,
        include_skeleton: true,
    };

    // Collect files to process
    let mut files: Vec<(Vec<u8>, ugx::UgxVersion)> = Vec::new();

    if let Some(hw1_dir) = load_game_dir("HW1_GAME_DIR") {
        let era_paths = find_files_flat(&hw1_dir, "era");
        'hw1: for era_path in &era_paths {
            let mut archive = match open_era(era_path) {
                Ok(a) => a,
                Err(_) => continue,
            };
            let entries = find_entries_in_era(&archive, ".ugx");
            for (idx, _filename) in &entries {
                if files.len() >= 50 {
                    break 'hw1;
                }
                let Ok(data) = archive.read_entry(*idx) else {
                    continue;
                };
                files.push((data, ugx::UgxVersion::Hw1));
            }
        }
    }

    if let Some(hw2_dir) = load_game_dir("HW2_GAME_DIR") {
        let ugx_files = find_files_by_ext(&hw2_dir, "ugx");
        for path in ugx_files.iter().take(50) {
            let Ok(data) = std::fs::read(path) else {
                continue;
            };
            files.push((data, ugx::UgxVersion::Hw2));
        }
    }

    for (data, version) in &files {
        let Ok(geom) = ugx::UgxGeom::from_bytes(data) else {
            continue;
        };

        let Ok(export) = export_to_gltf(&geom, &export_opts) else {
            continue;
        };
        let import_opts = GltfImportOptions {
            version: *version,
            include_skeleton: true,
            include_materials: false,
        };
        let Ok(imported) = import_from_gltf(&export.json, export.buffer.as_deref(), &import_opts)
        else {
            continue;
        };
        let Ok(rt_bytes) = ugx::Writer::write(&imported, *version) else {
            continue;
        };
        let Ok(rt_geom) = ugx::UgxGeom::from_bytes(&rt_bytes) else {
            continue;
        };

        for si in 0..geom.sections.len().min(rt_geom.sections.len()) {
            let Ok(ov) = geom.unpack_section_vertices(si) else {
                continue;
            };
            let Ok(rv) = rt_geom.unpack_section_vertices(si) else {
                continue;
            };

            for (a, b) in ov.iter().zip(rv.iter()) {
                let om = mag3(&a.normal);
                let rm = mag3(&b.normal);
                if om > 1e-6 {
                    orig_normal.record(om);
                    rt_normal.record(rm);
                    if rm > 1e-6 {
                        let dot = (a.normal[0] * b.normal[0]
                            + a.normal[1] * b.normal[1]
                            + a.normal[2] * b.normal[2])
                            / (om * rm);
                        let angle_err = dot.clamp(-1.0, 1.0).acos().to_degrees();
                        direction_err_normal.record(angle_err);
                    }
                }

                let otm = mag3_from4(&a.tangent);
                let rtm = mag3_from4(&b.tangent);
                if otm > 1e-6 {
                    orig_tangent.record(otm);
                    rt_tangent.record(rtm);
                    if rtm > 1e-6 {
                        let dot = (a.tangent[0] * b.tangent[0]
                            + a.tangent[1] * b.tangent[1]
                            + a.tangent[2] * b.tangent[2])
                            / (otm * rtm);
                        let angle_err = dot.clamp(-1.0, 1.0).acos().to_degrees();
                        direction_err_tangent.record(angle_err);
                    }
                }
            }
        }
        files_tested += 1;
    }

    eprintln!(
        "\n=== Normal Magnitude Distribution ({} files, {} normals) ===",
        files_tested, orig_normal.count
    );
    eprintln!(
        "  Original:    min={:.6} max={:.6} avg_dev={:.6} max_dev={:.6} non_unit(>0.01)={}",
        orig_normal.min_mag,
        orig_normal.max_mag,
        orig_normal.avg_deviation(),
        orig_normal.max_deviation,
        orig_normal.non_unit_count
    );
    eprintln!(
        "  Roundtrip:   min={:.6} max={:.6} avg_dev={:.6} max_dev={:.6} non_unit(>0.01)={}",
        rt_normal.min_mag,
        rt_normal.max_mag,
        rt_normal.avg_deviation(),
        rt_normal.max_deviation,
        rt_normal.non_unit_count
    );
    eprintln!(
        "  Dir error:   avg={:.4}° max={:.4}° (>1°: {})",
        direction_err_normal.avg_deviation(),
        direction_err_normal.max_deviation,
        direction_err_normal.non_unit_count
    );

    eprintln!(
        "\n=== Tangent Magnitude Distribution ({} tangents) ===",
        orig_tangent.count
    );
    eprintln!(
        "  Original:    min={:.6} max={:.6} avg_dev={:.6} max_dev={:.6} non_unit(>0.01)={}",
        orig_tangent.min_mag,
        orig_tangent.max_mag,
        orig_tangent.avg_deviation(),
        orig_tangent.max_deviation,
        orig_tangent.non_unit_count
    );
    eprintln!(
        "  Roundtrip:   min={:.6} max={:.6} avg_dev={:.6} max_dev={:.6} non_unit(>0.01)={}",
        rt_tangent.min_mag,
        rt_tangent.max_mag,
        rt_tangent.avg_deviation(),
        rt_tangent.max_deviation,
        rt_tangent.non_unit_count
    );
    eprintln!(
        "  Dir error:   avg={:.4}° max={:.4}° (>1°: {})",
        direction_err_tangent.avg_deviation(),
        direction_err_tangent.max_deviation,
        direction_err_tangent.non_unit_count
    );
}

/// Diagnostic: verify whether normalizing before Dec3N packing produces
/// identical bytes to packing the original (non-unit) values.
/// Also checks if unpack→repack (no normalize) reproduces original raw bytes.
/// Run with: cargo test -p ugx-gltf --test real_roundtrip survey_dec3n_bytes -- --ignored --nocapture
#[test]
#[ignore]
fn survey_dec3n_bytes() {
    load_dotenv();
    use ugx::vertex::element::VertexElementType;

    fn pack_dec3n(v: [f32; 4]) -> [u8; 4] {
        let mut buf = Vec::new();
        VertexElementType::Dec3N.pack(&mut buf, v);
        [buf[0], buf[1], buf[2], buf[3]]
    }

    fn normalize3(v: [f32; 3]) -> [f32; 3] {
        let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if len < 1e-10 {
            return v;
        }
        [v[0] / len, v[1] / len, v[2] / len]
    }

    let mut total_normals = 0usize;
    let mut total_tangents = 0usize;
    // How many times does pack(original) == pack(normalized)?
    let mut normal_bytes_match = 0usize;
    let mut tangent_bytes_match = 0usize;
    // How many times does pack(original) == original_raw_bytes?
    let mut normal_repack_match = 0usize;
    let mut tangent_repack_match = 0usize;

    // We need raw bytes, so we read the vertex buffer directly
    let mut process_file = |data: &[u8]| {
        let Ok(geom) = ugx::UgxGeom::from_bytes(data) else {
            return;
        };

        for (si, sec) in geom.sections.iter().enumerate() {
            let vert_size = sec.vert_size as usize;
            let vb_start = sec.vb_offset as usize;

            // Determine normal/tangent byte offsets within each vertex.
            // For HW1 (has packer): parse pack_order to find offsets.
            // For HW2 (no packer): fixed layout — normal at +12, tangent at +16.
            let (normal_off, tangent_off) = if let Some(ref packer) = sec.base_vert_packer {
                // Only test Dec3N sections — Float3 sections have different byte layout
                if packer.normal_type != VertexElementType::Dec3N
                    || packer.tangent_type != VertexElementType::Dec3N
                {
                    continue;
                }
                // Walk pack_order to find byte offsets
                let mut off = 0usize;
                let mut n_off = None;
                let mut t_off = None;
                let mut chars = packer.pack_order.chars().peekable();
                while let Some(c) = chars.next() {
                    match c.to_ascii_uppercase() {
                        'P' => off += packer.pos_type.size(),
                        'N' => {
                            n_off = Some(off);
                            off += packer.normal_type.size();
                        }
                        'A' => {
                            chars.next();
                            t_off = Some(off);
                            off += packer.tangent_type.size();
                        }
                        'B' => {
                            chars.next();
                            off += packer.basis_type.size() * 2;
                        }
                        'X' => {
                            chars.next();
                            off += packer.basis_scale_type.size();
                        }
                        'T' => {
                            chars.next();
                            off += packer.uv_types[0].size();
                        }
                        'S' => {
                            off += packer.indices_type.size() + packer.weights_type.size();
                        }
                        'D' => off += packer.diffuse_type.size(),
                        'I' => off += packer.index_type.size(),
                        _ => {}
                    }
                }
                match (n_off, t_off) {
                    (Some(n), Some(t)) => (n, t),
                    _ => continue, // skip sections without both normal+tangent
                }
            } else {
                if vert_size < 20 {
                    continue;
                } // no normal/tangent
                (12, 16) // HW2 fixed: pos(8) + uv(4) + normal(4) + tangent(4)
            };

            let Ok(verts) = geom.unpack_section_vertices(si) else {
                continue;
            };

            for (vi, v) in verts.iter().enumerate() {
                let base = vb_start + vi * vert_size;

                // Original raw 4 bytes for normal
                let raw_n: [u8; 4] = geom.vertex_buffer[base + normal_off..base + normal_off + 4]
                    .try_into()
                    .unwrap();
                // Original raw 4 bytes for tangent
                let raw_t: [u8; 4] = geom.vertex_buffer[base + tangent_off..base + tangent_off + 4]
                    .try_into()
                    .unwrap();

                // Pack the unpacked (non-unit) value back
                let repacked_n = pack_dec3n([v.normal[0], v.normal[1], v.normal[2], 1.0]);
                let norm_n = normalize3(v.normal);
                let normalized_n = pack_dec3n([norm_n[0], norm_n[1], norm_n[2], 1.0]);

                total_normals += 1;
                if repacked_n == normalized_n {
                    normal_bytes_match += 1;
                }
                if repacked_n == raw_n {
                    normal_repack_match += 1;
                }

                let mag_t = (v.tangent[0] * v.tangent[0]
                    + v.tangent[1] * v.tangent[1]
                    + v.tangent[2] * v.tangent[2])
                    .sqrt();
                if mag_t > 1e-6 {
                    let repacked_t = pack_dec3n(v.tangent);
                    let norm_t = normalize3([v.tangent[0], v.tangent[1], v.tangent[2]]);
                    let normalized_t = pack_dec3n([norm_t[0], norm_t[1], norm_t[2], 1.0]);

                    total_tangents += 1;
                    if repacked_t == normalized_t {
                        tangent_bytes_match += 1;
                    }
                    if repacked_t == raw_t {
                        tangent_repack_match += 1;
                    }
                }
            }
        }
    };

    // HW1
    if let Some(hw1_dir) = load_game_dir("HW1_GAME_DIR") {
        let era_paths = find_files_flat(&hw1_dir, "era");
        let mut count = 0;
        'hw1: for era_path in &era_paths {
            let mut archive = match open_era(era_path) {
                Ok(a) => a,
                Err(_) => continue,
            };
            let entries = find_entries_in_era(&archive, ".ugx");
            for (idx, _) in &entries {
                if count >= 30 {
                    break 'hw1;
                }
                let Ok(data) = archive.read_entry(*idx) else {
                    continue;
                };
                process_file(&data);
                count += 1;
            }
        }
    }

    // HW2
    if let Some(hw2_dir) = load_game_dir("HW2_GAME_DIR") {
        let ugx_files = find_files_by_ext(&hw2_dir, "ugx");
        for path in ugx_files.iter().take(30) {
            let Ok(data) = std::fs::read(path) else {
                continue;
            };
            process_file(&data);
        }
    }

    eprintln!("\n=== Dec3N Packed Byte Comparison ===");
    eprintln!("Normals ({} total):", total_normals);
    eprintln!(
        "  pack(original) == pack(normalized): {} ({:.1}%)",
        normal_bytes_match,
        100.0 * normal_bytes_match as f64 / total_normals.max(1) as f64
    );
    eprintln!(
        "  pack(original) == raw_bytes:        {} ({:.1}%)",
        normal_repack_match,
        100.0 * normal_repack_match as f64 / total_normals.max(1) as f64
    );
    eprintln!("Tangents ({} total):", total_tangents);
    eprintln!(
        "  pack(original) == pack(normalized): {} ({:.1}%)",
        tangent_bytes_match,
        100.0 * tangent_bytes_match as f64 / total_tangents.max(1) as f64
    );
    eprintln!(
        "  pack(original) == raw_bytes:        {} ({:.1}%)",
        tangent_repack_match,
        100.0 * tangent_repack_match as f64 / total_tangents.max(1) as f64
    );
}

/// Diagnostic: check what bits 30-31 of Dec3N packed values actually contain,
/// and how many values use -512 (which unpacks to -1.00196, outside [-1,1]).
/// Run with: cargo test -p ugx-gltf --test real_roundtrip survey_dec3n_w_bits -- --ignored --nocapture
#[test]
#[ignore]
fn survey_dec3n_w_bits() {
    load_dotenv();

    let mut total_normals = 0usize;
    let mut total_tangents = 0usize;
    // Bits 30-31 value distribution for normals
    let mut normal_w_vals = [0usize; 4]; // 0b00, 0b01, 0b10, 0b11
    let mut tangent_w_vals = [0usize; 4];
    // Count of -512 values per component
    let mut normal_neg512_count = 0usize;
    let mut tangent_neg512_count = 0usize;
    // Count of components > 1.0 after unpack (i.e. -512 case)
    let mut normal_oor_count = 0usize; // out-of-range
    let mut tangent_oor_count = 0usize;

    let mut process_file = |data: &[u8]| {
        let Ok(geom) = ugx::UgxGeom::from_bytes(data) else {
            return;
        };

        for (si, sec) in geom.sections.iter().enumerate() {
            let vert_size = sec.vert_size as usize;
            let vb_start = sec.vb_offset as usize;

            let (normal_off, tangent_off) = if let Some(ref packer) = sec.base_vert_packer {
                let mut off = 0usize;
                let mut n_off = None;
                let mut t_off = None;
                let mut chars = packer.pack_order.chars().peekable();
                while let Some(c) = chars.next() {
                    match c.to_ascii_uppercase() {
                        'P' => off += packer.pos_type.size(),
                        'N' => {
                            n_off = Some(off);
                            off += packer.normal_type.size();
                        }
                        'A' => {
                            chars.next();
                            t_off = Some(off);
                            off += packer.tangent_type.size();
                        }
                        'B' => {
                            chars.next();
                            off += packer.basis_type.size() * 2;
                        }
                        'X' => {
                            chars.next();
                            off += packer.basis_scale_type.size();
                        }
                        'T' => {
                            chars.next();
                            off += packer.uv_types[0].size();
                        }
                        'S' => {
                            off += packer.indices_type.size() + packer.weights_type.size();
                        }
                        'D' => off += packer.diffuse_type.size(),
                        'I' => off += packer.index_type.size(),
                        _ => {}
                    }
                }
                match (n_off, t_off) {
                    (Some(n), Some(t)) => (n, t),
                    _ => continue,
                }
            } else {
                if vert_size < 20 {
                    continue;
                }
                (12, 16)
            };

            let Ok(verts) = geom.unpack_section_vertices(si) else {
                continue;
            };
            let num_verts = sec.num_verts as usize;

            for vi in 0..num_verts.min(verts.len()) {
                let base = vb_start + vi * vert_size;
                if base + normal_off + 4 > geom.vertex_buffer.len() {
                    break;
                }
                if base + tangent_off + 4 > geom.vertex_buffer.len() {
                    break;
                }

                // Normal
                let raw_n = u32::from_le_bytes(
                    geom.vertex_buffer[base + normal_off..base + normal_off + 4]
                        .try_into()
                        .unwrap(),
                );
                let w_n = (raw_n >> 30) & 0x3;
                normal_w_vals[w_n as usize] += 1;
                total_normals += 1;

                // Check each 10-bit component for -512
                for shift in [0, 10, 20] {
                    let raw10 = ((raw_n >> shift) & 0x3FF) as i32;
                    let extended = if raw10 & 0x200 != 0 {
                        raw10 | !0x3FF
                    } else {
                        raw10
                    };
                    if extended == -512 {
                        normal_neg512_count += 1;
                    }
                    let val = extended as f32 / 511.0;
                    if val.abs() > 1.0 {
                        normal_oor_count += 1;
                    }
                }

                // Tangent
                let raw_t = u32::from_le_bytes(
                    geom.vertex_buffer[base + tangent_off..base + tangent_off + 4]
                        .try_into()
                        .unwrap(),
                );
                let w_t = (raw_t >> 30) & 0x3;
                tangent_w_vals[w_t as usize] += 1;
                total_tangents += 1;

                for shift in [0, 10, 20] {
                    let raw10 = ((raw_t >> shift) & 0x3FF) as i32;
                    let extended = if raw10 & 0x200 != 0 {
                        raw10 | !0x3FF
                    } else {
                        raw10
                    };
                    if extended == -512 {
                        tangent_neg512_count += 1;
                    }
                    let val = extended as f32 / 511.0;
                    if val.abs() > 1.0 {
                        tangent_oor_count += 1;
                    }
                }
            }
        }
    };

    // HW1
    if let Some(hw1_dir) = load_game_dir("HW1_GAME_DIR") {
        let era_paths = find_files_flat(&hw1_dir, "era");
        let mut count = 0;
        'hw1: for era_path in &era_paths {
            let mut archive = match open_era(era_path) {
                Ok(a) => a,
                Err(_) => continue,
            };
            let entries = find_entries_in_era(&archive, ".ugx");
            for (idx, _) in &entries {
                if count >= 50 {
                    break 'hw1;
                }
                let Ok(data) = archive.read_entry(*idx) else {
                    continue;
                };
                process_file(&data);
                count += 1;
            }
        }
    }

    // HW2
    if let Some(hw2_dir) = load_game_dir("HW2_GAME_DIR") {
        let ugx_files = find_files_by_ext(&hw2_dir, "ugx");
        for path in ugx_files.iter().take(50) {
            let Ok(data) = std::fs::read(path) else {
                continue;
            };
            process_file(&data);
        }
    }

    eprintln!("\n=== Dec3N Bits 30-31 (W field) Distribution ===");
    eprintln!("Normals ({} total):", total_normals);
    eprintln!(
        "  W=0b00: {} ({:.1}%)",
        normal_w_vals[0],
        100.0 * normal_w_vals[0] as f64 / total_normals.max(1) as f64
    );
    eprintln!(
        "  W=0b01: {} ({:.1}%)",
        normal_w_vals[1],
        100.0 * normal_w_vals[1] as f64 / total_normals.max(1) as f64
    );
    eprintln!(
        "  W=0b10: {} ({:.1}%)",
        normal_w_vals[2],
        100.0 * normal_w_vals[2] as f64 / total_normals.max(1) as f64
    );
    eprintln!(
        "  W=0b11: {} ({:.1}%)",
        normal_w_vals[3],
        100.0 * normal_w_vals[3] as f64 / total_normals.max(1) as f64
    );
    eprintln!(
        "  Components == -512: {} ({:.3}% of 3*normals)",
        normal_neg512_count,
        100.0 * normal_neg512_count as f64 / (3 * total_normals).max(1) as f64
    );
    eprintln!(
        "  Components |val| > 1.0: {} ({:.3}%)",
        normal_oor_count,
        100.0 * normal_oor_count as f64 / (3 * total_normals).max(1) as f64
    );

    eprintln!("\nTangents ({} total):", total_tangents);
    eprintln!(
        "  W=0b00: {} ({:.1}%)",
        tangent_w_vals[0],
        100.0 * tangent_w_vals[0] as f64 / total_tangents.max(1) as f64
    );
    eprintln!(
        "  W=0b01: {} ({:.1}%)",
        tangent_w_vals[1],
        100.0 * tangent_w_vals[1] as f64 / total_tangents.max(1) as f64
    );
    eprintln!(
        "  W=0b10: {} ({:.1}%)",
        tangent_w_vals[2],
        100.0 * tangent_w_vals[2] as f64 / total_tangents.max(1) as f64
    );
    eprintln!(
        "  W=0b11: {} ({:.1}%)",
        tangent_w_vals[3],
        100.0 * tangent_w_vals[3] as f64 / total_tangents.max(1) as f64
    );
    eprintln!(
        "  Components == -512: {} ({:.3}% of 3*tangents)",
        tangent_neg512_count,
        100.0 * tangent_neg512_count as f64 / (3 * total_tangents).max(1) as f64
    );
    eprintln!(
        "  Components |val| > 1.0: {} ({:.3}%)",
        tangent_oor_count,
        100.0 * tangent_oor_count as f64 / (3 * total_tangents).max(1) as f64
    );
}

// ---------------------------------------------------------------------------
// Diagnostic: launcher_01.ugx roundtrip diff
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn diagnose_launcher_01() {
    let data = match std::fs::read("../../launcher_01.ugx") {
        Ok(d) => d,
        Err(_) => {
            eprintln!("launcher_01.ugx not found in repo root — skipping");
            return;
        }
    };

    let original = ugx::Reader::read(&data).unwrap();
    // Detect version from signature in first 4 bytes of cached data (offset depends on ECF)
    // Use the section packer presence as proxy: HW1 has packers, HW2 doesn't
    let version = if original
        .sections
        .first()
        .is_some_and(|s| s.base_vert_packer.is_some())
    {
        ugx::UgxVersion::Hw1
    } else {
        ugx::UgxVersion::Hw2
    };
    eprintln!("Version: {:?}", version);

    eprintln!("\n=== ORIGINAL GEOM ===");
    eprintln!(
        "Sections:{} Mats:{} Bones:{} GrannyBones:{} GrannyMeshes:{}",
        original.sections.len(),
        original.materials.len(),
        original.bones.len(),
        original.granny_bones.len(),
        original.granny_meshes.len()
    );
    eprintln!(
        "Accessories:{} ValidAcc:{} AABB:{}",
        original.accessories.len(),
        original.valid_accessories.len(),
        original.aabb_tree.is_some()
    );
    eprintln!(
        "rigid_only={} rigid_bone_index={} max_inst={} iim={} lgbi={}",
        original.rigid_only,
        original.rigid_bone_index,
        original.max_instances,
        original.instance_index_multiplier,
        original.large_geom_bone_index
    );
    eprintln!(
        "all_sections_rigid={} all_sections_skinned={} global_bones={}",
        original.all_sections_rigid, original.all_sections_skinned, original.global_bones
    );
    eprintln!(
        "VB:{} IB:{}",
        original.vertex_buffer.len(),
        original.index_buffer.len()
    );

    for (si, s) in original.sections.iter().enumerate() {
        eprintln!(
            "  sec[{}]: mat={} acc={} maxB={} rigB={} ibO={} tri={} vbO={} vbB={} vSz={} nV={} packer={} remap={:?} rig={} glb={}",
            si,
            s.material_index,
            s.accessory_index,
            s.max_bones,
            s.rigid_bone_index,
            s.ib_offset,
            s.num_tris,
            s.vb_offset,
            s.vb_bytes,
            s.vert_size,
            s.num_verts,
            s.base_vert_packer.is_some(),
            s.bone_remap,
            s.rigid_only,
            s.global_bones
        );
    }

    let export_opts = GltfExportOptions {
        embed_buffers: false,
        include_materials: true,
        include_skeleton: true,
    };
    let export = export_to_gltf(&original, &export_opts).unwrap();
    let import_opts = GltfImportOptions {
        version,
        include_skeleton: true,
        include_materials: true,
    };
    let imported = import_from_gltf(&export.json, export.buffer.as_deref(), &import_opts).unwrap();
    let rt_bytes = ugx::Writer::write(&imported, version).unwrap();

    eprintln!("\n=== ROUNDTRIPPED ===");
    eprintln!(
        "Sections:{} Mats:{} Bones:{} GrannyBones:{} GrannyMeshes:{}",
        imported.sections.len(),
        imported.materials.len(),
        imported.bones.len(),
        imported.granny_bones.len(),
        imported.granny_meshes.len()
    );
    eprintln!(
        "Accessories:{} ValidAcc:{} AABB:{}",
        imported.accessories.len(),
        imported.valid_accessories.len(),
        imported.aabb_tree.is_some()
    );
    eprintln!(
        "rigid_only={} rigid_bone_index={} max_inst={} iim={} lgbi={}",
        imported.rigid_only,
        imported.rigid_bone_index,
        imported.max_instances,
        imported.instance_index_multiplier,
        imported.large_geom_bone_index
    );

    for (si, s) in imported.sections.iter().enumerate() {
        eprintln!(
            "  sec[{}]: mat={} acc={} maxB={} rigB={} ibO={} tri={} vbO={} vbB={} vSz={} nV={} packer={} remap={:?} rig={} glb={}",
            si,
            s.material_index,
            s.accessory_index,
            s.max_bones,
            s.rigid_bone_index,
            s.ib_offset,
            s.num_tris,
            s.vb_offset,
            s.vb_bytes,
            s.vert_size,
            s.num_verts,
            s.base_vert_packer.is_some(),
            s.bone_remap,
            s.rigid_only,
            s.global_bones
        );
    }

    // Field-level diffs
    eprintln!("\n=== FIELD DIFFS ===");
    if original.rigid_only != imported.rigid_only {
        eprintln!(
            "DIFF rigid_only: {} -> {}",
            original.rigid_only, imported.rigid_only
        );
    }
    if original.rigid_bone_index != imported.rigid_bone_index {
        eprintln!(
            "DIFF rigid_bone_index: {} -> {}",
            original.rigid_bone_index, imported.rigid_bone_index
        );
    }
    if original.max_instances != imported.max_instances {
        eprintln!(
            "DIFF max_instances: {} -> {}",
            original.max_instances, imported.max_instances
        );
    }
    if original.instance_index_multiplier != imported.instance_index_multiplier {
        eprintln!(
            "DIFF iim: {} -> {}",
            original.instance_index_multiplier, imported.instance_index_multiplier
        );
    }
    if original.large_geom_bone_index != imported.large_geom_bone_index {
        eprintln!(
            "DIFF large_geom_bone_index: {} -> {}",
            original.large_geom_bone_index, imported.large_geom_bone_index
        );
    }
    if original.all_sections_rigid != imported.all_sections_rigid {
        eprintln!(
            "DIFF all_sections_rigid: {} -> {}",
            original.all_sections_rigid, imported.all_sections_rigid
        );
    }
    if original.all_sections_skinned != imported.all_sections_skinned {
        eprintln!(
            "DIFF all_sections_skinned: {} -> {}",
            original.all_sections_skinned, imported.all_sections_skinned
        );
    }
    if original.global_bones != imported.global_bones {
        eprintln!(
            "DIFF global_bones: {} -> {}",
            original.global_bones, imported.global_bones
        );
    }
    if original.bones.len() != imported.bones.len() {
        eprintln!(
            "DIFF bones count: {} -> {}",
            original.bones.len(),
            imported.bones.len()
        );
    }
    if original.granny_bones.len() != imported.granny_bones.len() {
        eprintln!(
            "DIFF granny_bones count: {} -> {}",
            original.granny_bones.len(),
            imported.granny_bones.len()
        );
    }
    if original.granny_meshes.len() != imported.granny_meshes.len() {
        eprintln!(
            "DIFF granny_meshes count: {} -> {}",
            original.granny_meshes.len(),
            imported.granny_meshes.len()
        );
    }
    if original.accessories.len() != imported.accessories.len() {
        eprintln!(
            "DIFF accessories count: {} -> {}",
            original.accessories.len(),
            imported.accessories.len()
        );
    }

    for (si, (os, rs)) in original
        .sections
        .iter()
        .zip(imported.sections.iter())
        .enumerate()
    {
        if os.material_index != rs.material_index {
            eprintln!(
                "DIFF sec[{}] mat: {} -> {}",
                si, os.material_index, rs.material_index
            );
        }
        if os.accessory_index != rs.accessory_index {
            eprintln!(
                "DIFF sec[{}] acc: {} -> {}",
                si, os.accessory_index, rs.accessory_index
            );
        }
        if os.max_bones != rs.max_bones {
            eprintln!(
                "DIFF sec[{}] maxBones: {} -> {}",
                si, os.max_bones, rs.max_bones
            );
        }
        if os.rigid_bone_index != rs.rigid_bone_index {
            eprintln!(
                "DIFF sec[{}] rigidBone: {} -> {}",
                si, os.rigid_bone_index, rs.rigid_bone_index
            );
        }
        if os.vert_size != rs.vert_size {
            eprintln!(
                "DIFF sec[{}] vert_size: {} -> {}",
                si, os.vert_size, rs.vert_size
            );
        }
        if os.bone_remap != rs.bone_remap {
            eprintln!(
                "DIFF sec[{}] bone_remap: {:?} -> {:?}",
                si, os.bone_remap, rs.bone_remap
            );
        }
        if os.rigid_only != rs.rigid_only {
            eprintln!(
                "DIFF sec[{}] rigid_only: {} -> {}",
                si, os.rigid_only, rs.rigid_only
            );
        }
        if os.global_bones != rs.global_bones {
            eprintln!(
                "DIFF sec[{}] global_bones: {} -> {}",
                si, os.global_bones, rs.global_bones
            );
        }
    }

    // Check vertex data - bone weights/indices
    eprintln!("\n=== VERTEX SKINNING CHECK ===");
    for si in 0..original.sections.len().min(imported.sections.len()) {
        let orig_verts = original.unpack_section_vertices(si).unwrap();
        let rt_verts = imported.unpack_section_vertices(si).unwrap();
        let mut orig_skinned = 0;
        let mut rt_skinned = 0;
        for v in &orig_verts {
            if v.bone_weights.iter().any(|&w| w > 0.0) {
                orig_skinned += 1;
            }
        }
        for v in &rt_verts {
            if v.bone_weights.iter().any(|&w| w > 0.0) {
                rt_skinned += 1;
            }
        }
        eprintln!(
            "  sec[{}]: orig_skinned={}/{} rt_skinned={}/{}",
            si,
            orig_skinned,
            orig_verts.len(),
            rt_skinned,
            rt_verts.len()
        );

        // Show first skinned vertex from original
        if let Some(v) = orig_verts
            .iter()
            .find(|v| v.bone_weights.iter().any(|&w| w > 0.0))
        {
            eprintln!(
                "    orig sample: indices={:?} weights={:?}",
                v.bone_indices, v.bone_weights
            );
        }
        if let Some(v) = rt_verts
            .iter()
            .find(|v| v.bone_weights.iter().any(|&w| w > 0.0))
        {
            eprintln!(
                "    rt   sample: indices={:?} weights={:?}",
                v.bone_indices, v.bone_weights
            );
        }

        // Show packer info
        if let Some(ref p) = original.sections[si].base_vert_packer {
            eprintln!(
                "    orig packer: pack='{}' decl='{}'",
                p.pack_order, p.decl_order
            );
            eprintln!(
                "    types: pos={:?} norm={:?} tan={:?} idx={:?} wt={:?}",
                p.pos_type, p.normal_type, p.tangent_type, p.indices_type, p.weights_type
            );
        }
        if let Some(ref p) = imported.sections[si].base_vert_packer {
            eprintln!(
                "    rt   packer: pack='{}' decl='{}'",
                p.pack_order, p.decl_order
            );
            eprintln!(
                "    types: pos={:?} norm={:?} tan={:?} idx={:?} wt={:?}",
                p.pos_type, p.normal_type, p.tangent_type, p.indices_type, p.weights_type
            );
        }
    }

    // Byte-level diff of full files
    eprintln!("\n=== BYTE-LEVEL DIFF ===");
    let min_len = data.len().min(rt_bytes.len());
    let mut diffs = 0usize;
    for i in 0..min_len {
        if data[i] != rt_bytes[i] {
            diffs += 1;
        }
    }
    diffs += data.len().abs_diff(rt_bytes.len());
    eprintln!(
        "{} byte diffs out of {} (orig={} rt={})",
        diffs,
        data.len().max(rt_bytes.len()),
        data.len(),
        rt_bytes.len()
    );
    // Show first 20 diffs
    let mut shown = 0;
    for i in 0..min_len {
        if data[i] != rt_bytes[i] {
            eprintln!("  @0x{:06X}: 0x{:02X} -> 0x{:02X}", i, data[i], rt_bytes[i]);
            shown += 1;
            if shown >= 20 {
                break;
            }
        }
    }

    eprintln!("\nTotal: orig={} rt={}", data.len(), rt_bytes.len());
    std::fs::write("launcher_01_rt.ugx", &rt_bytes).unwrap();
    eprintln!("Wrote launcher_01_rt.ugx");
}

#[test]
#[ignore]
fn diagnose_hw1_vanilla() {
    let game_dir = match load_game_dir("HW1_GAME_DIR") {
        Some(d) => d,
        None => {
            eprintln!("HW1_GAME_DIR not set — skipping");
            return;
        }
    };
    let era_paths = find_files_flat(&game_dir, "era");

    // Find a skinned UGX (e.g. marine) from the first ERA that has one
    let mut found = None;
    for era_path in &era_paths {
        let mut archive = match open_era(era_path) {
            Ok(a) => a,
            Err(_) => continue,
        };
        let entries = find_entries_in_era(&archive, ".ugx");
        for (idx, filename) in &entries {
            if (filename.contains("marine")
                || filename.contains("warthog")
                || filename.contains("scorpion"))
                && let Ok(data) = archive.read_entry(*idx)
            {
                eprintln!(
                    "Using: {} from {}",
                    filename,
                    era_path.file_name().unwrap().to_string_lossy()
                );
                found = Some((filename.clone(), data));
                break;
            }
        }
        if found.is_some() {
            break;
        }
    }

    let (filename, data) = match found {
        Some(f) => f,
        None => {
            eprintln!("No suitable HW1 file found");
            return;
        }
    };

    let original = ugx::Reader::read(&data).unwrap();
    eprintln!("\n=== ORIGINAL ({}) ===", filename);
    eprintln!(
        "Sections:{} Mats:{} Bones:{}",
        original.sections.len(),
        original.materials.len(),
        original.bones.len()
    );
    eprintln!(
        "rigid_only={} rigid_bone_index={} max_inst={} iim={} lgbi={}",
        original.rigid_only,
        original.rigid_bone_index,
        original.max_instances,
        original.instance_index_multiplier,
        original.large_geom_bone_index
    );
    eprintln!(
        "all_sections_rigid={} all_sections_skinned={} global_bones={}",
        original.all_sections_rigid, original.all_sections_skinned, original.global_bones
    );

    for (si, s) in original.sections.iter().enumerate() {
        eprintln!(
            "  sec[{}]: vSz={} glb={} rig={} rigB={} maxB={} nV={} remap={:?}",
            si,
            s.vert_size,
            s.global_bones,
            s.rigid_only,
            s.rigid_bone_index,
            s.max_bones,
            s.num_verts,
            s.bone_remap
        );
        if let Some(ref p) = s.base_vert_packer {
            eprintln!(
                "    packer: pack='{}' pos={:?} norm={:?} tan={:?} uv0={:?} idx={:?} wt={:?}",
                p.pack_order,
                p.pos_type,
                p.normal_type,
                p.tangent_type,
                p.uv_types[0],
                p.indices_type,
                p.weights_type
            );
        }
    }

    let export_opts = GltfExportOptions {
        embed_buffers: false,
        include_materials: true,
        include_skeleton: true,
    };
    let export = export_to_gltf(&original, &export_opts).unwrap();
    let import_opts = GltfImportOptions {
        version: ugx::UgxVersion::Hw1,
        include_skeleton: true,
        include_materials: true,
    };
    let imported = import_from_gltf(&export.json, export.buffer.as_deref(), &import_opts).unwrap();
    let rt_bytes = ugx::Writer::write(&imported, ugx::UgxVersion::Hw1).unwrap();
    let re_read = ugx::Reader::read(&rt_bytes).unwrap();

    eprintln!("\n=== ROUNDTRIPPED ===");
    eprintln!(
        "Sections:{} Mats:{} Bones:{}",
        re_read.sections.len(),
        re_read.materials.len(),
        re_read.bones.len()
    );
    eprintln!(
        "rigid_only={} rigid_bone_index={} max_inst={} iim={} lgbi={}",
        re_read.rigid_only,
        re_read.rigid_bone_index,
        re_read.max_instances,
        re_read.instance_index_multiplier,
        re_read.large_geom_bone_index
    );
    eprintln!(
        "all_sections_rigid={} all_sections_skinned={} global_bones={}",
        re_read.all_sections_rigid, re_read.all_sections_skinned, re_read.global_bones
    );

    for (si, s) in re_read.sections.iter().enumerate() {
        eprintln!(
            "  sec[{}]: vSz={} glb={} rig={} rigB={} maxB={} nV={}",
            si,
            s.vert_size,
            s.global_bones,
            s.rigid_only,
            s.rigid_bone_index,
            s.max_bones,
            s.num_verts
        );
        if let Some(ref p) = s.base_vert_packer {
            eprintln!(
                "    packer: pack='{}' pos={:?} norm={:?} tan={:?}",
                p.pack_order, p.pos_type, p.normal_type, p.tangent_type
            );
        }
    }

    eprintln!("\n=== DIFFS ===");
    if original.rigid_only != re_read.rigid_only {
        eprintln!(
            "DIFF rigid_only: {} -> {}",
            original.rigid_only, re_read.rigid_only
        );
    }
    if original.rigid_bone_index != re_read.rigid_bone_index {
        eprintln!(
            "DIFF rigid_bone_index: {} -> {}",
            original.rigid_bone_index, re_read.rigid_bone_index
        );
    }
    if original.max_instances != re_read.max_instances {
        eprintln!(
            "DIFF max_instances: {} -> {}",
            original.max_instances, re_read.max_instances
        );
    }
    if original.instance_index_multiplier != re_read.instance_index_multiplier {
        eprintln!(
            "DIFF iim: {} -> {}",
            original.instance_index_multiplier, re_read.instance_index_multiplier
        );
    }
    if original.all_sections_rigid != re_read.all_sections_rigid {
        eprintln!(
            "DIFF all_sections_rigid: {} -> {}",
            original.all_sections_rigid, re_read.all_sections_rigid
        );
    }
    if original.all_sections_skinned != re_read.all_sections_skinned {
        eprintln!(
            "DIFF all_sections_skinned: {} -> {}",
            original.all_sections_skinned, re_read.all_sections_skinned
        );
    }
    if original.global_bones != re_read.global_bones {
        eprintln!(
            "DIFF global_bones: {} -> {}",
            original.global_bones, re_read.global_bones
        );
    }
    for (si, (os, rs)) in original
        .sections
        .iter()
        .zip(re_read.sections.iter())
        .enumerate()
    {
        if os.vert_size != rs.vert_size {
            eprintln!(
                "DIFF sec[{}] vert_size: {} -> {}",
                si, os.vert_size, rs.vert_size
            );
        }
        if os.max_bones != rs.max_bones {
            eprintln!(
                "DIFF sec[{}] maxBones: {} -> {}",
                si, os.max_bones, rs.max_bones
            );
        }
        if os.rigid_bone_index != rs.rigid_bone_index {
            eprintln!(
                "DIFF sec[{}] rigidBone: {} -> {}",
                si, os.rigid_bone_index, rs.rigid_bone_index
            );
        }
        if os.global_bones != rs.global_bones {
            eprintln!(
                "DIFF sec[{}] global_bones: {} -> {}",
                si, os.global_bones, rs.global_bones
            );
        }
        if os.rigid_only != rs.rigid_only {
            eprintln!(
                "DIFF sec[{}] rigid_only: {} -> {}",
                si, os.rigid_only, rs.rigid_only
            );
        }
    }

    let min_len = data.len().min(rt_bytes.len());
    let mut diffs = 0usize;
    for i in 0..min_len {
        if data[i] != rt_bytes[i] {
            diffs += 1;
        }
    }
    diffs += data.len().abs_diff(rt_bytes.len());
    eprintln!(
        "\n{} byte diffs (orig={} rt={})",
        diffs,
        data.len(),
        rt_bytes.len()
    );
}

#[test]
#[ignore]
fn diagnose_hw2_global_bones() {
    let path = "/Users/dev/gamedepot/wstore/DUMP/data/archetypes/covenant/banish_cover_small/mesh_cover_small.ugx";
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(_) => {
            eprintln!("File not found — skipping");
            return;
        }
    };
    let original = ugx::Reader::read(&data).unwrap();
    eprintln!("=== ORIGINAL ===");
    for (si, s) in original.sections.iter().enumerate() {
        eprintln!(
            "  sec[{}]: vSz={} glb={} rig={} rigB={} maxB={}",
            si, s.vert_size, s.global_bones, s.rigid_only, s.rigid_bone_index, s.max_bones
        );
    }

    let export_opts = GltfExportOptions {
        embed_buffers: false,
        include_materials: true,
        include_skeleton: true,
    };
    let export = export_to_gltf(&original, &export_opts).unwrap();

    // Check mesh extras
    let root: serde_json::Value = serde_json::from_str(&export.json).unwrap();
    if let Some(meshes) = root.get("meshes").and_then(|m| m.as_array()) {
        for (i, m) in meshes.iter().enumerate() {
            let gb = m.get("extras").and_then(|e| e.get("ugx_global_bones"));
            eprintln!("  glTF mesh[{}] extras.ugx_global_bones = {:?}", i, gb);
        }
    }

    let import_opts = GltfImportOptions {
        version: ugx::UgxVersion::Hw2,
        include_skeleton: true,
        include_materials: true,
    };
    let imported = import_from_gltf(&export.json, export.buffer.as_deref(), &import_opts).unwrap();
    eprintln!("\n=== IMPORTED ===");
    for (si, s) in imported.sections.iter().enumerate() {
        eprintln!(
            "  sec[{}]: vSz={} glb={} rig={} rigB={} maxB={}",
            si, s.vert_size, s.global_bones, s.rigid_only, s.rigid_bone_index, s.max_bones
        );
    }

    let rt_bytes = ugx::Writer::write(&imported, ugx::UgxVersion::Hw2).unwrap();
    let re_read = ugx::Reader::read(&rt_bytes).unwrap();
    eprintln!("\n=== RE-READ ===");
    for (si, s) in re_read.sections.iter().enumerate() {
        eprintln!(
            "  sec[{}]: vSz={} glb={} rig={} rigB={} maxB={}",
            si, s.vert_size, s.global_bones, s.rigid_only, s.rigid_bone_index, s.max_bones
        );
    }

    // Check vertex weights
    for si in 0..original.sections.len().min(re_read.sections.len()) {
        let ov = original.unpack_section_vertices(si).unwrap();
        let rv = re_read.unpack_section_vertices(si).unwrap();
        eprintln!(
            "\n  sec[{}] vert[0] orig weights={:?} indices={:?}",
            si, ov[0].bone_weights, ov[0].bone_indices
        );
        eprintln!(
            "  sec[{}] vert[0] re_read weights={:?} indices={:?}",
            si, rv[0].bone_weights, rv[0].bone_indices
        );
    }
}

#[test]
#[ignore]
fn survey_max_instances() {
    load_dotenv();
    use std::collections::BTreeMap;

    let mut counts: BTreeMap<i16, (usize, Vec<String>)> = BTreeMap::new();
    let mut add = |val: i16, example: String| {
        let entry = counts.entry(val).or_insert((0, Vec::new()));
        entry.0 += 1;
        if entry.1.len() < 3 {
            entry.1.push(example);
        }
    };

    // HW1
    if let Some(hw1_dir) = load_game_dir("HW1_GAME_DIR") {
        let era_paths = find_files_flat(&hw1_dir, "era");
        for era_path in &era_paths {
            let mut archive = match open_era(era_path) {
                Ok(a) => a,
                Err(_) => continue,
            };
            let ugx_entries = find_entries_in_era(&archive, ".ugx");
            for (idx, filename) in &ugx_entries {
                let Ok(data) = archive.read_entry(*idx) else {
                    continue;
                };
                let Ok(geom) = ugx::UgxGeom::from_bytes(&data) else {
                    continue;
                };
                add(geom.max_instances, format!("HW1:{}", filename));
            }
        }
    }

    // HW2
    if let Some(hw2_dir) = load_game_dir("HW2_GAME_DIR") {
        let ugx_files = find_files_by_ext(&hw2_dir, "ugx");
        for path in &ugx_files {
            let Ok(data) = std::fs::read(path) else {
                continue;
            };
            let Ok(geom) = ugx::UgxGeom::from_bytes(&data) else {
                continue;
            };
            let fname = path.file_name().unwrap().to_string_lossy().to_string();
            add(geom.max_instances, format!("HW2:{}", fname));
        }
    }

    for (val, (count, examples)) in &counts {
        eprintln!(
            "max_instances={}: {} files  (e.g. {})",
            val,
            count,
            examples.join(", ")
        );
    }
}
