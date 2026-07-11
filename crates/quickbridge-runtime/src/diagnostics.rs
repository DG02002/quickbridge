use std::ffi::{OsStr, OsString};

pub fn render_command(program: &OsStr, args: &[OsString]) -> String {
    std::iter::once(program.to_string_lossy().into_owned())
        .chain(args.iter().map(|arg| shell_escape(&arg.to_string_lossy())))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn render_request(method: &str, url: &str, extra: Option<&str>) -> String {
    match extra {
        Some(extra) => format!("Request: {method} {url} ({extra})"),
        None => format!("Request: {method} {url}"),
    }
}

fn shell_escape(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | ':' | '.' | '_' | '-'))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}
