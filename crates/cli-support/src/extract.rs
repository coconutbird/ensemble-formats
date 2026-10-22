//! Shared workflow to extract archive entries to the file system.
//!
//! An archive tool uses this module as follows:
//!
//! 1. Open the archive and report errors in the format of the tool.
//! 2. Make an [`EntrySelector`] from the filter arguments.
//! 3. Start an [`Extraction`]. This makes the output directory.
//! 4. For each selected entry, read the data and give it to
//!    [`Extraction::write_entry`].
//! 5. Call [`Extraction::finish`] to print the report and get the exit code.

use std::fmt::{self, Display};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::exit_code;

/// Replaces each backslash in an archive path with a forward slash.
#[must_use]
pub fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
}

/// Error for a filter argument that is not a valid glob pattern.
#[derive(Debug)]
pub struct InvalidPattern {
    pattern: String,
    error: glob::PatternError,
}

impl Display for InvalidPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Invalid glob pattern '{}': {}", self.pattern, self.error)
    }
}

impl std::error::Error for InvalidPattern {}

/// A path that the user names with a file argument.
struct FileSelector {
    /// The normalized path.
    path: String,
    /// The normalized path with a `/` prefix, to match a path suffix.
    suffix: String,
}

/// Selects archive entries by a glob filter or by a list of file paths.
///
/// If the list of file paths is not empty, the selector ignores the glob
/// filter. An entry matches a file path if the paths are equal, or if the
/// entry path ends with `/` and the file path.
pub struct EntrySelector {
    pattern: Option<glob::Pattern>,
    files: Vec<FileSelector>,
}

impl EntrySelector {
    /// Makes a selector from the filter and file arguments of a tool.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidPattern`] if `filter` is not a valid glob pattern.
    pub fn new(filter: Option<&str>, specific_files: &[String]) -> Result<Self, InvalidPattern> {
        let pattern = filter
            .map(|filter| {
                glob::Pattern::new(filter).map_err(|error| InvalidPattern {
                    pattern: filter.to_string(),
                    error,
                })
            })
            .transpose()?;
        let files = specific_files
            .iter()
            .map(|file| {
                let path = normalize_path(file);
                let suffix = format!("/{path}");
                FileSelector { path, suffix }
            })
            .collect();
        Ok(Self { pattern, files })
    }

    /// Returns `true` if the selector selects the entry at `normalized`.
    ///
    /// `normalized` is the entry path after [`normalize_path`].
    #[must_use]
    pub fn matches(&self, normalized: &str) -> bool {
        if self.files.is_empty() {
            self.pattern
                .as_ref()
                .is_none_or(|pattern| pattern.matches(normalized))
        } else {
            self.files
                .iter()
                .any(|file| normalized == file.path || normalized.ends_with(&file.suffix))
        }
    }
}

/// Output options of an extraction.
#[derive(Debug, Clone, Copy)]
pub struct ExtractionOptions {
    /// Print one JSON report at the end, and no text lines.
    pub json: bool,
    /// Print no text lines. Errors also stay silent.
    pub quiet: bool,
}

/// The entry name that text lines show.
#[derive(Debug, Clone, Copy)]
pub enum NameDisplay {
    /// Show the entry name as the archive stores it.
    Original,
    /// Show the entry name after [`normalize_path`].
    Normalized,
}

/// The format-specific text of an extraction.
#[derive(Debug, Clone, Copy)]
pub struct ExtractionStyle {
    /// The entry name that text lines show.
    pub display: NameDisplay,
    /// Prefix of the JSON error text when the archive read fails.
    pub read_error_prefix: &'static str,
    /// Prefix of the JSON error text when the directory creation fails.
    pub create_dir_error_prefix: &'static str,
    /// Prefix of the JSON error text when the file write fails.
    pub write_error_prefix: &'static str,
}

/// The names of one archive entry.
#[derive(Debug, Clone, Copy)]
pub struct EntryName<'a> {
    /// The entry name as the archive stores it.
    pub original: &'a str,
    /// The entry name after [`normalize_path`]. The output file uses this path.
    pub normalized: &'a str,
}

