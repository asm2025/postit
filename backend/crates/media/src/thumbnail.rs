//! JPEG thumbnails, longest side 480 px.

use std::path::{Path, PathBuf};

use image::ImageReader;
use image::codecs::jpeg::JpegEncoder;
use postit_core::{MediaKind, ProbeInfo};

const MAX_SIDE: u32 = 480;
const QUALITY: u8 = 80;
const MAX_ALLOC: u64 = 512 * 1024 * 1024;

/// Makes thumbnails. A failure never fails an upload: `make` answers `false`.
#[derive(Debug, Clone)]
pub struct Thumbnailer {
    max_pixels: u64,
    #[cfg_attr(not(feature = "ffprobe"), allow(dead_code))]
    ffmpeg: PathBuf,
}

fn image_thumbnail(source: &Path, dest: &Path) -> Result<(), image::ImageError> {
    let mut reader = ImageReader::open(source)?.with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MAX_ALLOC);
    reader.limits(limits);
    let rgb = reader.decode()?.thumbnail(MAX_SIDE, MAX_SIDE).to_rgb8();
    let file = std::fs::File::create(dest)?;
    let result = rgb.write_with_encoder(JpegEncoder::new_with_quality(
        std::io::BufWriter::new(file),
        QUALITY,
    ));
    if result.is_err() {
        let _ = std::fs::remove_file(dest);
    }
    result
}

impl Thumbnailer {
    /// A thumbnailer that skips images with more than `max_pixels` pixels and cuts video frames
    /// with the `ffmpeg` executable at `ffmpeg`.
    #[must_use]
    pub fn new(max_pixels: u64, ffmpeg: PathBuf) -> Self {
        Self { max_pixels, ffmpeg }
    }

    /// Writes a thumbnail of `source` to `dest`; `true` when one was written.
    pub async fn make(
        &self,
        source: &Path,
        kind: MediaKind,
        probe: &ProbeInfo,
        dest: &Path,
    ) -> bool {
        match kind {
            MediaKind::Image => {
                let pixels =
                    u64::from(probe.width.unwrap_or(0)) * u64::from(probe.height.unwrap_or(0));
                if pixels == 0 || pixels > self.max_pixels {
                    return false;
                }
                let (source, dest) = (source.to_path_buf(), dest.to_path_buf());
                tokio::task::spawn_blocking(move || image_thumbnail(&source, &dest).is_ok())
                    .await
                    .unwrap_or(false)
            }
            MediaKind::Video => {
                #[cfg(feature = "ffprobe")]
                {
                    self.video_thumbnail(source, dest).await
                }
                #[cfg(not(feature = "ffprobe"))]
                {
                    let _ = (source, dest);
                    false
                }
            }
        }
    }

    #[cfg(feature = "ffprobe")]
    async fn video_thumbnail(&self, source: &Path, dest: &Path) -> bool {
        let status = tokio::process::Command::new(&self.ffmpeg)
            .args(["-v", "error", "-y", "-ss", "0", "-i"])
            .arg(source)
            .args([
                "-frames:v",
                "1",
                "-vf",
                "scale=w='min(480,iw)':h='min(480,ih)':force_original_aspect_ratio=decrease",
            ])
            .arg(dest)
            .kill_on_drop(true)
            .status()
            .await;
        let made = status.is_ok_and(|s| s.success()) && dest.exists();
        if !made {
            let _ = tokio::fs::remove_file(dest).await;
        }
        made
    }
}
