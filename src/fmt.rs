use std::time::SystemTime;

/// Hand-rolled "time ago" formatting so we don't need a chrono dependency.
pub fn relative(t: SystemTime) -> String {
    let secs = SystemTime::now()
        .duration_since(t)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    if secs < 60 {
        "just now".to_string()
    } else if secs < 60 * 60 {
        format!("{}m ago", secs / 60)
    } else if secs < 60 * 60 * 24 {
        format!("{}h ago", secs / (60 * 60))
    } else if secs < 60 * 60 * 24 * 7 {
        format!("{}d ago", secs / (60 * 60 * 24))
    } else {
        format!("{}w ago", secs / (60 * 60 * 24 * 7))
    }
}

/// Hand-rolled byte-size formatting so we don't need an extra crate.
pub fn human_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    let b = bytes as f64;
    if b >= GB {
        format!("{:.1}GB", b / GB)
    } else if b >= MB {
        format!("{:.1}MB", b / MB)
    } else if b >= KB {
        format!("{:.1}KB", b / KB)
    } else {
        format!("{bytes}B")
    }
}
