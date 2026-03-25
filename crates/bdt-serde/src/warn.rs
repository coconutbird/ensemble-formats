//! Diagnostics for tracking issues during deserialization.
//!
//! When using [`crate::from_node_warned`] (or the xmb-serde equivalents),
//! the deserializer collects [`Warning`]s instead of silently ignoring
//! problems. Three categories are tracked:
//!
//! - **Extra fields** — the XML/BDT data contains attributes or child
//!   elements that the target Rust struct does not declare.
//! - **Missing fields** — the struct expected a field that was absent in
//!   the data (only recorded when serde falls back to a default).
//! - **Type mismatches** — a value existed but could not be converted to
//!   the type the struct field requires (e.g. a string where an `i32` was
//!   expected).

use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::fmt;

/// A diagnostic warning produced during deserialization.
///
/// Warnings do **not** stop the parse — the deserializer continues with
/// defaults or skipped values. Inspect the returned `Vec<Warning>` after
/// deserialization to discover schema mismatches in game data files.
#[derive(Debug, Clone)]
pub enum Warning {
    /// The data contained a field that the target struct does not declare.
    ExtraField {
        /// The attribute (`@name`) or child element name that was not consumed.
        field: String,
        /// The parent element where the extra field was found.
        element: String,
    },

    /// A value was present but its type did not match what the struct field
    /// expected (e.g. a string where an `i32` was required).
    TypeMismatch {
        /// The field name (`@attr`, `$text`, or child element name).
        field: String,
        /// The parent element where the mismatch occurred.
        element: String,
        /// What the struct field expected (e.g. `"u32"`, `"bool"`).
        expected: String,
        /// A short description of the actual value (e.g. `"String(\"abc\")"`)
        actual: String,
    },
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Warning::ExtraField { field, element } => {
                write!(f, "extra field `{field}` in <{element}>")
            }
            Warning::TypeMismatch {
                field,
                element,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "type mismatch for `{field}` in <{element}>: expected {expected}, got {actual}"
                )
            }
        }
    }
}

/// Collects warnings during deserialization without stopping the parse.
///
/// Passed through the deserializer via shared reference so that nested
/// `NodeMapAccess` instances can all record to the same list.
pub struct Diagnostics {
    warnings: RefCell<Vec<Warning>>,
}

impl Diagnostics {
    pub fn new() -> Self {
        Self {
            warnings: RefCell::new(Vec::new()),
        }
    }

    /// Record an extra (unmapped) field.
    pub fn record_extra(&self, field: String, element: String) {
        self.warnings
            .borrow_mut()
            .push(Warning::ExtraField { field, element });
    }

    /// Record a type mismatch.
    pub fn record_type_mismatch(
        &self,
        field: String,
        element: String,
        expected: String,
        actual: String,
    ) {
        self.warnings.borrow_mut().push(Warning::TypeMismatch {
            field,
            element,
            expected,
            actual,
        });
    }

    /// Consume and return all collected warnings.
    pub fn into_warnings(self) -> Vec<Warning> {
        self.warnings.into_inner()
    }
}
