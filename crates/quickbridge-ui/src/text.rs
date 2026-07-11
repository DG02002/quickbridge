use quickbridge_core::Timecode;

pub fn format_playback_time(
    estimated_position: Timecode,
    total_runtime: Option<Timecode>,
) -> String {
    match total_runtime {
        Some(total_runtime) => format!("{estimated_position} / {total_runtime}"),
        None => estimated_position.to_string(),
    }
}

pub fn format_warning(text: &str) -> String {
    format!("[WARN] {text}")
}

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];

    if bytes < 1024 {
        return format!("{bytes} B");
    }

    let mut value = bytes as f64;
    let mut unit_index = 0;
    while value >= 1024.0 && unit_index < UNITS.len() - 1 {
        value /= 1024.0;
        unit_index += 1;
    }

    format!("{value:.1} {}", UNITS[unit_index])
}

pub fn format_bytes_per_second(bytes_per_second: u64) -> String {
    format!("{}/s", format_bytes(bytes_per_second))
}
