# Repository standards

These rules apply to the entire workspace.

## Cargo project layout

- Follow the Cargo Book's package layout: <https://doc.rust-lang.org/cargo/guide/project-layout.html>.
- Keep each package manifest, and its lockfile when the package owns one, at the package root.
- Put library and default binary entry points at `src/lib.rs` and `src/main.rs`.
- Put additional binaries in `src/bin/`, examples in `examples/`, benchmarks in `benches/`, and integration tests in `tests/`.
- Give a multi-file binary, example, benchmark, or integration test its own directory with a `main.rs` entry point and sibling modules.
- Use kebab-case for target names and snake_case for Rust module files, unless compatibility requires an existing target name.
- Prefer Cargo's target auto-discovery. Declare a target explicitly only when its public name or configuration differs from the conventional path.

## Workspace manifests

- Every workspace package must use `[lints] workspace = true`.
- Define every direct third-party and intra-workspace dependency once in the root `[workspace.dependencies]` table. Package manifests must opt in with `.workspace = true` and may add only package-specific features or optionality.
- Inherit shared package metadata such as edition, license, repository, authors, and version from `[workspace.package]` when applicable.

## Rust source standards

- Rust source files must remain under 1,000 physical lines. Split a file into cohesive snake_case modules before it reaches 1,000 lines; do not evade the limit by compressing formatting.
- Fix compiler, Clippy, and rustdoc warnings at their source. Do not add `allow`, `expect`, command-line lint caps, or weaker lint levels to silence diagnostics.
- Preserve `no_std` support where a crate advertises it. Use `nostdio` 0.2 for the workspace's allocation-backed I/O abstractions.
- Use checked integer conversions and checked offset/range arithmetic at binary-format boundaries. Return a descriptive error for malformed or unrepresentable data.
- Keep public APIs documented, including failure conditions for fallible operations.
- Keep tests deterministic and express floating-point intent explicitly with tolerances or bit-pattern comparisons.

## Required validation

Run these checks after repository-wide changes:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features
cargo test --workspace --all-targets --all-features
cargo doc --workspace --all-features --no-deps
```

Also test relevant `no_std` crates without default features and confirm that every Rust source file is below the 1,000-line limit.
