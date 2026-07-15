use crate::Timecode;
use serde::Deserialize;
use thiserror::Error;

/// Metadata discovered about the media source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceMetadata {
    filename: String,
    size_bytes: Option<u64>,
}

impl SourceMetadata {
    pub fn new(filename: impl Into<String>, size_bytes: Option<u64>) -> Self {
        Self {
            filename: filename.into(),
            size_bytes,
        }
    }

    pub fn filename(&self) -> &str {
        &self.filename
    }

    pub fn size_bytes(&self) -> Option<u64> {
        self.size_bytes
    }

    pub fn display_size(&self) -> String {
        self.size_bytes
            .map(format_bytes)
            .unwrap_or_else(|| String::from("Unknown"))
    }
}

/// Whether the source supports seeking via HTTP range requests.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SeekSupport {
    Enabled,
    Disabled { warning: String },
}

/// Inspection result for the input source URL.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceInspection {
    metadata: SourceMetadata,
    seek_support: SeekSupport,
}

impl SourceInspection {
    pub fn new(metadata: SourceMetadata, seek_support: SeekSupport) -> Self {
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

/// Parsed ffprobe metadata for the input source.
#[derive(Clone, Debug)]
pub struct MediaInfo {
    videos: Vec<VideoStream>,
    audios: Vec<AudioStream>,
    duration: Option<Timecode>,
}

impl MediaInfo {
    pub fn new(
        videos: Vec<VideoStream>,
        audios: Vec<AudioStream>,
        duration: Option<Timecode>,
    ) -> Self {
        Self {
            videos,
            audios,
            duration,
        }
    }

    pub fn duration(&self) -> Option<Timecode> {
        self.duration
    }

    pub fn from_ffprobe_json(json: &str) -> Result<Self, MediaInfoParseError> {
        let parsed: FfprobeOutput = serde_json::from_str(json)?;

        let mut videos = Vec::new();
        let mut audios = Vec::new();
        for stream in parsed.streams {
            let display_line = display_stream_line(&stream);

            match stream.codec_type.as_deref() {
                Some("video")
                    if !stream
                        .disposition
                        .as_ref()
                        .and_then(FfprobeDisposition::is_attached_pic)
                        .unwrap_or(false) =>
                {
                    videos.push(VideoStream {
                        stream_index: stream.index,
                        codec_name: stream.codec_name.clone(),
                        profile: stream.profile.clone(),
                        level: stream.level,
                        codec_tag: stream.codec_tag_string.clone(),
                        pixel_format: stream.pix_fmt.clone(),
                        width: stream.width,
                        height: stream.height,
                        frame_rate: stream.r_frame_rate.clone(),
                        bit_rate: stream
                            .bit_rate
                            .as_deref()
                            .and_then(|value| value.parse().ok()),
                        color_primaries: stream.color_primaries.clone(),
                        color_transfer: stream.color_transfer.clone(),
                        color_matrix: stream.color_space.clone(),
                        dolby_vision: stream
                            .side_data_list
                            .iter()
                            .find(|data| {
                                data.side_data_type.as_deref() == Some("DOVI configuration record")
                            })
                            .and_then(FfprobeSideData::dolby_vision),
                        display_line,
                        is_default: stream
                            .disposition
                            .as_ref()
                            .and_then(FfprobeDisposition::is_default)
                            .unwrap_or(false),
                    })
                }
                Some("audio") => audios.push(AudioStream {
                    stream_index: stream.index,
                    codec_name: stream.codec_name,
                    profile: stream.profile,
                    language: stream.tags.as_ref().and_then(|tags| tags.language.clone()),
                    title: stream.tags.as_ref().and_then(|tags| tags.title.clone()),
                    channel_layout: stream.channel_layout,
                    channels: stream.channels,
                    display_line,
                    is_default: stream
                        .disposition
                        .as_ref()
                        .and_then(FfprobeDisposition::is_default)
                        .unwrap_or(false),
                }),
                Some("subtitle") => {}
                _ => {}
            }
        }

        videos.sort_by_key(|stream| stream.stream_index);
        audios.sort_by_key(|stream| stream.stream_index);

        let duration = parsed
            .format
            .as_ref()
            .and_then(|format| format.duration.as_deref())
            .and_then(|value| value.parse::<f64>().ok())
            .and_then(Timecode::from_seconds_f64);

        Ok(Self {
            videos,
            audios,
            duration,
        })
    }

    pub fn videos(&self) -> &[VideoStream] {
        &self.videos
    }

    pub fn audios(&self) -> &[AudioStream] {
        &self.audios
    }

    pub fn render_input_file(&self) -> String {
        let mut lines = Vec::new();

        if !self.videos.is_empty() {
            lines.push(String::from("  Video"));
            lines.extend(
                self.videos
                    .iter()
                    .map(|stream| format!("    {}", stream.display_line())),
            );
        }

        if !self.audios.is_empty() {
            if !lines.is_empty() {
                lines.push(String::new());
            }
            lines.push(String::from("  Audio"));
            lines.extend(
                self.audios
                    .iter()
                    .map(|stream| format!("    {}", stream.display_line())),
            );
        }

        if lines.is_empty() {
            String::from("  No supported tracks found")
        } else {
            lines.join("\n")
        }
    }

    pub fn selection_request(&self) -> Result<TrackSelectionRequest, TrackSelectionError> {
        if self.videos.is_empty() {
            return Err(TrackSelectionError::NoVideoTrack);
        }

        Ok(TrackSelectionRequest {
            videos: self.videos.clone(),
            audios: self.audios.clone(),
            default_video_index: default_index(&self.videos),
            default_audio_index: (!self.audios.is_empty()).then(|| default_index(&self.audios)),
        })
    }

    pub fn default_selection(&self) -> Result<StreamSelection, TrackSelectionError> {
        let request = self.selection_request()?;
        request.build_selection(request.default_video_index, request.default_audio_index)
    }
}

/// Prompt model used by the UI when selecting tracks.
#[derive(Clone, Debug)]
pub struct TrackSelectionRequest {
    videos: Vec<VideoStream>,
    audios: Vec<AudioStream>,
    default_video_index: usize,
    default_audio_index: Option<usize>,
}

impl TrackSelectionRequest {
    pub fn videos(&self) -> &[VideoStream] {
        &self.videos
    }

    pub fn audios(&self) -> &[AudioStream] {
        &self.audios
    }

    pub fn default_video_index(&self) -> usize {
        self.default_video_index
    }

    pub fn default_audio_index(&self) -> Option<usize> {
        self.default_audio_index
    }

    pub fn build_selection(
        &self,
        video_index: usize,
        audio_index: Option<usize>,
    ) -> Result<StreamSelection, TrackSelectionError> {
        let video = self
            .videos
            .get(video_index)
            .cloned()
            .ok_or(TrackSelectionError::VideoIndexOutOfRange { index: video_index })?;
        let audio = match (self.audios.is_empty(), audio_index) {
            (true, _) => None,
            (false, Some(index)) => Some(
                self.audios
                    .get(index)
                    .cloned()
                    .ok_or(TrackSelectionError::AudioIndexOutOfRange { index })?,
            ),
            (false, None) => return Err(TrackSelectionError::AudioSelectionRequired),
        };

        Ok(StreamSelection::new(video, audio))
    }
}

/// Errors produced when choosing audio and video tracks.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum TrackSelectionError {
    #[error("the source does not contain a supported video track")]
    NoVideoTrack,
    #[error("video selection index {index} is out of range")]
    VideoIndexOutOfRange { index: usize },
    #[error("audio selection index {index} is out of range")]
    AudioIndexOutOfRange { index: usize },
    #[error("an audio track selection is required")]
    AudioSelectionRequired,
}

/// Errors returned while parsing ffprobe JSON output.
#[derive(Debug, Error)]
pub enum MediaInfoParseError {
    #[error("unable to read ffprobe output")]
    InvalidJson(#[from] serde_json::Error),
}

/// Apple-oriented MP4 packaging for a selected video stream.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VideoPackaging {
    tag: &'static str,
    bitstream_filter: Option<&'static str>,
    unofficial: bool,
}

impl VideoPackaging {
    fn new(tag: &'static str, bitstream_filter: Option<&'static str>, unofficial: bool) -> Self {
        Self {
            tag,
            bitstream_filter,
            unofficial,
        }
    }

