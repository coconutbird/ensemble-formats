//! XML writer — builds an indented XML document into a [`String`].
//!
//! # Example
//!
//! ```
//! use xml::Writer;
//!
//! let mut w = Writer::new();
//! w.declaration();
//! w.open("root");
//! w.attr("version", "1");
//! w.close();
//! w.open("child");
//! w.close();
//! w.text("hello");
//! w.end("child");
//! w.empty("leaf");
//! w.end("root");
//!
//! let xml = w.finish();
//! assert!(xml.contains("<root version=\"1\">"));
//! assert!(xml.contains("    <child>hello</child>"));
//! assert!(xml.contains("    <leaf/>"));
//! ```

use crate::escape::escape_into;
use alloc::string::String;

/// Indented XML writer that accumulates output in a [`String`].
pub struct Writer {
    buf: String,
    depth: usize,
    indent: &'static str,
    /// `true` while inside an open `<tag` that hasn't been closed with `>` yet.
    in_open_tag: bool,
    /// `true` if the current element has had content written (text/children).
    has_content: bool,
}

impl Writer {
    /// Create a new writer with 4-space indentation.
    pub fn new() -> Self {
        Self {
            buf: String::new(),
            depth: 0,
            indent: "    ",
            in_open_tag: false,
            has_content: false,
        }
    }

    /// Write `<?xml version="1.0" encoding="utf-8"?>` followed by a newline.
    pub fn declaration(&mut self) {
        self.buf
            .push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n");
    }

    /// Begin an element: writes indentation and `<name`.
    ///
    /// Follow with [`attr`](Self::attr) calls, then either:
    /// - [`close`](Self::close) to emit `>` (expects children/text + [`end`](Self::end))
    /// - [`close_empty`](Self::close_empty) to emit `/>` (self-closing, no [`end`](Self::end))
    pub fn open(&mut self, name: &str) {
        self.finish_open_tag_if_needed(true);
        self.write_indent();
        self.buf.push('<');
        self.buf.push_str(name);
        self.in_open_tag = true;
        self.has_content = false;
    }

    /// Add an attribute to the currently open element.
    ///
    /// The value is XML-escaped automatically.
    pub fn attr(&mut self, name: &str, value: &str) {
        debug_assert!(self.in_open_tag, "attr() called outside an open tag");
        self.buf.push(' ');
        self.buf.push_str(name);
        self.buf.push_str("=\"");
        escape_into(&mut self.buf, value);
        self.buf.push('"');
    }

    /// Close the open tag with `>` and increase indentation depth.
    pub fn close(&mut self) {
        debug_assert!(self.in_open_tag, "close() called without a matching open()");
        self.buf.push('>');
        self.in_open_tag = false;
        self.depth += 1;
    }

    /// Self-close the open tag with `/>` followed by a newline.
    pub fn close_empty(&mut self) {
        debug_assert!(
            self.in_open_tag,
            "close_empty() called without a matching open()"
        );
        self.buf.push_str("/>\n");
        self.in_open_tag = false;
    }

    /// Convenience: write a self-closing element with no attributes.
    pub fn empty(&mut self, name: &str) {
        self.open(name);
        self.close_empty();
    }

    /// Write escaped text content inside the current element.
    pub fn text(&mut self, text: &str) {
        self.has_content = true;
        escape_into(&mut self.buf, text);
    }

    /// Write a closing `</name>` tag, decrease depth, and append a newline.
    pub fn end(&mut self, name: &str) {
        self.depth -= 1;
        if !self.has_content {
            // children were written — indent the closing tag
            self.write_indent();
        }
        self.buf.push_str("</");
        self.buf.push_str(name);
        self.buf.push_str(">\n");
        // Reset: the parent now has content (this child).
        self.has_content = false;
    }

    /// Consume the writer and return the accumulated XML string.
    pub fn finish(self) -> String {
        self.buf
    }

    /// Returns a reference to the accumulated output so far.
    pub fn as_str(&self) -> &str {
        &self.buf
    }

    fn write_indent(&mut self) {
        for _ in 0..self.depth {
            self.buf.push_str(self.indent);
        }
    }

    fn finish_open_tag_if_needed(&mut self, _newline: bool) {
        // No-op — open tags are always closed explicitly via close() or close_empty().
    }
}

impl Default for Writer {
    fn default() -> Self {
        Self::new()
    }
}
