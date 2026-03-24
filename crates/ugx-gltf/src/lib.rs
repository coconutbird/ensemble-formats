//! glTF import/export for UGX (Unit Graphics) models.
//!
//! This crate provides conversion between UGX geometry and glTF 2.0 format.
//!
//! ## Example
//!
//! ```ignore
//! use ugx::Reader;
//! use ugx_gltf::{GltfExportOptions, export_to_gltf};
//!
//! let data = std::fs::read("model.ugx")?;
//! let geom = Reader::read(&data)?;
//! let export = export_to_gltf(&geom, &GltfExportOptions::default())?;
//! std::fs::write("model.gltf", &export.json)?;
//! ```

mod export;
pub use export::{GltfExport, GltfExportOptions, export_to_gltf, export_to_gltf_with_buffer_name};

mod import;
pub use import::{GltfImportOptions, import_from_gltf};