    pub fn tag(&self) -> &'static str {
        self.tag
    }

    pub fn bitstream_filter(&self) -> Option<&'static str> {
        self.bitstream_filter
    }

    pub fn is_unofficial(&self) -> bool {
        self.unofficial
    }
}

/// Why a selected video stream cannot be safely packaged for Apple playback.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum VideoPackagingError {
    #[error("video stream #{stream_index} is missing codec metadata")]
    MissingCodec { stream_index: usize },
    #[error("video stream #{stream_index} uses unsupported codec `{codec}`")]
    UnsupportedCodec { stream_index: usize, codec: String },
    #[error("video stream #{stream_index} uses unsupported Dolby Vision profile {profile}")]
    UnsupportedDolbyVisionProfile { stream_index: usize, profile: u8 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DolbyVisionConfig {
    profile: u8,
    level: u8,
    base_layer_present: bool,
    enhancement_layer_present: bool,
    base_layer_signal_compatibility_id: u8,
}

impl DolbyVisionConfig {
    pub fn profile(&self) -> u8 {
        self.profile
    }
    pub fn level(&self) -> u8 {
        self.level
    }
    pub fn base_layer_present(&self) -> bool {
        self.base_layer_present
    }
    pub fn enhancement_layer_present(&self) -> bool {
        self.enhancement_layer_present
    }
    pub fn base_layer_signal_compatibility_id(&self) -> u8 {
        self.base_layer_signal_compatibility_id
    }
}

/// Video stream metadata rendered in the UI.
#[derive(Clone, Debug)]
pub struct VideoStream {
    pub stream_index: usize,
    codec_name: Option<String>,
    profile: Option<String>,
    level: Option<u32>,
    codec_tag: Option<String>,
    pixel_format: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    frame_rate: Option<String>,
    bit_rate: Option<u64>,
    color_primaries: Option<String>,
    color_transfer: Option<String>,
    color_matrix: Option<String>,
    dolby_vision: Option<DolbyVisionConfig>,
    display_line: String,
    is_default: bool,
}

impl VideoStream {
    pub fn new(stream_index: usize, display_line: impl Into<String>, is_default: bool) -> Self {
        Self {
            stream_index,
            codec_name: Some(String::from("h264")),
            profile: None,
            level: None,
            codec_tag: None,
            pixel_format: None,
            width: None,
            height: None,
            frame_rate: None,
            bit_rate: None,
            color_primaries: None,
            color_transfer: None,
            color_matrix: None,
            dolby_vision: None,
            display_line: display_line.into(),
            is_default,
        }
    }

    pub fn display_line(&self) -> &str {
        &self.display_line
    }

    pub fn codec_name(&self) -> Option<&str> {
        self.codec_name.as_deref()
    }
    pub fn profile(&self) -> Option<&str> {
        self.profile.as_deref()
    }
    pub fn level(&self) -> Option<u32> {
        self.level
    }
    pub fn codec_tag(&self) -> Option<&str> {
        self.codec_tag.as_deref()
    }
    pub fn pixel_format(&self) -> Option<&str> {
        self.pixel_format.as_deref()
    }
    pub fn dimensions(&self) -> Option<(u32, u32)> {
        self.width.zip(self.height)
    }
    pub fn frame_rate(&self) -> Option<&str> {
        self.frame_rate.as_deref()
    }
    pub fn bit_rate(&self) -> Option<u64> {
        self.bit_rate
    }
    pub fn color_primaries(&self) -> Option<&str> {
        self.color_primaries.as_deref()
    }
    pub fn color_transfer(&self) -> Option<&str> {
        self.color_transfer.as_deref()
    }
    pub fn color_matrix(&self) -> Option<&str> {
        self.color_matrix.as_deref()
    }
    pub fn dolby_vision(&self) -> Option<&DolbyVisionConfig> {
        self.dolby_vision.as_ref()
    }
    pub fn is_default(&self) -> bool {
        self.is_default
    }

    pub fn apple_packaging(&self) -> Result<VideoPackaging, VideoPackagingError> {
        match self.codec_name.as_deref() {
            Some("h264") => match self.dolby_vision.as_ref() {
                None => Ok(VideoPackaging::new("avc1", None, false)),
                Some(config) if config.profile == 9 => Ok(VideoPackaging::new("dva1", None, true)),
                Some(config) => Err(VideoPackagingError::UnsupportedDolbyVisionProfile {
                    stream_index: self.stream_index,
                    profile: config.profile,
                }),
            },
            Some("hevc") => match self.dolby_vision.as_ref() {
                None => Ok(VideoPackaging::new("hvc1", Some("hevc_metadata"), false)),
                Some(config) if matches!(config.profile, 5 | 8) => {
                    Ok(VideoPackaging::new("dvh1", Some("hevc_metadata"), true))
                }
                Some(config) => Err(VideoPackagingError::UnsupportedDolbyVisionProfile {
                    stream_index: self.stream_index,
                    profile: config.profile,
                }),
            },
            Some("av1") => Ok(VideoPackaging::new("av01", None, false)),
            Some("vc1") => Ok(VideoPackaging::new("vc-1", None, false)),
            Some(codec) => Err(VideoPackagingError::UnsupportedCodec {
                stream_index: self.stream_index,
                codec: codec.to_string(),
            }),
            None => Err(VideoPackagingError::MissingCodec {
                stream_index: self.stream_index,
            }),
        }
    }
}

/// Audio stream metadata rendered in the UI.
#[derive(Clone, Debug)]
pub struct AudioStream {
    pub stream_index: usize,
    pub codec_name: Option<String>,
    profile: Option<String>,
    language: Option<String>,
    title: Option<String>,
    channel_layout: Option<String>,
    channels: Option<u32>,
    display_line: String,
    is_default: bool,
}

impl AudioStream {
    pub fn new(
        stream_index: usize,
        codec_name: Option<String>,
        display_line: impl Into<String>,
        is_default: bool,
    ) -> Self {
        Self {
            stream_index,
            codec_name,
            profile: None,
            language: None,
            title: None,
            channel_layout: None,
            channels: None,
            display_line: display_line.into(),
            is_default,
        }
    }

    pub fn display_line(&self) -> &str {
        &self.display_line
    }

    pub fn profile(&self) -> Option<&str> {
        self.profile.as_deref()
    }
    pub fn language(&self) -> Option<&str> {
        self.language.as_deref()
    }
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }
    pub fn channel_layout(&self) -> Option<&str> {
        self.channel_layout.as_deref()
    }
    pub fn channels(&self) -> Option<u32> {
        self.channels
    }
    pub fn is_atmos(&self) -> bool {
        self.profile
            .as_deref()
            .is_some_and(|profile| profile.to_ascii_lowercase().contains("atmos"))
            || self
                .title
                .as_deref()
                .is_some_and(|title| title.to_ascii_lowercase().contains("atmos"))
    }
    pub fn is_default(&self) -> bool {
        self.is_default
    }
}

