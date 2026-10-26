#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};

use image::{DynamicImage, Frame, RgbImage, Rgba, RgbaImage};
use postit_core::{MediaError, MediaKind, MediaProbe, ProbeInfo};
use postit_media::{NativeProbe, Thumbnailer};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn write_image(dir: &Path, name: &str, w: u32, h: u32) -> PathBuf {
    let path = dir.join(name);
    if Path::new(name).extension().is_some_and(|e| e == "jpg") {
        DynamicImage::ImageRgb8(RgbImage::new(w, h))
            .save(&path)
            .unwrap();
    } else {
        DynamicImage::ImageRgba8(RgbaImage::new(w, h))
            .save(&path)
            .unwrap();
    }
    path
}

#[tokio::test]
async fn native_probe_reads_images() {
    let dir = tempfile::tempdir().unwrap();
    for (name, mime) in [
        ("a.png", "image/png"),
        ("a.jpg", "image/jpeg"),
        ("a.gif", "image/gif"),
        ("a.webp", "image/webp"),
    ] {
        let path = write_image(dir.path(), name, 64, 32);
        let info = NativeProbe.probe(&path, mime).await.unwrap();
        assert_eq!(info.width, Some(64), "{name}");
        assert_eq!(info.height, Some(32), "{name}");
        assert_eq!(info.duration_ms, None, "{name}");
    }
}

#[tokio::test]
async fn native_probe_reads_mp4_and_mov() {
    let mp4 = NativeProbe
        .probe(&fixture("landscape.mp4"), "video/mp4")
        .await
        .unwrap();
    assert_eq!((mp4.width, mp4.height), (Some(320), Some(240)));
    let ms = mp4.duration_ms.unwrap();
    assert!((900..=1100).contains(&ms), "{ms}");

    let mov = NativeProbe
        .probe(&fixture("portrait.mov"), "video/quicktime")
        .await
        .unwrap();
    assert_eq!((mov.width, mov.height), (Some(240), Some(320)));
    let ms = mov.duration_ms.unwrap();
    assert!((900..=1100).contains(&ms), "{ms}");
}

#[tokio::test]
async fn native_probe_refuses_garbage() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("junk.bin");
    std::fs::write(&path, b"this is not media at all").unwrap();
    for mime in ["image/png", "video/mp4"] {
        assert!(matches!(
            NativeProbe.probe(&path, mime).await,
            Err(MediaError::Unsupported(_))
        ));
    }
}

fn thumbnailer(max_pixels: u64) -> Thumbnailer {
    Thumbnailer::new(max_pixels, PathBuf::from("ffmpeg"))
}

fn info(w: u32, h: u32) -> ProbeInfo {
    ProbeInfo {
        width: Some(w),
        height: Some(h),
        duration_ms: None,
    }
}

#[tokio::test]
async fn thumbnail_fits_480() {
    let dir = tempfile::tempdir().unwrap();
    let src = write_image(dir.path(), "big.png", 1920, 1080);
    let dest = dir.path().join("big.thumb.jpg");
    let made = thumbnailer(40_000_000)
        .make(&src, MediaKind::Image, &info(1920, 1080), &dest)
        .await;
    assert!(made);
    let (w, h) = image::image_dimensions(&dest).unwrap();
    assert_eq!((w, h), (480, 270));
    assert_eq!(
        image::ImageReader::open(&dest)
            .unwrap()
            .with_guessed_format()
            .unwrap()
            .format(),
        Some(image::ImageFormat::Jpeg)
    );
}

#[tokio::test]
async fn thumbnail_skips_over_pixel_limit() {
    let dir = tempfile::tempdir().unwrap();
    let src = write_image(dir.path(), "small.png", 64, 32);
    let dest = dir.path().join("small.thumb.jpg");
    let made = thumbnailer(1000)
        .make(&src, MediaKind::Image, &info(64, 32), &dest)
        .await;
    assert!(!made);
    assert!(!dest.exists());
}

#[tokio::test]
async fn gif_thumbnail_uses_first_frame() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("two.gif");
    let red = RgbaImage::from_pixel(64, 64, Rgba([255, 0, 0, 255]));
    let blue = RgbaImage::from_pixel(64, 64, Rgba([0, 0, 255, 255]));
    {
        let file = std::fs::File::create(&src).unwrap();
        let mut enc = image::codecs::gif::GifEncoder::new(file);
        enc.encode_frames([Frame::new(red), Frame::new(blue)])
            .unwrap();
    }
    let dest = dir.path().join("two.thumb.jpg");
    assert!(
        thumbnailer(40_000_000)
            .make(&src, MediaKind::Image, &info(64, 64), &dest)
            .await
    );
    let px = image::open(&dest).unwrap().to_rgb8();
    let p = px.get_pixel(10, 10);
    assert!(p[0] > 200 && p[2] < 80, "{p:?}");
}

#[cfg(not(feature = "ffprobe"))]
#[tokio::test]
async fn video_thumbnail_needs_the_ffprobe_feature() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("v.thumb.jpg");
    assert!(
        !thumbnailer(40_000_000)
            .make(
                &fixture("landscape.mp4"),
                MediaKind::Video,
                &info(320, 240),
                &dest
            )
            .await
    );
    assert!(!dest.exists());
}

#[cfg(feature = "ffprobe")]
#[tokio::test]
async fn ffprobe_reads_fixtures() {
    use postit_media::FfprobeProbe;
    let probe = FfprobeProbe::new(PathBuf::from("ffprobe"));
    let mp4 = probe
        .probe(&fixture("landscape.mp4"), "video/mp4")
        .await
        .unwrap();
    assert_eq!((mp4.width, mp4.height), (Some(320), Some(240)));
    assert!((900..=1100).contains(&mp4.duration_ms.unwrap()));
    let mov = probe
        .probe(&fixture("portrait.mov"), "video/quicktime")
        .await
        .unwrap();
    assert_eq!((mov.width, mov.height), (Some(240), Some(320)));
    let dir = tempfile::tempdir().unwrap();
    let png = write_image(dir.path(), "i.png", 8, 4);
    let img = probe.probe(&png, "image/png").await.unwrap();
    assert_eq!((img.width, img.height), (Some(8), Some(4)));
    let junk = dir.path().join("junk.mp4");
    std::fs::write(&junk, b"nope").unwrap();
    assert!(matches!(
        probe.probe(&junk, "video/mp4").await,
        Err(MediaError::Unsupported(_))
    ));
}

#[cfg(feature = "ffprobe")]
#[tokio::test]
async fn video_thumbnail_written() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("v.thumb.jpg");
    assert!(
        thumbnailer(40_000_000)
            .make(
                &fixture("landscape.mp4"),
                MediaKind::Video,
                &info(320, 240),
                &dest
            )
            .await
    );
    let (w, h) = image::image_dimensions(&dest).unwrap();
    assert_eq!((w, h), (320, 240));
}
