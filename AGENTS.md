# Agent rules

These rules apply to all agents that change this repository. Obey all rules in all changes.

## Language: ASD-STE100 style

Write all new and changed text in a style like ASD-STE100 Simplified Technical English. This
includes doc comments, code comments, Markdown files and commit messages. Follow the writing
rules below. The STE dictionary does not apply: use any clear English word.

- Keep sentences short: about 20 words for an instruction, about 25 words for a description.
- Write one instruction in each sentence.
- Use the imperative form for instructions.
- Use the active voice.
- Prefer simple tenses.
- Give each word one meaning, and use the same term for the same item each time.
- Use the names from the code for code items (for example `Node`, `CompressionMethod`).
- Use articles ("the", "a") where they make the text clearer.
- Avoid long noun clusters. Break up clusters of more than three words.
- Write short paragraphs with one topic each.
- Use a vertical list for a sequence of steps or for many items.
- Put a warning or a caution before the instruction it applies to.

Write a commit message as `area: summary`, with a lowercase summary. Use the crate name as the
area (for example `ugx`, `era`, `xmb-cli`).

## Lints: Clippy pedantic

The workspace denies all warnings, all Clippy `pedantic` lints, `missing_docs` and
`unsafe_code`. `Cargo.toml` sets this in `[workspace.lints]`.

- Keep `[lints] workspace = true` in the `Cargo.toml` file of each crate.
- Do not lower a lint level in a crate or in the code.
- Do not use `#[allow(...)]`. The workspace denies `clippy::allow_attributes`.
- Use `#[expect(lint, reason = "...")]` only if no change to the code can remove the lint.
- Do not add an item to `clippy.toml` to hide a lint.
- Do not add `unsafe` code.
- Use `zerocopy` to read and write plain data as bytes. Do not use `bytemuck`.
- Make sure that this command completes without warnings:

```sh
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

## File size: 1000 lines

- Keep each source file at a maximum of 1000 lines. This includes integration tests.
- If a change makes a file longer than 1000 lines, split the file into submodules.
- If you change a file that is already longer than 1000 lines, do not make it longer. Split it
  as part of the change.
- Put each submodule in a new file below the directory of its parent module.
- Split by topic. Do not split at a random line.

Clippy also limits each function to 200 lines (`clippy.toml`).

## Layout: Cargo package layout

Follow the Cargo package layout:
<https://doc.rust-lang.org/cargo/guide/project-layout.html>.

- Put the library root in `src/lib.rs`.
- Put the default executable in `src/main.rs`.
- Put other executables in `src/bin/`.
- Put integration tests in `tests/`.
- Put examples in `examples/`.
- Put benchmarks in `benches/`.
- For a test, example, benchmark or executable with many files, make a directory with a `main.rs`
  file and the modules.
- Use snake_case names for modules.
- Put a module in `foo.rs` and its child modules in `foo/`. Do not use `mod.rs` files. The
  workspace denies `clippy::mod_module_files`.
- Put each crate in `crates/<crate-name>/`. Give the command-line tool of a format the name
  `<format>-cli`.
- Define each dependency one time in `[workspace.dependencies]` in the root `Cargo.toml`. In a
  crate, use `<name>.workspace = true` and add only the features that the crate needs.

## Layers

The crates of this repository are in four layers:

1. Base crates: `bdt`, `ecf`, `fxb`, `pkg`, `ufx` and `xml`.
2. Format crates: `ddx`, `era`, `lgt`, `uax`, `ugx`, `xmb`, `xtd` and `xtt`.
3. Adapter crates: `bdt-serde`, `xmb-serde` and `ugx-gltf`.
4. Tool crates: the `*-cli` crates and `test-utils`.

- Make a crate depend only on crates in the same layer or in lower layers.
- A dev-dependency on `test-utils` is permitted in all layers.
- Do not make a crate depend on `ensemble-assets` or `openensemble`. These repositories use this
  repository.

## Format rules

- Keep `no_std` support in each crate that has `#![no_std]`. Use `nostdio` for I/O in these
  crates. Put code that needs `std` behind a `std` feature.
- Do not panic on malformed input. Return an error that tells which value is incorrect.
- Use checked integer conversions at each binary-format boundary.
- Use checked arithmetic for offsets, lengths and ranges that come from a file.
- Write an `# Errors` section for each public function that returns a `Result`.

## Tests

- Add a regression test for each defect. Make sure that the test fails before the fix.
- Make tests deterministic. Compare floating-point values with a tolerance or by bit pattern.
- If a test needs game files, mark it with `#[ignore = "requires ..."]`. Read the game
  directory with `test-utils`. `.env.example` shows the variables (`HW1_GAME_DIR`,
  `HW2_GAME_DIR`).
- Run the ignored tests with `cargo test --workspace --all-features -- --ignored`.

## Working copies

- Use one git worktree for each branch when agents work in parallel.
- Use absolute paths for all edits in a worktree. Do not edit the primary checkout from a
  worktree task.

## Checks

Make sure that all these checks pass before you report a change as complete:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo doc --workspace --all-features --no-deps
cargo test --workspace --all-features
```

For each `no_std` crate that you change, also run
`cargo check -p <crate> --no-default-features`.

`prek run --all-files` runs the format and Clippy hooks.