trait DefaultTrack {
    fn is_default(&self) -> bool;
}

impl DefaultTrack for VideoStream {
    fn is_default(&self) -> bool {
        self.is_default
    }
}

impl DefaultTrack for AudioStream {
    fn is_default(&self) -> bool {
        self.is_default
    }
}

fn default_index<T: DefaultTrack>(tracks: &[T]) -> usize {
    tracks
        .iter()
        .position(DefaultTrack::is_default)
        .unwrap_or(0)
}

/// How quickbridge should handle the selected audio track.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AudioHandling {
    Copy,
    TranscodeAlac,
}

/// Final user-selected stream combination.
#[derive(Clone, Debug)]
pub struct StreamSelection {
    video: VideoStream,
    audio: Option<AudioStream>,
    audio_handling: Option<AudioHandling>,
}

impl StreamSelection {
    pub fn new(video: VideoStream, audio: Option<AudioStream>) -> Self {
        let audio_handling = audio.as_ref().map(|stream| {
            if should_transcode_audio(stream.codec_name.as_deref()) {
                AudioHandling::TranscodeAlac
            } else {
                AudioHandling::Copy
            }
        });

        Self {
            video,
            audio,
            audio_handling,
        }
    }

    pub fn video_stream_index(&self) -> usize {
        self.video.stream_index
    }