/// The JSON result of one entry.
#[derive(Debug, Serialize)]
pub struct ExtractedFile {
    /// The entry name as the archive stores it.
    pub filename: String,
    /// `true` if the tool wrote the file.
    pub success: bool,
    /// The error text if the extraction of the entry fails.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The JSON report of an extraction.
#[derive(Debug, Serialize)]
pub struct ExtractionReport {
    /// The archive path argument.
    pub archive: String,
    /// The output directory.
    pub output_dir: String,
    /// The number of entries that the tool wrote.
    pub extracted: usize,
    /// The number of entries that failed.
    pub errors: usize,
    /// The result of each entry. This list is empty if JSON output is off.
    pub files: Vec<ExtractedFile>,
}

impl ExtractionReport {
    /// Returns the exit code for the report.
    ///
    /// The code is [`exit_code::PARTIAL_FAILURE`] if an entry failed, and
    /// [`exit_code::SUCCESS`] if not.
    #[must_use]
    pub fn exit_code(&self) -> i32 {
        if self.errors > 0 {
            exit_code::PARTIAL_FAILURE
        } else {
            exit_code::SUCCESS
        }
    }
}

/// An extraction in progress.
pub struct Extraction {
    output_dir: PathBuf,
    options: ExtractionOptions,
    style: ExtractionStyle,
    report: ExtractionReport,
}

impl Extraction {
    /// Makes the output directory and prints the start line.
    ///
    /// If `output_dir` is `None`, the output directory is the file stem of
    /// `archive_path`, or `output` if the path has no file stem.
    ///
    /// # Errors
    ///
    /// Returns the I/O error if the output directory cannot be made.
    pub fn start(
        archive_path: &str,
        output_dir: Option<&str>,
        options: ExtractionOptions,
        style: ExtractionStyle,
    ) -> io::Result<Self> {
        let output_dir = output_dir.map_or_else(
            || {
                Path::new(archive_path)
                    .file_stem()
                    .map_or_else(|| "output".to_string(), |s| s.to_string_lossy().to_string())
            },
            str::to_string,
        );
        let path = PathBuf::from(&output_dir);
        fs::create_dir_all(&path)?;

        if !options.quiet && !options.json {
            println!("Extracting {} to {}...", archive_path, path.display());
        }

        Ok(Self {
            output_dir: path,
            options,
            style,
            report: ExtractionReport {
                archive: archive_path.to_string(),
                output_dir,
                extracted: 0,
                errors: 0,
                files: Vec::new(),
            },
        })
    }

    /// Writes the data of one entry, or records the read error.
    ///
    /// The function writes the file to the normalized entry path below the
    /// output directory. It makes the parent directories first.
    pub fn write_entry<E: Display>(&mut self, name: EntryName<'_>, data: Result<Vec<u8>, E>) {
        match self.try_write_entry(name, data) {
            Ok(()) => {
                self.report.extracted += 1;
                if self.options.json {
                    self.push_file(name, None);
                } else if !self.options.quiet {
                    println!("  {}", self.display_name(name));
                }
            }
            Err((prefix, action, error)) => {
                self.report.errors += 1;
                if self.options.json {
                    self.push_file(name, Some(format!("{prefix}{error}")));
                } else if !self.options.quiet {
                    eprintln!("  Error {action} {}: {error}", self.display_name(name));
                }
            }
        }
    }

