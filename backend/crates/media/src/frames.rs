//! Small JPEG stills of stored media for an AI model: images and GIFs downscaled, videos cut
//! into frames with `ffmpeg`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ImageDecoder as _, ImageReader};
use postit_core::{
    CONTEXT_MAX_FRAMES, CONTEXT_MAX_SIDE, ContextImage, FrameExtractor, MediaAsset, MediaError,
    MediaKind, VideoContext,
};

use crate::storage::FsMediaStore;
use crate::thumbnail::decode_limits;

const QUALITY: u8 = 85;

/// How long `ffmpeg` may take to cut a video's frames before it is killed.
#[cfg(feature = "ffprobe")]
const CUT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Makes the stills the drafter shows a model. Ownership and readiness are checked the way
/// [`FsMediaStore`] checks them for a download.
#[derive(Debug, Clone)]
pub struct MediaFrames {
    store: Arc<FsMediaStore>,
    max_pixels: u64,
    #[cfg_attr(not(feature = "ffprobe"), allow(dead_code))]
    ffmpeg: Option<PathBuf>,
}

fn unsupported(err: &impl std::fmt::Display) -> MediaError {
    MediaError::Unsupported(err.to_string())
}

/// Decodes the image at `path` under the shared limits and encodes it as a JPEG. The file is
/// opened once: the decoder reads the header first, so a huge image is refused before any
/// pixel is allocated.
fn still_from_path(path: &Path, max_pixels: u64) -> Result<ContextImage, MediaError> {
    let mut reader = ImageReader::open(path)
        .map_err(|e| MediaError::Io(e.to_string()))?
        .with_guessed_format()
        .map_err(|e| MediaError::Io(e.to_string()))?;
    reader.limits(decode_limits());
    let decoder = reader.into_decoder().map_err(|e| unsupported(&e))?;
    let (w, h) = decoder.dimensions();
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > max_pixels {
        return Err(MediaError::Unsupported(
            "image is too large to read".to_owned(),
        ));
    }
    let mut img = DynamicImage::from_decoder(decoder).map_err(|e| unsupported(&e))?;
    if w.max(h) > CONTEXT_MAX_SIDE {
        img = img.thumbnail(CONTEXT_MAX_SIDE, CONTEXT_MAX_SIDE);
    }
    let rgb = img.to_rgb8();
    let mut jpeg = Vec::new();
    rgb.write_with_encoder(JpegEncoder::new_with_quality(&mut jpeg, QUALITY))
        .map_err(|e| unsupported(&e))?;
    Ok(ContextImage {
        jpeg: jpeg.into(),
        width: rgb.width(),
        height: rgb.height(),
    })
}

impl MediaFrames {
    /// An extractor over `store`. It refuses images with more than `max_pixels` pixels (give it
    /// the value the `Thumbnailer` gets) and cuts video frames with the `ffmpeg` executable
    /// when one is given.
    #[must_use]
    pub fn new(store: Arc<FsMediaStore>, max_pixels: u64, ffmpeg: Option<PathBuf>) -> Self {
        Self {
            store,
            max_pixels,
            ffmpeg,
        }
    }

    async fn still_at(&self, path: PathBuf) -> Result<ContextImage, MediaError> {
        let max_pixels = self.max_pixels;
        tokio::task::spawn_blocking(move || still_from_path(&path, max_pixels))
            .await
            .map_err(|e| MediaError::Io(e.to_string()))?
    }
}

#[async_trait]
impl FrameExtractor for MediaFrames {
    async fn still(&self, asset: &MediaAsset) -> Result<ContextImage, MediaError> {
        if asset.kind != MediaKind::Image {
            return Err(MediaError::Unsupported(
                "a still is made from an image".to_owned(),
            ));
        }
        let path = self.store.locate(asset).await?;
        self.still_at(path).await
    }

    async fn frames(&self, asset: &MediaAsset, count: u32) -> Result<VideoContext, MediaError> {
        if asset.kind != MediaKind::Video {
            return Err(MediaError::Unsupported(
                "frames are cut from a video".to_owned(),
            ));
        }
        let count = count.min(CONTEXT_MAX_FRAMES);
        if count == 0 {
            return Ok(VideoContext {
                frames: Vec::new(),
                duration_ms: asset.duration_ms,
            });
        }
        let source = self.store.locate(asset).await?;
        // A server with no `ffmpeg` describes the video by its length alone.
        let no_frames = VideoContext {
            frames: Vec::new(),
            duration_ms: asset.duration_ms,
        };
        let Some(ffmpeg) = self.ffmpeg.as_deref() else {
            return Ok(no_frames);
        };
        #[cfg(feature = "ffprobe")]
        {
            let _ = no_frames;
            self.cut(ffmpeg, &source, asset, count).await
        }
        #[cfg(not(feature = "ffprobe"))]
        {
            let _ = (ffmpeg, source, count);
            Ok(no_frames)
        }
    }
}

#[cfg(feature = "ffprobe")]
impl MediaFrames {
    /// Cuts `count` (at least 1) frames of `source` with one `ffmpeg` run bounded by
    /// [`CUT_TIMEOUT`], with no terminal attached: the file is opened once per frame, each
    /// seeked to its own time, and every frame written in the same pass.
    async fn cut(
        &self,
        ffmpeg: &Path,
        source: &Path,
        asset: &MediaAsset,
        count: u32,
    ) -> Result<VideoContext, MediaError> {
        use std::process::Stdio;

        const SCALE: &str =
            "scale=w='min(1568,iw)':h='min(1568,ih)':force_original_aspect_ratio=decrease";

        let dir = tempfile::tempdir().map_err(|e| MediaError::Io(e.to_string()))?;
        // With no known length every slice would start at 0, so one frame is all there is.
        let (duration, count) = match asset.duration_ms {
            Some(ms) if ms > 0 => (ms, count),
            _ => (0, 1),
        };
        let mut command = tokio::process::Command::new(ffmpeg);
        command.args(["-nostdin", "-v", "error", "-y"]);
        for i in 0..count {
            // The middle of the i-th of `count` equal slices of the video.
            let at_ms = duration * (u64::from(i) * 2 + 1) / (u64::from(count) * 2);
            let at = format!("{}.{:03}", at_ms / 1_000, at_ms % 1_000);
            command.args(["-ss", &at, "-i"]).arg(source);
        }
        let outs: Vec<PathBuf> = (0..count)
            .map(|i| dir.path().join(format!("frame-{i}.jpg")))
            .collect();
        for (i, out) in outs.iter().enumerate() {
            command
                .args(["-map", &format!("{i}:v:0"), "-frames:v", "1", "-vf", SCALE])
                .arg(out);
        }
        let run = command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .status();
        // A timeout drops the child, which kills it.
        let status = tokio::time::timeout(CUT_TIMEOUT, run)
            .await
            .map_err(|_| MediaError::Unsupported("ffmpeg took too long".to_owned()))?
            .map_err(|e| MediaError::Io(e.to_string()))?;
        if !status.success() || outs.iter().any(|out| !out.exists()) {
            return Err(MediaError::Unsupported(
                "ffmpeg could not cut a frame".to_owned(),
            ));
        }
        let mut frames = Vec::with_capacity(outs.len());
        for out in outs {
            frames.push(self.still_at(out).await?);
        }
        Ok(VideoContext {
            frames,
            duration_ms: asset.duration_ms,
        })
    }
}
