use crate::{Result, RuntimeError, diagnostics::render_command};
use quickbridge_core::MediaInfo;
use std::{ffi::OsString, process::Stdio};
use tokio::process::Command;
use tracing::debug;

#[derive(Debug)]
pub struct ProbeRunner {
    binary: OsString,
}

impl ProbeRunner {
    pub fn new() -> Self {
        let binary = std::env::var_os("QUICKBRIDGE_FFPROBE_BIN")
            .unwrap_or_else(|| OsString::from("ffprobe"));
        Self { binary }
    }

    #[cfg(test)]
    pub fn with_binary(binary: impl Into<OsString>) -> Self {
        Self {
            binary: binary.into(),
        }
    }

    pub async fn ensure_available(&self) -> Result<()> {
        let status = match Command::new(&self.binary)
            .arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
        {
            Ok(status) => status,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(RuntimeError::FfprobeUnavailable {
                    binary: self.binary.to_string_lossy().into_owned(),
                });
            }
            Err(error) => {
                return Err(RuntimeError::ExecuteBinary {
                    binary: self.binary.to_string_lossy().into_owned(),
                    source: error,
                });
            }
        };

        if !status.success() {
            return Err(RuntimeError::FfprobeUnavailable {
                binary: self.binary.to_string_lossy().into_owned(),
            });
        }

        Ok(())
    }

    pub async fn probe(&self, source_url: &str) -> Result<MediaInfo> {
        debug!(source_url, "probing source with ffprobe");
        let json = self.probe_json(source_url).await?;
        MediaInfo::from_ffprobe_json(&json).map_err(RuntimeError::from)
    }

    pub fn render_probe_commands(&self, source_url: &str) -> Vec<String> {
        vec![render_command(
            &self.binary,
            &[
                OsString::from("-v"),
                OsString::from("error"),
                OsString::from("-show_streams"),
                OsString::from("-show_format"),
                OsString::from("-of"),
                OsString::from("json"),
                OsString::from(source_url),
            ],
        )]
    }

    async fn probe_json(&self, source_url: &str) -> Result<String> {
        let output = Command::new(&self.binary)
            .args([
                "-v",
                "error",
                "-show_streams",
                "-show_format",
                "-of",
                "json",
            ])
            .arg(source_url)
            .output()
            .await
            .map_err(|source| RuntimeError::ExecuteBinary {
                binary: self.binary.to_string_lossy().into_owned(),
                source,
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(RuntimeError::FfprobeFailed {
                stderr: stderr.trim().to_string(),
            });
        }

        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

impl Default for ProbeRunner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::ProbeRunner;
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    static SCRIPT_COUNTER: AtomicU64 = AtomicU64::new(1);

    #[cfg(unix)]
    #[tokio::test]
    async fn probes_with_fake_ffprobe() {
        let log_path = std::env::temp_dir().join(format!(
            "quickbridge-fake-ffprobe-log-{}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis(),
            SCRIPT_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let script = write_script(&format!(
            r#"#!/bin/sh
set -eu
printf '%s\n' "$*" >> "{}"
if [ "${{1:-}}" = "-version" ]; then
  exit 0
fi
if [ "${{1:-}}" = "-v" ]; then
cat <<'JSON'
{{"streams":[{{"index":0,"codec_type":"video","codec_name":"h264","profile":"High","pix_fmt":"yuv420p","width":1280,"height":720,"disposition":{{"default":1}}}},{{"index":1,"codec_type":"audio","codec_name":"aac","sample_rate":"48000","channel_layout":"stereo","sample_fmt":"fltp","bit_rate":"160000","tags":{{"language":"eng"}},"disposition":{{"default":1}}}}],"format":{{"duration":"65.0"}}}}
JSON
exit 0
fi
exit 1
"#,
            log_path.display()
        ));

        let runner = ProbeRunner::with_binary(&script);
        runner.ensure_available().await.unwrap();
        let media = runner.probe("https://example.com/video.mkv").await.unwrap();
        assert!(
            media.render_input_file().contains(
                "Stream #0:1(eng): Audio: aac, 48000 Hz, stereo, fltp, 160 kb/s (default)"
            )
        );
        assert_eq!(media.duration().unwrap().to_string(), "00:01:05");
        let invocations = fs::read_to_string(log_path).unwrap();
        let lines = invocations.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "-version");
        assert_eq!(
            lines[1],
            "-v error -show_streams -show_format -of json https://example.com/video.mkv"
        );
    }

    #[cfg(unix)]
    fn write_script(contents: &str) -> String {
        use std::os::unix::fs::PermissionsExt;

        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let unique = SCRIPT_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("quickbridge-fake-ffprobe-{millis}-{unique}.sh"));
        fs::write(&path, contents).unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).unwrap();
        path.to_string_lossy().into_owned()
    }
}