    pub fn selected_video(&self) -> &VideoStream {
        &self.video
    }

    pub fn audio_stream_index(&self) -> Option<usize> {
        self.audio.as_ref().map(|stream| stream.stream_index)
    }

    pub fn selected_audio(&self) -> Option<&AudioStream> {
        self.audio.as_ref()
    }

    pub fn audio_handling(&self) -> Option<&AudioHandling> {
        self.audio_handling.as_ref()
    }

    pub fn video_packaging(&self) -> Result<VideoPackaging, VideoPackagingError> {
        self.video.apple_packaging()
    }

    pub fn render_output_file(&self) -> String {
        let mut lines = vec![self.video.display_line().to_string()];
        if let Some(audio) = &self.audio {
            lines.push(audio.display_line().to_string());
        }
        lines.join("\n")
    }

    pub fn selected_audio_summary(&self) -> Option<String> {
        self.audio
            .as_ref()
            .map(|audio| audio.display_line().to_string())
    }

    pub fn audio_notice(&self) -> Option<String> {
        match (&self.audio, self.audio_handling()) {
            (Some(audio), Some(AudioHandling::TranscodeAlac)) => Some(format!(
                "Audio track #{} uses {}. quickbridge will convert it to ALAC so QuickTime Player can play it.",
                audio.stream_index,
                audio
                    .codec_name
                    .as_deref()
                    .unwrap_or("an unsupported codec")
            )),
            _ => None,
        }
    }
}

