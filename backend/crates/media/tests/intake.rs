#![allow(clippy::unwrap_used, clippy::cast_possible_truncation)]

use std::path::Path;
use std::sync::Arc;

use bytes::Bytes;
use chrono::{Duration, Utc};
use futures::StreamExt;
use postit_core::{ByteStream, Clock, SystemClock, TestClock, UserId};
use postit_data::OwnerScope;
use postit_data::media::{MediaRepo, MediaStatus};
use postit_data::testkit::{callers, seed_user};
use postit_media::testkit::{media_service, media_settings};
use postit_media::{MediaService, MediaServiceError};
use sha2::{Digest, Sha256};
use sqlx::PgPool;

fn stream(chunks: Vec<std::io::Result<Bytes>>) -> ByteStream {
    futures::stream::iter(chunks).boxed()
}

fn whole(bytes: &[u8]) -> ByteStream {
    stream(
        bytes
            .chunks(7000)
            .map(|c| Ok(Bytes::copy_from_slice(c)))
            .collect(),
    )
}

/// A noisy PNG, so its size is a few hundred KiB.
fn png(w: u32, h: u32) -> Vec<u8> {
    let mut state = 0x1234_5678_u32;
    let img = image::RgbaImage::from_fn(w, h, |_, _| {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let b = state.to_le_bytes();
        image::Rgba([b[0], b[1], b[2], 255])
    });
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

struct Env {
    pool: PgPool,
    dir: tempfile::TempDir,
    owner: UserId,
    scope: OwnerScope,
    service: MediaService,
}

async fn env_with(pool: PgPool, tweak: impl FnOnce(&mut postit_config::MediaSettings)) -> Env {
    let dir = tempfile::tempdir().unwrap();
    let mut settings = media_settings(dir.path());
    tweak(&mut settings);
    let owner = seed_user(&pool, "active").await;
    let service = media_service(pool.clone(), settings, Arc::new(SystemClock)).unwrap();
    Env {
        scope: OwnerScope::own(owner),
        pool,
        dir,
        owner,
        service,
    }
}

async fn env(pool: PgPool) -> Env {
    env_with(pool, |_| {}).await
}

fn files_under(root: &Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                found.push(path);
            }
        }
    }
    found
}

