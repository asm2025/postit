#![allow(clippy::unwrap_used)]

use std::io::Cursor;
use std::sync::Arc;

use chrono::{Duration, Utc};
use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
use postit_core::{
    CONTEXT_MAX_SIDE, FrameExtractor, MediaAsset, MediaAssetId, MediaAssetState, MediaError,
    MediaKind, UserId,
};
use postit_data::media::{MediaRepo, MediaStatus, NewMedia};
use postit_data::testkit::{callers, seed_user};
use postit_media::{FsMediaStore, MediaFrames, StorageKey};
use sqlx::PgPool;
use uuid::Uuid;

struct Fixture {
    _dir: tempfile::TempDir,
    store: Arc<FsMediaStore>,
}

fn fixture(pool: &PgPool) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(FsMediaStore::open_root(dir.path(), pool.clone()).unwrap());
    Fixture { _dir: dir, store }
}

/// Stores `bytes` as a ready asset owned by `owner`.
async fn put(
    pool: &PgPool,
    fx: &Fixture,
    owner: UserId,
    mime: &str,
    kind: MediaKind,
    bytes: &[u8],
) -> MediaAsset {
    let who = callers(pool, owner, &[]).await;
    let id = MediaAssetId::from_uuid(Uuid::now_v7());
    let key = StorageKey::new(owner, id, Utc::now());
    let path = fx.store.resolve(&key).unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, bytes).unwrap();
    let mut c = pool.acquire().await.unwrap();
    MediaRepo::insert(
        &mut c,
        &who.owner,
        NewMedia {
            id,
            storage_key: key.as_str().to_string(),
            original_filename: None,
            mime: mime.into(),
            bytes: i64::try_from(bytes.len()).unwrap(),
            status: MediaStatus::Ready,
            declared_bytes: None,
            upload_expires_at: Some(Utc::now() + Duration::hours(1)),
            derived_from: None,
            fit_spec: None,
        },
    )
    .await
    .unwrap();
    MediaAsset {
        id,
        owner_id: owner,
        kind,
        mime: mime.into(),
        bytes: bytes.len() as u64,
        width: None,
        height: None,
        duration_ms: Some(1_000),
        sha256: None,
        state: MediaAssetState::Ready,
        derived_from: None,
    }
}

fn png(w: u32, h: u32) -> Vec<u8> {
    let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(w, h, Rgb([200, 30, 30])));
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, ImageFormat::Png).unwrap();
    out.into_inner()
}

fn decode(jpeg: &[u8]) -> DynamicImage {
    image::load_from_memory_with_format(jpeg, ImageFormat::Jpeg).unwrap()
}

const BIG: u64 = 100_000_000;