fn should_transcode_audio(codec_name: Option<&str>) -> bool {
    matches!(codec_name, Some("dts" | "truehd"))
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

fn display_stream_line(stream: &FfprobeStream) -> String {
    match stream.codec_type.as_deref() {
        Some("video") => display_video_stream_line(stream),
        Some("audio") => display_audio_stream_line(stream),
        _ => fallback_stream_line(stream),
    }
}

fn display_video_stream_line(stream: &FfprobeStream) -> String {
    let codec = stream.codec_name.as_deref().unwrap_or("unknown");
    let profile_suffix = stream
        .profile
        .as_deref()
        .filter(|profile| !profile.is_empty())
        .map(|profile| format!(" ({profile})"))
        .unwrap_or_default();

    let mut details = Vec::new();
    if let Some(pix_fmt) = stream.pix_fmt.as_deref().filter(|value| !value.is_empty()) {
        let mut qualifiers = Vec::new();
        if let Some(color_range) = stream
            .color_range
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            qualifiers.push(color_range);
        }
        if let Some(color_space) = stream
            .color_space
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            qualifiers.push(color_space);
        }
        if let Some(field_order) = stream
            .field_order
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            qualifiers.push(field_order);
        }
        if qualifiers.is_empty() {
            details.push(pix_fmt.to_string());
        } else {
            details.push(format!("{pix_fmt}({})", qualifiers.join(", ")));
        }
    }
    if let (Some(width), Some(height)) = (stream.width, stream.height) {
        details.push(format!("{width}x{height}"));
    }

    format!(
        "Stream #0:{}{}: Video: {}{}{}{}",
        stream.index,
        language_suffix(stream.tags.as_ref()),
        codec,
        profile_suffix,
        detail_suffix(&details),
        default_suffix(stream)
    )
}

