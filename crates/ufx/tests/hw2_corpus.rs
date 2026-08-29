//! Optional retail-file coverage supplied through `HW2_GAME_DIR`.

use std::collections::BTreeMap;
use std::path::Path;

use test_utils::prelude::*;
use ufx::ShaderStage;

#[test]
fn retail_hw2_stage_tables_match_all_dxbc_programs() {
    let Some(game_dir) = load_game_dir("HW2_GAME_DIR") else {
        return;
    };
    let files = find_files_by_ext(&game_dir, "ufx");
    assert!(
        !files.is_empty(),
        "No UFX files found under {}",
        game_dir.display()
    );

    let mut versions = BTreeMap::<u32, usize>::new();
    let mut stage_counts = BTreeMap::<String, usize>::new();
    let mut input_counts = BTreeMap::<usize, usize>::new();
    let mut failures = Vec::new();

    for path in &files {
        let data = match std::fs::read(path) {
            Ok(data) => data,
            Err(error) => {
                failures.push(format!("{}: read: {error}", path.display()));
                continue;
            }
        };
        let file = match ufx::parse(&data) {
            Ok(file) => file,
            Err(error) => {
                failures.push(format!("{}: parse: {error}", path.display()));
                continue;
            }
        };

        *versions.entry(file.version).or_default() += 1;
        *input_counts.entry(file.vertex_inputs.len()).or_default() += 1;
        validate_inputs(path, &file, &mut failures);
        validate_stages(path, &file, &mut stage_counts, &mut failures);
    }

    eprintln!(
        "HW2 UFX corpus: {} files, versions {versions:?}, inputs {input_counts:?}, stages {stage_counts:?}",
        files.len()
    );
    assert!(
        failures.is_empty(),
        "HW2 UFX failures ({}):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

fn validate_inputs(path: &Path, file: &ufx::UfxFile<'_>, failures: &mut Vec<String>) {
    for input in &file.vertex_inputs {
        let Some(stride) = file.vertex_stride(input.input_slot) else {
            failures.push(format!(
                "{}: input slot {} has no computed stride",
                path.display(),
                input.input_slot,
            ));
            continue;
        };
        let Some(end_offset) = input.end_offset() else {
            failures.push(format!(
                "{}: input {input:?} has an overflowing end offset",
                path.display(),
            ));
            continue;
        };
        if end_offset > stride {
            failures.push(format!(
                "{}: input {input:?} ends after slot stride {stride}",
                path.display(),
            ));
        }
    }
}

fn validate_stages(
    path: &Path,
    file: &ufx::UfxFile<'_>,
    stage_counts: &mut BTreeMap<String, usize>,
    failures: &mut Vec<String>,
) {
    for stage in ShaderStage::ALL {
        let range = file.stage_ranges.get(stage);
        let shader = file.shader(stage);
        if !range.is_present() {
            if shader.is_some() {
                failures.push(format!(
                    "{}: absent {stage} unexpectedly parsed",
                    path.display()
                ));
            }
            continue;
        }
        let Some(shader) = shader else {
            failures.push(format!(
                "{}: present {stage} range was not parsed",
                path.display()
            ));
            continue;
        };
        let Ok(expected_offset) = usize::try_from(range.offset) else {
            failures.push(format!(
                "{}: {stage} offset {:#X} is not representable on this platform",
                path.display(),
                range.offset
            ));
            continue;
        };
        if shader.offset() != expected_offset || shader.size() != range.size {
            failures.push(format!(
                "{}: {stage} range {:?} != parsed offset {:#X}, size {:#X}",
                path.display(),
                range,
                shader.offset(),
                shader.size()
            ));
        }
        *stage_counts.entry(stage.to_string()).or_default() += 1;
    }
}