    /// Runs the steps of one entry and returns the failed step on error.
    ///
    /// The error is the JSON prefix, the text-line action and the error text.
    fn try_write_entry<E: Display>(
        &self,
        name: EntryName<'_>,
        data: Result<Vec<u8>, E>,
    ) -> Result<(), (&'static str, &'static str, String)> {
        let data =
            data.map_err(|error| (self.style.read_error_prefix, "reading", error.to_string()))?;
        let file_path = self.output_dir.join(name.normalized);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                (
                    self.style.create_dir_error_prefix,
                    "creating directory for",
                    error.to_string(),
                )
            })?;
        }
        fs::write(&file_path, data)
            .map_err(|error| (self.style.write_error_prefix, "writing", error.to_string()))
    }

    fn display_name<'a>(&self, name: EntryName<'a>) -> &'a str {
        match self.style.display {
            NameDisplay::Original => name.original,
            NameDisplay::Normalized => name.normalized,
        }
    }

    fn push_file(&mut self, name: EntryName<'_>, error: Option<String>) {
        self.report.files.push(ExtractedFile {
            filename: name.original.to_string(),
            success: error.is_none(),
            error,
        });
    }

    /// Prints the report and returns the exit code.
    ///
    /// With JSON output, the function prints the JSON report. Otherwise, it
    /// prints a summary line if quiet output is off.
    ///
    /// # Panics
    ///
    /// Panics if the report cannot be serialized as JSON. This does not occur
    /// for the field types of the report.
    #[must_use]
    pub fn finish(self) -> i32 {
        let report = self.report;
        if self.options.json {
            println!(
                "{}",
                serde_json::to_string(&report).expect("extraction report serializes")
            );
        } else if !self.options.quiet {
            println!();
            println!(
                "Extracted {} files ({} errors)",
                report.extracted, report.errors
            );
        }
        report.exit_code()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{
        EntryName, EntrySelector, Extraction, ExtractionOptions, ExtractionStyle, NameDisplay,
    };
    use crate::exit_code;

    const STYLE: ExtractionStyle = ExtractionStyle {
        display: NameDisplay::Original,
        read_error_prefix: "Read error: ",
        create_dir_error_prefix: "Directory creation error: ",
        write_error_prefix: "Write error: ",
    };

    #[test]
    fn selector_matches_exact_paths_and_path_suffixes() {
        let files = vec![String::from("dir\\b.txt")];
        let selector = EntrySelector::new(Some("*.xml"), &files).unwrap();
        assert!(selector.matches("dir/b.txt"));
        assert!(selector.matches("root/dir/b.txt"));
        assert!(!selector.matches("rootdir/b.txt"));
        assert!(!selector.matches("a.xml"));
    }

    #[test]
    fn selector_uses_pattern_without_files() {
        let selector = EntrySelector::new(Some("art/*.ddx"), &[]).unwrap();
        assert!(selector.matches("art/a.ddx"));
        assert!(!selector.matches("data/a.ddx"));
        assert!(EntrySelector::new(None, &[]).unwrap().matches("x"));
    }

    #[test]
    fn selector_reports_invalid_pattern() {
        let Err(error) = EntrySelector::new(Some("[a"), &[]) else {
            panic!("pattern must be invalid");
        };
        assert!(error.to_string().starts_with("Invalid glob pattern '[a': "));
    }

    #[test]
    fn extraction_writes_files_and_reports_json() {
        let root = std::env::temp_dir().join(format!("cli-support-extract-{}", std::process::id()));
        let output_dir = root.join("out");
        let output_text = output_dir.to_str().unwrap().to_string();
        let options = ExtractionOptions {
            json: true,
            quiet: false,
        };
        let mut extraction =
            Extraction::start("archive.era", Some(&output_text), options, STYLE).unwrap();
        extraction.write_entry(
            EntryName {
                original: "dir\\a.bin",
                normalized: "dir/a.bin",
            },
            Ok::<_, String>(vec![1, 2, 3]),
        );
        extraction.write_entry(
            EntryName {
                original: "b.bin",
                normalized: "b.bin",
            },
            Err::<Vec<u8>, _>("bad entry"),
        );

        assert_eq!(fs::read(output_dir.join("dir/a.bin")).unwrap(), [1, 2, 3]);
        let report = &extraction.report;
        assert_eq!(report.exit_code(), exit_code::PARTIAL_FAILURE);
        let expected = format!(
            concat!(
                r#"{{"archive":"archive.era","output_dir":{},"extracted":1,"errors":1,"#,
                r#""files":[{{"filename":"dir\\a.bin","success":true}},"#,
                r#"{{"filename":"b.bin","success":false,"error":"Read error: bad entry"}}]}}"#
            ),
            serde_json::to_string(&output_text).unwrap()
        );
        assert_eq!(serde_json::to_string(report).unwrap(), expected);
        fs::remove_dir_all(root).unwrap();
    }
}
