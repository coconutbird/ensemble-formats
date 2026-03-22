//! XML entity escaping and unescaping.

use alloc::string::String;

/// Append `s` to `buf`, replacing `& < > " '` with XML entities.
pub fn escape_into(buf: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '&' => buf.push_str("&amp;"),
            '<' => buf.push_str("&lt;"),
            '>' => buf.push_str("&gt;"),
            '"' => buf.push_str("&quot;"),
            '\'' => buf.push_str("&apos;"),
            _ => buf.push(c),
        }
    }
}

/// Return a new string with XML entities replaced by their characters.
pub fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return String::from(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find('&') {
        out.push_str(&rest[..pos]);
        rest = &rest[pos..];
        if let Some(end) = rest.find(';') {
            let entity = &rest[..end + 1];
            match entity {
                "&amp;" => out.push('&'),
                "&lt;" => out.push('<'),
                "&gt;" => out.push('>'),
                "&quot;" => out.push('"'),
                "&apos;" => out.push('\''),
                _ => out.push_str(entity),
            }
            rest = &rest[end + 1..];
        } else {
            out.push_str(rest);
            rest = "";
        }
    }
    out.push_str(rest);
    out
}
