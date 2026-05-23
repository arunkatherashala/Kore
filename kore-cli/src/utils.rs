use std::path::Path;

/// Format bytes to human-readable format
pub fn format_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_idx = 0;

    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }

    format!("{:.2} {}", size, UNITS[unit_idx])
}

/// Format duration to human-readable format
pub fn format_duration(secs: u64) -> String {
    match secs {
        0..=59 => format!("{}s", secs),
        60..=3599 => format!("{}m {}s", secs / 60, secs % 60),
        _ => format!("{}h {}m", secs / 3600, (secs % 3600) / 60),
    }
}

/// Check if file has Kore format
pub fn is_kore_file(path: &Path) -> bool {
    matches!(path.extension().and_then(|s| s.to_str()), Some("kore"))
}

/// Format percentage
pub fn format_percent(value: f64) -> String {
    format!("{:.1}%", value)
}

/// Calculate throughput (MB/s)
pub fn calculate_throughput(bytes: u64, secs: f64) -> f64 {
    (bytes as f64 / (1024.0 * 1024.0)) / secs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(512), "512.00 B");
        assert_eq!(format_size(1024), "1.00 KB");
        assert_eq!(format_size(1024 * 1024), "1.00 MB");
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(30), "30s");
        assert_eq!(format_duration(90), "1m 30s");
        assert_eq!(format_duration(3661), "1h 1m");
    }

    #[test]
    fn test_format_percent() {
        assert_eq!(format_percent(50.5), "50.5%");
    }
}