fn display_audio_stream_line(stream: &FfprobeStream) -> String {
    let codec = stream.codec_name.as_deref().unwrap_or("unknown");

    let mut details = Vec::new();
    if let Some(sample_rate) = stream
        .sample_rate
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        details.push(format!("{sample_rate} Hz"));
    }
    if let Some(channel_layout) = stream
        .channel_layout
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        details.push(channel_layout.to_string());
    } else if let Some(channels) = stream.channels {
        details.push(format_channel_count(channels));
    }
    if let Some(sample_fmt) = stream
        .sample_fmt
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        details.push(sample_fmt.to_string());
    }
    if let Some(bit_rate) = stream
        .bit_rate
        .as_deref()
        .and_then(|value| value.parse::<u64>().ok())
    {
        details.push(format!("{} kb/s", bit_rate / 1000));
    }

    format!(
        "Stream #0:{}{}: Audio: {}{}{}",
        stream.index,
        language_suffix(stream.tags.as_ref()),
        codec,
        detail_suffix(&details),
        default_suffix(stream)
    )
}

fn fallback_stream_line(stream: &FfprobeStream) -> String {
    let kind = stream.codec_type.as_deref().unwrap_or("unknown");
    let codec = stream.codec_name.as_deref().unwrap_or("unknown");
    format!(
        "Stream #0:{}: {}: {}",
        stream.index,
        capitalize(kind),
        codec
    )
}

fn capitalize(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn language_suffix(tags: Option<&FfprobeTags>) -> String {
    tags.and_then(|tags| tags.language.as_deref())
        .filter(|language| !language.is_empty() && *language != "und")
        .map(|language| format!("({language})"))
        .unwrap_or_default()
}

fn detail_suffix(details: &[String]) -> String {
    if details.is_empty() {
        String::new()
    } else {
        format!(", {}", details.join(", "))
    }
}

fn default_suffix(stream: &FfprobeStream) -> &'static str {
    if stream
        .disposition
        .as_ref()
        .and_then(FfprobeDisposition::is_default)
        .unwrap_or(false)
    {
        " (default)"
    } else {
        ""
    }
}

fn format_channel_count(channels: u32) -> String {
    match channels {
        1 => String::from("mono"),
        2 => String::from("stereo"),
        6 => String::from("5.1"),
        8 => String::from("7.1"),
        _ => format!("{channels} channels"),
    }
}

#[derive(Debug, Deserialize)]
struct FfprobeOutput {
    #[serde(default)]
    streams: Vec<FfprobeStream>,
    format: Option<FfprobeFormat>,
}

#[derive(Debug, Deserialize)]
struct FfprobeStream {
    index: usize,
    codec_type: Option<String>,
    codec_name: Option<String>,
    profile: Option<String>,
    level: Option<u32>,
    codec_tag_string: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    pix_fmt: Option<String>,
    color_range: Option<String>,
    color_space: Option<String>,
    color_primaries: Option<String>,
    color_transfer: Option<String>,
    r_frame_rate: Option<String>,
    field_order: Option<String>,
    sample_fmt: Option<String>,
    sample_rate: Option<String>,
    channels: Option<u32>,
    channel_layout: Option<String>,
    bit_rate: Option<String>,
    disposition: Option<FfprobeDisposition>,
    tags: Option<FfprobeTags>,
    #[serde(default)]
    side_data_list: Vec<FfprobeSideData>,
}

#[derive(Debug, Deserialize)]
struct FfprobeSideData {
    side_data_type: Option<String>,
    dv_profile: Option<u8>,
    dv_level: Option<u8>,
    bl_present_flag: Option<u8>,
    el_present_flag: Option<u8>,
    dv_bl_signal_compatibility_id: Option<u8>,
}

impl FfprobeSideData {
    fn dolby_vision(&self) -> Option<DolbyVisionConfig> {
        Some(DolbyVisionConfig {
            profile: self.dv_profile?,
            level: self.dv_level?,
            base_layer_present: self.bl_present_flag? != 0,
            enhancement_layer_present: self.el_present_flag? != 0,
            base_layer_signal_compatibility_id: self.dv_bl_signal_compatibility_id?,
        })
    }
}

