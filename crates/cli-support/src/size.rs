//! Human-readable byte sizes.

const KB: u64 = 1024;
const MB: u64 = 1024 * KB;
const GB: u64 = 1024 * MB;

/// Formats a byte count as a human-readable size.
///
/// The function uses binary units (`KB`, `MB`, `GB`) and integer arithmetic.
/// It truncates the value to one decimal place. A value below 1024 has the
/// suffix `B` and no decimal place.
#[must_use]
pub fn format_size(bytes: u64) -> String {
    if bytes >= GB {
        format_scaled_size(bytes, GB, "GB")
    } else if bytes >= MB {
        format_scaled_size(bytes, MB, "MB")
    } else if bytes >= KB {
        format_scaled_size(bytes, KB, "KB")
    } else {
        format!("{bytes} B")
    }
}

fn format_scaled_size(bytes: u64, unit: u64, suffix: &str) -> String {
    let whole = bytes / unit;
    let tenths = (bytes % unit) * 10 / unit;
    format!("{whole}.{tenths} {suffix}")
}

#[cfg(test)]
mod tests {
    use super::{GB, KB, MB, format_size};

    #[test]
    fn formats_unit_boundaries() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(KB - 1), "1023 B");
        assert_eq!(format_size(KB), "1.0 KB");
        assert_eq!(format_size(MB - 1), "1023.9 KB");
        assert_eq!(format_size(MB), "1.0 MB");
        assert_eq!(format_size(GB - 1), "1023.9 MB");
        assert_eq!(format_size(GB), "1.0 GB");
    }

    #[test]
    fn truncates_fractions() {
        assert_eq!(format_size(1535), "1.4 KB");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(2 * MB + MB / 2), "2.5 MB");
        assert_eq!(format_size(u64::MAX), "17179869183.9 GB");
    }
}