#[sqlx::test(migrations = "../data/migrations")]
async fn still_downscales_to_1568_long_side(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let fx = fixture(&pool);
    let asset = put(
        &pool,
        &fx,
        owner,
        "image/png",
        MediaKind::Image,
        &png(4000, 1000),
    )
    .await;
    let frames = MediaFrames::new(fx.store.clone(), BIG, None);
    let still = frames.still(&asset).await.unwrap();
    assert_eq!((still.width, still.height), (1568, 392));
    let img = decode(&still.jpeg);
    assert_eq!((img.width(), img.height()), (1568, 392));
    assert_eq!(CONTEXT_MAX_SIDE, 1568);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn still_keeps_small_image_size(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let fx = fixture(&pool);
    let asset = put(
        &pool,
        &fx,
        owner,
        "image/png",
        MediaKind::Image,
        &png(800, 600),
    )
    .await;
    let frames = MediaFrames::new(fx.store.clone(), BIG, None);
    let still = frames.still(&asset).await.unwrap();
    assert_eq!((still.width, still.height), (800, 600));
    assert_eq!(decode(&still.jpeg).width(), 800);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn still_of_gif_is_first_frame(pool: PgPool) {
    use image::codecs::gif::GifEncoder;
    use image::{Frame, RgbaImage};
    let owner = seed_user(&pool, "active").await;
    let fx = fixture(&pool);
    let mut buf = Vec::new();
    {
        let mut enc = GifEncoder::new(&mut buf);
        let red = RgbaImage::from_pixel(32, 24, image::Rgba([255, 0, 0, 255]));
        let blue = RgbaImage::from_pixel(32, 24, image::Rgba([0, 0, 255, 255]));
        enc.encode_frame(Frame::new(red)).unwrap();
        enc.encode_frame(Frame::new(blue)).unwrap();
    }
    let asset = put(&pool, &fx, owner, "image/gif", MediaKind::Image, &buf).await;
    let frames = MediaFrames::new(fx.store.clone(), BIG, None);
    let still = frames.still(&asset).await.unwrap();
    assert_eq!((still.width, still.height), (32, 24));
    let px = decode(&still.jpeg).to_rgb8().get_pixel(16, 12).0;
    assert!(px[0] > 200 && px[2] < 60, "first frame is red, got {px:?}");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn still_of_other_owner_is_not_found(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let other = seed_user(&pool, "active").await;
    let fx = fixture(&pool);
    let mut asset = put(&pool, &fx, owner, "image/png", MediaKind::Image, &png(8, 8)).await;
    asset.owner_id = other;
    let frames = MediaFrames::new(fx.store.clone(), BIG, None);
    assert_eq!(
        frames.still(&asset).await.unwrap_err(),
        MediaError::NotFound
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn still_over_max_pixels_is_unsupported(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let fx = fixture(&pool);
    let asset = put(
        &pool,
        &fx,
        owner,
        "image/png",
        MediaKind::Image,
        &png(100, 100),
    )
    .await;
    let frames = MediaFrames::new(fx.store.clone(), 5_000, None);
    assert!(matches!(
        frames.still(&asset).await,
        Err(MediaError::Unsupported(_))
    ));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn frames_without_ffmpeg_give_the_length_only(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let fx = fixture(&pool);
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/landscape.mp4"
    ))
    .unwrap();
    let asset = put(&pool, &fx, owner, "video/mp4", MediaKind::Video, &bytes).await;
    let frames = MediaFrames::new(fx.store.clone(), BIG, None);
    let cut = frames.frames(&asset, 4).await.unwrap();
    assert!(cut.frames.is_empty());
    assert_eq!(cut.duration_ms, asset.duration_ms);
}

#[cfg(feature = "ffprobe")]
#[sqlx::test(migrations = "../data/migrations")]
#[ignore = "needs ffmpeg on PATH"]
async fn frames_spread_over_video(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let fx = fixture(&pool);
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/landscape.mp4"
    ))
    .unwrap();
    let asset = put(&pool, &fx, owner, "video/mp4", MediaKind::Video, &bytes).await;
    let frames = MediaFrames::new(fx.store.clone(), BIG, Some("ffmpeg".into()));
    let video = frames.frames(&asset, 4).await.unwrap();
    assert_eq!(video.frames.len(), 4);
    assert_eq!(video.duration_ms, Some(1_000));
    for f in &video.frames {
        assert!(f.width.max(f.height) <= CONTEXT_MAX_SIDE);
        assert_eq!((f.width, f.height), (320, 240));
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn still_refuses_a_video_and_frames_refuse_an_image(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let fx = fixture(&pool);
    let image = put(&pool, &fx, owner, "image/png", MediaKind::Image, &png(8, 8)).await;
    let mut video = image.clone();
    video.kind = MediaKind::Video;
    let frames = MediaFrames::new(fx.store.clone(), BIG, Some("ffmpeg".into()));
    assert!(matches!(
        frames.still(&video).await,
        Err(MediaError::Unsupported(_))
    ));
    assert!(matches!(
        frames.frames(&image, 4).await,
        Err(MediaError::Unsupported(_))
    ));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn zero_frames_cuts_nothing(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let fx = fixture(&pool);
    let asset = put(
        &pool,
        &fx,
        owner,
        "video/mp4",
        MediaKind::Video,
        b"not a movie",
    )
    .await;
    // No ffmpeg is needed when nothing is cut.
    let frames = MediaFrames::new(fx.store.clone(), BIG, None);
    let video = frames.frames(&asset, 0).await.unwrap();
    assert!(video.frames.is_empty());
    assert_eq!(video.duration_ms, Some(1_000));
}

#[cfg(feature = "ffprobe")]
#[sqlx::test(migrations = "../data/migrations")]
#[ignore = "needs ffmpeg on PATH"]
async fn frames_are_capped_and_unknown_duration_gives_one(pool: PgPool) {
    use postit_core::CONTEXT_MAX_FRAMES;
    let owner = seed_user(&pool, "active").await;
    let fx = fixture(&pool);
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/landscape.mp4"
    ))
    .unwrap();
    let mut asset = put(&pool, &fx, owner, "video/mp4", MediaKind::Video, &bytes).await;
    let frames = MediaFrames::new(fx.store.clone(), BIG, Some("ffmpeg".into()));
    let many = frames.frames(&asset, 1_000).await.unwrap();
    assert_eq!(many.frames.len(), CONTEXT_MAX_FRAMES as usize);

    for duration in [None, Some(0)] {
        asset.duration_ms = duration;
        let video = frames.frames(&asset, 4).await.unwrap();
        assert_eq!(video.frames.len(), 1, "{duration:?}");
        assert_eq!(video.duration_ms, duration);
    }
}
