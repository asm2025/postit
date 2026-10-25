//! Reading dimensions and duration from stored files.

use std::path::Path;
#[cfg(feature = "ffprobe")]
use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use postit_core::{MediaError, MediaProbe, ProbeInfo};

fn unsupported(what: &str) -> MediaError {
    MediaError::Unsupported(what.to_string())
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// Probes in-process: image headers through `imagesize`, `MP4` and `QuickTime` movies through
/// `mp4`.
#[derive(Debug, Clone, Copy, Default)]
pub struct NativeProbe;

fn probe_image(path: &Path) -> Result<ProbeInfo, MediaError> {
    let size = imagesize::size(path).map_err(|_| unsupported("unreadable image"))?;
    Ok(ProbeInfo {
        width: u32::try_from(size.width).ok(),
        height: u32::try_from(size.height).ok(),
        duration_ms: None,
    })
}

fn probe_movie(path: &Path) -> Result<ProbeInfo, MediaError> {
    let file = std::fs::File::open(path).map_err(|e| MediaError::Io(e.to_string()))?;
    let size = file
        .metadata()
        .map_err(|e| MediaError::Io(e.to_string()))?
        .len();
    let reader = mp4::Mp4Reader::read_header(std::io::BufReader::new(file), size)
        .map_err(|_| unsupported("unreadable movie"))?;
    let video = reader
        .tracks()
        .values()
        .find(|t| t.track_type().is_ok_and(|k| k == mp4::TrackType::Video))
        .ok_or_else(|| unsupported("movie has no video track"))?;
    Ok(ProbeInfo {
        width: Some(u32::from(video.width())),
        height: Some(u32::from(video.height())),
        duration_ms: Some(millis(reader.duration())),
    })
}

#[async_trait]
impl MediaProbe for NativeProbe {
    async fn probe(&self, path: &Path, mime: &str) -> Result<ProbeInfo, MediaError> {
        let path = path.to_path_buf();
        let image = mime.starts_with("image/");
        tokio::task::spawn_blocking(move || {
            if image {
                probe_image(&path)
            } else {
                probe_movie(&path)
            }
        })
        .await
        .map_err(|e| MediaError::Io(e.to_string()))?
    }
}

/// Probes videos with `ffprobe`; images still go through [`NativeProbe`].
#[cfg(feature = "ffprobe")]
#[derive(Debug, Clone)]
pub struct FfprobeProbe {
    ffprobe: PathBuf,
}

#[cfg(feature = "ffprobe")]
impl FfprobeProbe {
    /// A probe that runs the `ffprobe` executable at `ffprobe`.
    #[must_use]
    pub fn new(ffprobe: PathBuf) -> Self {
        Self { ffprobe }
    }
}

#[cfg(feature = "ffprobe")]
#[async_trait]
impl MediaProbe for FfprobeProbe {
    async fn probe(&self, path: &Path, mime: &str) -> Result<ProbeInfo, MediaError> {
        if mime.starts_with("image/") {
            return NativeProbe.probe(path, mime).await;
        }
        let output = tokio::process::Command::new(&self.ffprobe)
            .args([
                "-v",
                "error",
                "-print_format",
                "json",
                "-show_streams",
                "-show_format",
            ])
            .arg(path)
            .kill_on_drop(true)
            .output()
            .await
            .map_err(|e| MediaError::Io(e.to_string()))?;
        if !output.status.success() {
            return Err(unsupported("ffprobe refused the file"));
        }
        let json: serde_json::Value =
            serde_json::from_slice(&output.stdout).map_err(|_| unsupported("ffprobe output"))?;
        let video = json["streams"]
            .as_array()
            .and_then(|s| s.iter().find(|s| s["codec_type"] == "video"))
            .ok_or_else(|| unsupported("movie has no video track"))?;
        let dim = |key: &str| video[key].as_u64().and_then(|v| u32::try_from(v).ok());
        let duration_ms = json["format"]["duration"]
            .as_str()
            .and_then(|d| d.parse::<f64>().ok())
            .filter(|d| d.is_finite() && *d >= 0.0)
            .map(|d| millis(Duration::from_secs_f64(d)));
        Ok(ProbeInfo {
            width: dim("width"),
            height: dim("height"),
            duration_ms,
        })
    }
}
