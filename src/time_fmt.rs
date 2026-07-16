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
