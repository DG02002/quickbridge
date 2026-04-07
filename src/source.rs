use reqwest::{
    Client, StatusCode, Url,
    header::{ACCEPT_RANGES, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, RANGE},
};
use tracing::debug;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceMetadata {
    filename: String,
    size_bytes: Option<u64>,
}

impl SourceMetadata {
    pub(crate) fn new(filename: impl Into<String>, size_bytes: Option<u64>) -> Self {
        Self {
            filename: filename.into(),
            size_bytes,
        }
    }

    pub fn filename(&self) -> &str {
        &self.filename
    }

    pub fn display_size(&self) -> String {
        self.size_bytes
            .map(format_bytes)
            .unwrap_or_else(|| String::from("Unknown"))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SeekSupport {
    Enabled,
    Disabled { warning: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceInspection {
    metadata: SourceMetadata,
    seek_support: SeekSupport,
}

impl SourceInspection {
    pub(crate) fn new(metadata: SourceMetadata, seek_support: SeekSupport) -> Self {
        Self {
            metadata,
            seek_support,
        }
    }

    pub fn metadata(&self) -> &SourceMetadata {
        &self.metadata
    }

    pub fn seek_support(&self) -> &SeekSupport {
        &self.seek_support
    }

    pub fn seeking_enabled(&self) -> bool {
        matches!(self.seek_support, SeekSupport::Enabled)
    }

    pub fn seek_warning(&self) -> Option<&str> {
        match &self.seek_support {
            SeekSupport::Enabled => None,
            SeekSupport::Disabled { warning } => Some(warning.as_str()),
        }
    }
}

pub async fn inspect_source(url: &Url) -> SourceInspection {
    let mut metadata = SourceMetadata {
        filename: filename_from_url(url.as_str()),
        size_bytes: None,
    };

    let Ok(client) = Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
    else {
        return finish_inspection(
            metadata,
            SeekSupport::Disabled {
                warning: String::from(
                    "Couldn't confirm whether this source supports jumping to a different time.",
                ),
            },
        );
    };

    let mut support = None;

    debug!(url = %url, "sending HEAD request for source inspection");
    if let Ok(response) = client.head(url.clone()).send().await {
        apply_response_metadata(&mut metadata, &response);
        support = classify_seek_support(&response);
    }

    if support != Some(SeekSupport::Enabled) {
        debug!(url = %url, "sending GET range request for source inspection");
        if let Ok(response) = client
            .get(url.clone())
            .header(RANGE, "bytes=0-0")
            .send()
            .await
        {
            apply_response_metadata(&mut metadata, &response);
            support = classify_seek_support(&response);
        }
    }

    let seek_support = support.unwrap_or_else(|| SeekSupport::Disabled {
        warning: String::from(
            "Couldn't confirm whether this source supports jumping to a different time.",
        ),
    });

    finish_inspection(metadata, seek_support)
}

fn finish_inspection(mut metadata: SourceMetadata, seek_support: SeekSupport) -> SourceInspection {
    if metadata.filename.is_empty() {
        metadata.filename = String::from("Unknown");
    }

    SourceInspection {
        metadata,
        seek_support,
    }
}

fn classify_seek_support(response: &reqwest::Response) -> Option<SeekSupport> {
    classify_seek_support_parts(response.status(), response.headers())
}

fn classify_seek_support_parts(
    status: StatusCode,
    headers: &reqwest::header::HeaderMap,
) -> Option<SeekSupport> {
    if headers
        .get(ACCEPT_RANGES)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("bytes"))
    {
        return Some(SeekSupport::Enabled);
    }

    if status == StatusCode::PARTIAL_CONTENT || headers.contains_key(CONTENT_RANGE) {
        return Some(SeekSupport::Enabled);
    }

    if status.is_success() {
        return Some(SeekSupport::Disabled {
            warning: String::from(
                "This source doesn't appear to support jumping to a different time.",
            ),
        });
    }

    None
}

fn apply_response_metadata(metadata: &mut SourceMetadata, response: &reqwest::Response) {
    if let Some(filename) = response
        .headers()
        .get(CONTENT_DISPOSITION)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_content_disposition_filename)
        .filter(|value| !value.is_empty())
    {
        metadata.filename = filename;
    } else if metadata.filename.is_empty() {
        metadata.filename = filename_from_url(response.url().as_str());
    }

    if metadata.size_bytes.is_none() {
        metadata.size_bytes = response
            .headers()
            .get(CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .and_then(parse_content_range_size)
            .or_else(|| {
                response
                    .headers()
                    .get(CONTENT_LENGTH)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.parse::<u64>().ok())
            });
    }
}

fn parse_content_disposition_filename(value: &str) -> Option<String> {
    value
        .split(';')
        .map(str::trim)
        .find_map(|part| part.strip_prefix("filename=").map(trim_quotes))
        .map(str::to_string)
}

fn trim_quotes(value: &str) -> &str {
    value.trim().trim_matches('"')
}

fn parse_content_range_size(value: &str) -> Option<u64> {
    value.rsplit('/').next()?.parse::<u64>().ok()
}

fn filename_from_url(url: &str) -> String {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return String::from("Unknown");
    };

    if let Some(path_value) = parsed
        .query_pairs()
        .find_map(|(key, value)| (key == "path").then_some(value.into_owned()))
        && let Some(name) = path_value.rsplit('/').find(|segment| !segment.is_empty())
    {
        return name.to_string();
    }

    parsed
        .path_segments()
        .and_then(|mut segments| segments.rfind(|segment| !segment.is_empty()))
        .map(str::to_string)
        .unwrap_or_else(|| String::from("Unknown"))
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];

    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::{
        SeekSupport, classify_seek_support_parts, filename_from_url, format_bytes,
        parse_content_disposition_filename, parse_content_range_size,
    };
    use reqwest::StatusCode;
    use reqwest::header::{ACCEPT_RANGES, CONTENT_RANGE, HeaderMap, HeaderValue};

    #[test]
    fn extracts_filename_from_path_query() {
        let filename =
            filename_from_url("https://example.com/raw?path=/folder/Kaiju.No.8.S03E01.mkv");
        assert_eq!(filename, "Kaiju.No.8.S03E01.mkv");
    }

    #[test]
    fn extracts_filename_from_content_disposition() {
        let filename =
            parse_content_disposition_filename("attachment; filename=\"video.mkv\"").unwrap();
        assert_eq!(filename, "video.mkv");
    }

    #[test]
    fn parses_content_range_total_size() {
        assert_eq!(parse_content_range_size("bytes 0-0/12345"), Some(12345));
    }

    #[test]
    fn formats_bytes_human_readably() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1_073_741_824), "1.00 GiB");
    }

    #[test]
    fn accepts_explicit_byte_ranges() {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT_RANGES, HeaderValue::from_static("bytes"));
        assert_eq!(
            classify_seek_support_parts(StatusCode::OK, &headers),
            Some(SeekSupport::Enabled)
        );
    }

    #[test]
    fn accepts_partial_content_responses() {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_RANGE, HeaderValue::from_static("bytes 0-0/123"));
        assert_eq!(
            classify_seek_support_parts(StatusCode::PARTIAL_CONTENT, &headers),
            Some(SeekSupport::Enabled)
        );
    }

    #[test]
    fn disables_seeking_when_ranges_are_missing() {
        let headers = HeaderMap::new();
        assert_eq!(
            classify_seek_support_parts(StatusCode::OK, &headers),
            Some(SeekSupport::Disabled {
                warning: String::from(
                    "This source doesn't appear to support jumping to a different time.",
                ),
            })
        );
    }
}