#[derive(Debug, Deserialize)]
struct FfprobeFormat {
    duration: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FfprobeTags {
    language: Option<String>,
    title: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FfprobeDisposition {
    default: Option<u8>,
    attached_pic: Option<u8>,
}

impl FfprobeDisposition {
    fn is_default(&self) -> Option<bool> {
        self.default.map(|value| value != 0)
    }
    fn is_attached_pic(&self) -> Option<bool> {
        self.attached_pic.map(|value| value != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AudioHandling, AudioStream, MediaInfo, SeekSupport, SourceInspection, SourceMetadata,
        StreamSelection, TrackSelectionError, VideoPackaging, VideoPackagingError, VideoStream,
    };
    use crate::Timecode;

    #[test]
    fn parses_video_capabilities_and_excludes_attached_art() {
        let media =
            MediaInfo::from_ffprobe_json(include_str!("../tests/fixtures/dv-p5-cover.json"))
                .unwrap();
        assert_eq!(media.videos().len(), 1);
        let video = &media.videos()[0];
        let dovi = video.dolby_vision().unwrap();
        assert_eq!((dovi.profile(), dovi.level()), (5, 6));
        assert!(dovi.base_layer_present());
        assert!(!dovi.enhancement_layer_present());
    }

    #[test]
    fn classifies_apple_video_packaging() {
        let cases = [
            (
                r#"{"streams":[{"index":0,"codec_type":"video","codec_name":"h264"}]}"#,
                Ok(VideoPackaging::new("avc1", None, false)),
            ),
            (
                r#"{"streams":[{"index":0,"codec_type":"video","codec_name":"h264","side_data_list":[{"side_data_type":"DOVI configuration record","dv_profile":9,"dv_level":6,"bl_present_flag":1,"el_present_flag":0,"dv_bl_signal_compatibility_id":1}]}]}"#,
                Ok(VideoPackaging::new("dva1", None, true)),
            ),
            (
                include_str!("../tests/fixtures/hevc-bt709.json"),
                Ok(VideoPackaging::new("hvc1", Some("hevc_metadata"), false)),
            ),
            (
                include_str!("../tests/fixtures/hdr10-pq.json"),
                Ok(VideoPackaging::new("hvc1", Some("hevc_metadata"), false)),
            ),
            (
                include_str!("../tests/fixtures/dv-p5.json"),
                Ok(VideoPackaging::new("dvh1", Some("hevc_metadata"), true)),
            ),
            (
                r#"{"streams":[{"index":0,"codec_type":"video","codec_name":"hevc","side_data_list":[{"side_data_type":"DOVI configuration record","dv_profile":8,"dv_level":6,"bl_present_flag":1,"el_present_flag":0,"dv_bl_signal_compatibility_id":4}]}]}"#,
                Ok(VideoPackaging::new("dvh1", Some("hevc_metadata"), true)),
            ),
            (
                r#"{"streams":[{"index":0,"codec_type":"video","codec_name":"av1"}]}"#,
                Ok(VideoPackaging::new("av01", None, false)),
            ),
            (
                r#"{"streams":[{"index":0,"codec_type":"video","codec_name":"vc1"}]}"#,
                Ok(VideoPackaging::new("vc-1", None, false)),
            ),
        ];
        for (json, expected) in cases {
            let video = MediaInfo::from_ffprobe_json(json)
                .unwrap()
                .videos()
                .first()
                .unwrap()
                .clone();
            assert_eq!(video.apple_packaging(), expected);
        }

        let rejected = [
            (
                "hevc",
                Some((7, 0)),
                VideoPackagingError::UnsupportedDolbyVisionProfile {
                    stream_index: 0,
                    profile: 7,
                },
            ),
            (
                "h264",
                Some((5, 0)),
                VideoPackagingError::UnsupportedDolbyVisionProfile {
                    stream_index: 0,
                    profile: 5,
                },
            ),
        ];
        for (codec, dovi, expected) in rejected {
            let side = dovi.map(|(profile, compatibility)| format!(r#", "side_data_list":[{{"side_data_type":"DOVI configuration record","dv_profile":{profile},"dv_level":6,"bl_present_flag":1,"el_present_flag":0,"dv_bl_signal_compatibility_id":{compatibility}}}]"#)).unwrap_or_default();
            let json = format!(
                r#"{{"streams":[{{"index":0,"codec_type":"video","codec_name":"{codec}"{side}}}]}}"#
            );
            let media = MediaInfo::from_ffprobe_json(&json).unwrap();
            assert_eq!(media.videos()[0].apple_packaging(), Err(expected));
        }
    }

    #[test]
    fn builds_default_selection_from_media_info() {
        let media = MediaInfo::new(
            vec![VideoStream::new(0, "Stream #0:0: Video: h264", true)],
            vec![AudioStream::new(
                1,
                Some(String::from("aac")),
                "Stream #0:1: Audio: aac",
                true,
            )],
            Some(Timecode::from_seconds(60)),
        );

        let selection = media.default_selection().unwrap();
        assert_eq!(selection.video_stream_index(), 0);
        assert_eq!(selection.audio_stream_index(), Some(1));
    }

    #[test]
    fn exposes_structured_audio_metadata_without_parsing_display_text() {
        let media = MediaInfo::from_ffprobe_json(
            r#"{"streams":[{"index":1,"codec_type":"audio","codec_name":"truehd","profile":"Dolby TrueHD + Dolby Atmos","channels":8,"channel_layout":"7.1","tags":{"language":"eng","title":"Original theatrical mix"},"disposition":{"default":1}}]}"#,
        )
        .unwrap();

        let audio = &media.audios()[0];
        assert_eq!(audio.codec_name.as_deref(), Some("truehd"));
        assert_eq!(audio.profile(), Some("Dolby TrueHD + Dolby Atmos"));
        assert_eq!(audio.language(), Some("eng"));
        assert_eq!(audio.title(), Some("Original theatrical mix"));
        assert_eq!(audio.channel_layout(), Some("7.1"));
        assert_eq!(audio.channels(), Some(8));
        assert!(audio.is_atmos());
        assert!(audio.is_default());
    }

    #[test]
    fn selection_request_requires_a_video_track() {
        let media = MediaInfo::new(Vec::new(), Vec::new(), None);
        assert_eq!(
            media.selection_request().unwrap_err(),
            TrackSelectionError::NoVideoTrack
        );
    }

    #[test]
    fn marks_dts_audio_for_transcode() {
        let selection = StreamSelection::new(
            VideoStream::new(0, "Stream #0:0: Video: h264", true),
            Some(AudioStream::new(
                1,
                Some(String::from("dts")),
                "Stream #0:1: Audio: dts",
                true,
            )),
        );

        assert_eq!(
            selection.audio_handling(),
            Some(&AudioHandling::TranscodeAlac)
        );
    }

    #[test]
    fn source_inspection_reports_seek_state() {
        let inspection = SourceInspection::new(
            SourceMetadata::new("video.mkv", Some(1024)),
            SeekSupport::Disabled {
                warning: String::from("No ranges"),
            },
        );

        assert_eq!(inspection.metadata().filename(), "video.mkv");
        assert_eq!(inspection.metadata().display_size(), "1.00 KiB");
        assert_eq!(inspection.seek_warning(), Some("No ranges"));
    }

    #[test]
    fn renders_input_and_output_streams_with_ffprobe_style() {
        let media = MediaInfo::from_ffprobe_json(
            r#"{
              "streams": [
                {"index": 0, "codec_type": "video", "codec_name": "h264", "profile": "High", "pix_fmt": "yuv420p", "width": 1920, "height": 1080, "disposition": {"default": 1}},
                {"index": 1, "codec_type": "audio", "codec_name": "dts", "sample_rate": "48000", "channel_layout": "5.1", "sample_fmt": "fltp", "bit_rate": "1536000", "tags": {"language": "eng"}, "disposition": {"default": 1}},
                {"index": 2, "codec_type": "subtitle", "codec_name": "subrip"}
              ],
              "format": {"duration": "1460.4"}
            }"#,
        )
        .unwrap();

        assert!(
            media
                .render_input_file()
                .contains("Stream #0:1(eng): Audio: dts, 48000 Hz, 5.1, fltp, 1536 kb/s (default)")
        );
        assert_eq!(media.duration().unwrap().to_string(), "00:24:20");

        let selection = media.default_selection().unwrap();
        assert!(
            selection
                .render_output_file()
                .contains("Stream #0:0: Video: h264 (High), yuv420p, 1920x1080 (default)")
        );
    }
}
