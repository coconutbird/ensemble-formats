//! XML pull-reader — wraps [`xmlparser::Tokenizer`] with auto-unescaping.
//!
//! # Example
//!
//! ```
//! use xml::reader::{Reader, Event};
//!
//! let reader = Reader::new("<root attr=\"1 &amp; 2\">hello</root>");
//! let events: Vec<_> = reader.map(|e| e.unwrap()).collect();
//! assert!(matches!(&events[0], Event::ElementStart { name } if name == "root"));
//! assert!(matches!(&events[1], Event::Attribute { value, .. } if value == "1 & 2"));
//! ```

use alloc::string::String;
use xmlparser::{ElementEnd, Token, Tokenizer};

use crate::escape;

/// Simplified XML event with unescaped values.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// `<name` — start of an element (attributes follow).
    ElementStart { name: String },
    /// An attribute on the current element. Value is already unescaped.
    Attribute { name: String, value: String },
    /// `>` — the element tag is open (children/text follow).
    ElementOpen,
    /// `/>` — self-closing element.
    ElementEmpty,
    /// `</name>` — closing tag.
    ElementClose { name: String },
    /// Text content (unescaped).
    Text(String),
    /// CDATA content (verbatim).
    Cdata(String),
}

/// Error from the XML reader.
#[derive(Debug, Clone)]
pub struct Error {
    pub message: String,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.message)
    }
}

/// Pull-reader that yields [`Event`]s from an XML string.
pub struct Reader<'a> {
    tokenizer: Tokenizer<'a>,
}

impl<'a> Reader<'a> {
    /// Create a new reader from an XML string.
    #[must_use]
    pub fn new(xml: &'a str) -> Self {
        Self {
            tokenizer: Tokenizer::from(xml),
        }
    }
}

impl Iterator for Reader<'_> {
    type Item = Result<Event, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let token_result = self.tokenizer.next()?;
            let token = match token_result {
                Ok(t) => t,
                Err(e) => {
                    return Some(Err(Error {
                        message: alloc::format!("XML parse error: {e}"),
                    }));
                }
            };

            let event = match token {
                Token::ElementStart { local, .. } => Event::ElementStart {
                    name: String::from(local.as_str()),
                },
                Token::Attribute { local, value, .. } => Event::Attribute {
                    name: String::from(local.as_str()),
                    value: escape::unescape(value.as_str()),
                },
                Token::ElementEnd { end, .. } => match end {
                    ElementEnd::Open => Event::ElementOpen,
                    ElementEnd::Empty => Event::ElementEmpty,
                    ElementEnd::Close(_, local) => Event::ElementClose {
                        name: String::from(local.as_str()),
                    },
                },
                Token::Text { text } => Event::Text(escape::unescape(text.as_str())),
                Token::Cdata { text, .. } => Event::Cdata(String::from(text.as_str())),
                // Skip declarations, comments, processing instructions
                _ => continue,
            };

            return Some(Ok(event));
        }
    }
}