async fn only_asset(env: &Env) -> postit_data::media::Media {
    let mut c = env.pool.acquire().await.unwrap();
    let page = MediaRepo::list(
        &mut c,
        &env.scope,
        emixdb::dto::Pagination {
            page: 1,
            page_size: 50,
        },
    )
    .await
    .unwrap();
    assert_eq!(page.data.len(), 1);
    page.data.into_iter().next().unwrap()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn single_upload_ready_with_hash_and_dims(pool: PgPool) {
    let env = env(pool).await;
    let bytes = png(64, 32);
    let media = env
        .service
        .upload(&env.scope, "photo.png", bytes.len() as u64, whole(&bytes))
        .await
        .unwrap();
    assert_eq!(media.status, MediaStatus::Ready);
    assert_eq!(media.mime, "image/png");
    assert_eq!(media.sha256.unwrap(), Sha256::digest(&bytes).to_vec());
    assert_eq!((media.width, media.height), (Some(64), Some(32)));
    assert_eq!(u64::try_from(media.bytes).unwrap(), bytes.len() as u64);
    let thumb = media.thumbnail_key.unwrap();
    let thumb_path = env.dir.path().join(&thumb);
    assert!(thumb_path.exists(), "{thumb}");
    assert!(env.dir.path().join(&media.storage_key).exists());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn single_upload_short_body_fails(pool: PgPool) {
    let env = env(pool).await;
    let err = env
        .service
        .upload(&env.scope, "a.png", 100, whole(&[1u8; 50]))
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            MediaServiceError::Invalid {
                field: "content_length"
            }
        ),
        "{err:?}"
    );
    assert_eq!(only_asset(&env).await.status, MediaStatus::Failed);
    assert!(files_under(env.dir.path()).is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn long_body_fails(pool: PgPool) {
    let env = env(pool).await;
    let err = env
        .service
        .upload(&env.scope, "a.png", 100, whole(&[1u8; 150]))
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            MediaServiceError::Invalid {
                field: "content_length"
            }
        ),
        "{err:?}"
    );
    assert_eq!(only_asset(&env).await.status, MediaStatus::Failed);
    assert!(files_under(env.dir.path()).is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn spoofed_mime_refused(pool: PgPool) {
    let env = env(pool).await;
    let bytes = png(16, 16);
    let err = env
        .service
        .upload(&env.scope, "clip.mp4", bytes.len() as u64, whole(&bytes))
        .await
        .unwrap_err();
    assert!(
        matches!(err, MediaServiceError::UnsupportedMediaType),
        "{err:?}"
    );
    assert_eq!(only_asset(&env).await.status, MediaStatus::Failed);
    assert!(files_under(env.dir.path()).is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn svg_and_html_refused(pool: PgPool) {
    let env = env(pool).await;
    for body in [
        &b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>"[..],
        &b"<!doctype html><script>alert(1)</script>"[..],
    ] {
        let err = env
            .service
            .upload(&env.scope, "x.png", body.len() as u64, whole(body))
            .await
            .unwrap_err();
        assert!(
            matches!(err, MediaServiceError::UnsupportedMediaType),
            "{err:?}"
        );
    }
    assert!(files_under(env.dir.path()).is_empty());
    // An SVG name is refused before any row exists.
    let err = env
        .service
        .upload(&env.scope, "x.svg", 4, whole(b"<svg"))
        .await
        .unwrap_err();
    assert!(matches!(err, MediaServiceError::UnsupportedMediaType));
}

fn failing_after(bytes: &[u8], good: usize) -> ByteStream {
    let mut chunks: Vec<std::io::Result<Bytes>> = bytes[..good]
        .chunks(5000)
        .map(|c| Ok(Bytes::copy_from_slice(c)))
        .collect();
    chunks.push(Err(std::io::Error::new(
        std::io::ErrorKind::ConnectionReset,
        "reset",
    )));
    stream(chunks)
}

#[sqlx::test(migrations = "../data/migrations")]
async fn session_resume_after_drop_same_hash(pool: PgPool) {
    let env = env(pool).await;
    let bytes = png(256, 256);
    let total = bytes.len() as u64;
    let session = env
        .service
        .start_session(&env.scope, "big.png", total)
        .await
        .unwrap();
    let id = session.id;
    let forty = (total * 40 / 100) as usize;
    let quarter = (total * 25 / 100) as usize;

    // The connection drops after 25%: the stored offset does not move.
    let err = env
        .service
        .append(
            &env.scope,
            id,
            0,
            forty as u64 - 1,
            total,
            failing_after(&bytes, quarter),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, MediaServiceError::BodyUnreadable), "{err:?}");
    assert_eq!(
        env.service.offset(&env.scope, id).await.unwrap(),
        (0, total)
    );

    // Resume from the reported offset with a chunk shorter than what the drop left behind:
    // the file is cut to the committed offset, not left at the longer length.
    let ten = (total / 10) as usize;
    let offset = env
        .service
        .append(
            &env.scope,
            id,
            0,
            ten as u64 - 1,
            total,
            whole(&bytes[..ten]),
        )
        .await
        .unwrap();
    assert_eq!(offset, ten as u64);
    let stored = std::fs::metadata(env.dir.path().join(&session.storage_key)).unwrap();
    assert_eq!(stored.len(), ten as u64);
    let offset = env
        .service
        .append(
            &env.scope,
            id,
            ten as u64,
            forty as u64 - 1,
            total,
            whole(&bytes[ten..forty]),
        )
        .await
        .unwrap();
    assert_eq!(offset, forty as u64);
    let offset = env
        .service
        .append(
            &env.scope,
            id,
            forty as u64,
            total - 1,
            total,
            whole(&bytes[forty..]),
        )
        .await
        .unwrap();
    assert_eq!(offset, total);

    let done = env.service.complete(&env.scope, id).await.unwrap();
    assert_eq!(done.status, MediaStatus::Ready);
    assert_eq!(done.sha256.unwrap(), Sha256::digest(&bytes).to_vec());
    assert_eq!(u64::try_from(done.bytes).unwrap(), total);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn offset_mismatch_reports_offset(pool: PgPool) {
    let env = env(pool).await;
    let session = env
        .service
        .start_session(&env.scope, "a.png", 100)
        .await
        .unwrap();
    let err = env
        .service
        .append(&env.scope, session.id, 10, 19, 100, whole(&[0u8; 10]))
        .await
        .unwrap_err();
    assert!(
        matches!(err, MediaServiceError::UploadOffsetMismatch { offset: 0 }),
        "{err:?}"
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn range_past_total_refused(pool: PgPool) {
    let env = env(pool).await;
    let session = env
        .service
        .start_session(&env.scope, "a.png", 100)
        .await
        .unwrap();
    for (start, end, total) in [(0, 100, 100), (0, 49, 99), (50, 10, 100)] {
        let err = env
            .service
            .append(&env.scope, session.id, start, end, total, whole(&[0u8; 50]))
            .await
            .unwrap_err();
        assert!(
            matches!(
                err,
                MediaServiceError::Invalid {
                    field: "content_range"
                }
            ),
            "{start}-{end}/{total}: {err:?}"
        );
    }
    // A body that does not match its range is refused too.
    let err = env
        .service
        .append(&env.scope, session.id, 0, 49, 100, whole(&[0u8; 20]))
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        MediaServiceError::Invalid {
            field: "content_range"
        }
    ));
    assert_eq!(
        env.service.offset(&env.scope, session.id).await.unwrap().0,
        0
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn complete_before_last_byte_refused(pool: PgPool) {
    let env = env(pool).await;
    let session = env
        .service
        .start_session(&env.scope, "a.png", 100)
        .await
        .unwrap();
    env.service
        .append(&env.scope, session.id, 0, 59, 100, whole(&[0u8; 60]))
        .await
        .unwrap();
    let err = env
        .service
        .complete(&env.scope, session.id)
        .await
        .unwrap_err();
    assert!(
        matches!(err, MediaServiceError::UploadOffsetMismatch { offset: 60 }),
        "{err:?}"
    );
    // The session is still open.
    assert_eq!(
        env.service.offset(&env.scope, session.id).await.unwrap().0,
        60
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn expired_session_not_found(pool: PgPool) {
    let env = env(pool.clone()).await;
    let session = env
        .service
        .start_session(&env.scope, "a.png", 100)
        .await
        .unwrap();
    let later: Arc<dyn Clock> = Arc::new(TestClock::new(Utc::now() + Duration::hours(25)));
    let service = media_service(pool, media_settings(env.dir.path()), later).unwrap();
    let err = service
        .append(&env.scope, session.id, 0, 9, 100, whole(&[0u8; 10]))
        .await
        .unwrap_err();
    assert!(matches!(err, MediaServiceError::NotFound), "{err:?}");
    assert!(matches!(
        service.offset(&env.scope, session.id).await,
        Err(MediaServiceError::NotFound)
    ));
    assert!(matches!(
        service.complete(&env.scope, session.id).await,
        Err(MediaServiceError::NotFound)
    ));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn quota_enforced_up_front(pool: PgPool) {
    let env = env(pool.clone()).await;
    sqlx::query(
        "INSERT INTO user_limits (user_id, email_daily_cap, contacts_cap, storage_quota_bytes)
         VALUES ($1, 1, 1, 50)",
    )
    .bind(env.owner.as_uuid())
    .execute(&pool)
    .await
    .unwrap();
    let err = env
        .service
        .start_session(&env.scope, "a.png", 51)
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            MediaServiceError::StorageQuotaExceeded { quota: 50, used: 0 }
        ),
        "{err:?}"
    );
    let err = env
        .service
        .upload(&env.scope, "a.png", 51, whole(&[0u8; 51]))
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        MediaServiceError::StorageQuotaExceeded { .. }
    ));
    assert!(files_under(env.dir.path()).is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn kind_limit_up_front(pool: PgPool) {
    let env = env_with(pool, |s| {
        s.video_max_bytes = 10;
        s.image_max_bytes = 20;
    })
    .await;
    let err = env
        .service
        .start_session(&env.scope, "a.mp4", 11)
        .await
        .unwrap_err();
    assert!(
        matches!(err, MediaServiceError::PayloadTooLarge { max: 10 }),
        "{err:?}"
    );
    let err = env
        .service
        .upload(&env.scope, "a.png", 21, whole(&[0u8; 21]))
        .await
        .unwrap_err();
    assert!(
        matches!(err, MediaServiceError::PayloadTooLarge { max: 20 }),
        "{err:?}"
    );
    // Nothing was reserved.
    let mut c = env.pool.acquire().await.unwrap();
    assert_eq!(MediaRepo::stored_bytes(&mut c, env.owner).await.unwrap(), 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn hostile_filenames(pool: PgPool) {
    let env = env(pool).await;
    let bytes = png(8, 8);
    let ok_names = ["../../etc/passwd.png", "a/b\\c.png", "صورة جميلة.png"];
    for name in ok_names {
        let media = env
            .service
            .upload(&env.scope, name, bytes.len() as u64, whole(&bytes))
            .await
            .unwrap();
        assert_eq!(media.original_filename.as_deref(), Some(name));
        assert!(!media.storage_key.contains(".."), "{}", media.storage_key);
        assert!(!media.storage_key.contains("passwd"));
        assert!(env.dir.path().join(&media.storage_key).exists());
    }
    // Every file landed under the root, under a generated name.
    for file in files_under(env.dir.path()) {
        assert!(file.starts_with(env.dir.path()));
    }
    let long = format!("{}.png", "a".repeat(300));
    for bad in [long.as_str(), "nul\0.png", "   ", ""] {
        let err = env
            .service
            .upload(&env.scope, bad, bytes.len() as u64, whole(&bytes))
            .await
            .unwrap_err();
        assert!(
            matches!(err, MediaServiceError::Invalid { field: "filename" }),
            "{bad:?}: {err:?}"
        );
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn zero_size_refused(pool: PgPool) {
    let env = env(pool).await;
    let err = env
        .service
        .start_session(&env.scope, "a.png", 0)
        .await
        .unwrap_err();
    assert!(
        matches!(err, MediaServiceError::Invalid { field: "size" }),
        "{err:?}"
    );
    let err = env
        .service
        .upload(&env.scope, "a.png", 0, whole(b""))
        .await
        .unwrap_err();
    assert!(
        matches!(err, MediaServiceError::Invalid { field: "size" }),
        "{err:?}"
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn viewers_cannot_upload(pool: PgPool) {
    let env = env(pool.clone()).await;
    let who = callers(&pool, env.owner, &[]).await;
    let err = env
        .service
        .start_session(&who.view, "a.png", 10)
        .await
        .unwrap_err();
    assert!(matches!(err, MediaServiceError::Scope(_)), "{err:?}");
    let session = env
        .service
        .start_session(&env.scope, "a.png", 10)
        .await
        .unwrap();
    let err = env
        .service
        .append(&who.view, session.id, 0, 9, 10, whole(&[0u8; 10]))
        .await
        .unwrap_err();
    assert!(matches!(err, MediaServiceError::Scope(_)), "{err:?}");
    let err = env
        .service
        .complete(&who.view, session.id)
        .await
        .unwrap_err();
    assert!(matches!(err, MediaServiceError::Scope(_)), "{err:?}");
}

/// A body the test feeds by hand, so a request can be held mid-stream.
fn fed() -> (
    futures::channel::mpsc::UnboundedSender<std::io::Result<Bytes>>,
    ByteStream,
) {
    let (tx, rx) = futures::channel::mpsc::unbounded();
    (tx, rx.boxed())
}

fn parts_under(env: &Env) -> Vec<std::path::PathBuf> {
    let dir = env.dir.path().join(postit_media::PARTS_DIR);
    if dir.exists() {
        files_under(&dir)
    } else {
        Vec::new()
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn append_holds_no_row_lock_while_the_body_streams(pool: PgPool) {
    let env = env(pool).await;
    let bytes = png(64, 64);
    let total = bytes.len() as u64;
    let session = env
        .service
        .start_session(&env.scope, "a.png", total)
        .await
        .unwrap();
    let (feed, body) = fed();
    let service = env.service.clone();
    let scope = env.scope.clone();
    let append = tokio::spawn(async move {
        service
            .append(&scope, session.id, 0, total - 1, total, body)
            .await
    });
    let half = bytes.len() / 2;
    feed.unbounded_send(Ok(Bytes::copy_from_slice(&bytes[..half])))
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Mid-stream the upload row is free: a slow client cannot pin a connection or a lock.
    let mut tx = env.pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM media_assets WHERE id = $1 FOR UPDATE NOWAIT")
        .bind(session.id.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.rollback().await.unwrap();

    feed.unbounded_send(Ok(Bytes::copy_from_slice(&bytes[half..])))
        .unwrap();
    drop(feed);
    assert_eq!(append.await.unwrap().unwrap(), total);
    assert!(parts_under(&env).is_empty());
    let done = env.service.complete(&env.scope, session.id).await.unwrap();
    assert_eq!(done.sha256.unwrap(), Sha256::digest(&bytes).to_vec());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn concurrent_appends_of_one_range_commit_once(pool: PgPool) {
    let env = env(pool).await;
    let session = env
        .service
        .start_session(&env.scope, "a.png", 20)
        .await
        .unwrap();
    let (slow_feed, slow_body) = fed();
    let service = env.service.clone();
    let scope = env.scope.clone();
    let slow = tokio::spawn(async move {
        service
            .append(&scope, session.id, 0, 9, 20, slow_body)
            .await
    });
    slow_feed
        .unbounded_send(Ok(Bytes::from_static(b"AAAA")))
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // A retry of the same range wins while the first request still streams.
    let offset = env
        .service
        .append(&env.scope, session.id, 0, 9, 20, whole(b"BBBBBBBBBB"))
        .await
        .unwrap();
    assert_eq!(offset, 10);

    slow_feed
        .unbounded_send(Ok(Bytes::from_static(b"AAAAAA")))
        .unwrap();
    drop(slow_feed);
    let err = slow.await.unwrap().unwrap_err();
    assert!(
        matches!(err, MediaServiceError::UploadOffsetMismatch { offset: 10 }),
        "{err:?}"
    );
    let stored = std::fs::read(env.dir.path().join(&session.storage_key)).unwrap();
    assert_eq!(stored, b"BBBBBBBBBB");
    assert!(parts_under(&env).is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn complete_keeps_the_upload_open_after_a_failure_that_is_not_the_file(pool: PgPool) {
    let env = env(pool).await;
    let bytes = png(32, 32);
    let total = bytes.len() as u64;
    let session = env
        .service
        .start_session(&env.scope, "a.png", total)
        .await
        .unwrap();
    env.service
        .append(&env.scope, session.id, 0, total - 1, total, whole(&bytes))
        .await
        .unwrap();
    // The stored file cannot be read: a disk problem, not a refusal of its content.
    let path = env.dir.path().join(&session.storage_key);
    std::fs::remove_file(&path).unwrap();
    let err = env
        .service
        .complete(&env.scope, session.id)
        .await
        .unwrap_err();
    assert!(matches!(err, MediaServiceError::Io(_)), "{err:?}");
    assert_eq!(only_asset(&env).await.status, MediaStatus::Uploading);

    // Once the bytes are back, asking again finishes the upload.
    std::fs::write(&path, &bytes).unwrap();
    let done = env.service.complete(&env.scope, session.id).await.unwrap();
    assert_eq!(done.status, MediaStatus::Ready);
}

/// Waits `delay` before probing, or forever when `delay` is `None`.
struct SlowProbe(Option<std::time::Duration>);

#[async_trait::async_trait]
impl postit_core::MediaProbe for SlowProbe {
    async fn probe(
        &self,
        path: &Path,
        mime: &str,
    ) -> Result<postit_core::ProbeInfo, postit_core::MediaError> {
        match self.0 {
            Some(delay) => tokio::time::sleep(delay).await,
            None => futures::future::pending::<()>().await,
        }
        postit_core::MediaProbe::probe(&postit_media::NativeProbe, path, mime).await
    }
}

async fn env_with_probe(
    pool: PgPool,
    probe: SlowProbe,
    tweak: impl FnOnce(&mut postit_config::MediaSettings),
) -> Env {
    let dir = tempfile::tempdir().unwrap();
    let mut settings = media_settings(dir.path());
    tweak(&mut settings);
    let owner = seed_user(&pool, "active").await;
    let service = postit_media::testkit::media_service_with_probe(
        pool.clone(),
        settings,
        Arc::new(SystemClock),
        Arc::new(probe),
    )
    .unwrap();
    Env {
        scope: OwnerScope::own(owner),
        pool,
        dir,
        owner,
        service,
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn complete_finishes_when_its_request_is_dropped(pool: PgPool) {
    let env = env_with_probe(
        pool,
        SlowProbe(Some(std::time::Duration::from_millis(300))),
        |_| {},
    )
    .await;
    let bytes = png(32, 32);
    let total = bytes.len() as u64;
    let session = env
        .service
        .start_session(&env.scope, "a.png", total)
        .await
        .unwrap();
    env.service
        .append(&env.scope, session.id, 0, total - 1, total, whole(&bytes))
        .await
        .unwrap();
    // The request times out while the file is still being checked.
    let dropped = tokio::time::timeout(
        std::time::Duration::from_millis(50),
        env.service.complete(&env.scope, session.id),
    )
    .await;
    assert!(dropped.is_err());
    let mut status = MediaStatus::Processing;
    for _ in 0..50 {
        status = only_asset(&env).await.status;
        if status != MediaStatus::Processing {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(status, MediaStatus::Ready);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_probe_that_never_ends_refuses_the_file(pool: PgPool) {
    let env = env_with_probe(pool, SlowProbe(None), |s| {
        s.complete_timeout = std::time::Duration::from_millis(200);
    })
    .await;
    let bytes = png(32, 32);
    let err = env
        .service
        .upload(&env.scope, "a.png", bytes.len() as u64, whole(&bytes))
        .await
        .unwrap_err();
    assert!(
        matches!(err, MediaServiceError::UnsupportedMediaType),
        "{err:?}"
    );
    assert_eq!(only_asset(&env).await.status, MediaStatus::Failed);
    assert!(files_under(env.dir.path()).is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn an_idle_body_reads_as_timed_out(pool: PgPool) {
    let env = env(pool).await;
    let session = env
        .service
        .start_session(&env.scope, "a.png", 10)
        .await
        .unwrap();
    let body = stream(vec![Err(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        "idle",
    ))]);
    let err = env
        .service
        .append(&env.scope, session.id, 0, 9, 10, body)
        .await
        .unwrap_err();
    assert!(matches!(err, MediaServiceError::BodyTimedOut), "{err:?}");
}
