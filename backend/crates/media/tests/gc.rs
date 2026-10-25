#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::time::{Duration as StdDuration, SystemTime};

use chrono::{Duration, Utc};
use postit_core::{MediaAssetId, UserId};
use postit_data::OwnerScope;
use postit_data::media::{Media, MediaRepo, MediaStatus, NewMedia, ReadyMedia};
use postit_data::testkit::seed_user;
use postit_media::gc::{GcReport, run_media_gc};
use postit_media::{FsMediaStore, StorageKey};
use sqlx::PgPool;
use uuid::Uuid;

const TTL: StdDuration = StdDuration::from_hours(24);

struct Env {
    pool: PgPool,
    dir: tempfile::TempDir,
    store: FsMediaStore,
    owner: UserId,
    scope: OwnerScope,
}

async fn env(pool: PgPool) -> Env {
    let dir = tempfile::tempdir().unwrap();
    let store = FsMediaStore::open_root(dir.path(), pool.clone()).unwrap();
    let owner = seed_user(&pool, "active").await;
    Env {
        scope: OwnerScope::own(owner),
        pool,
        dir,
        store,
        owner,
    }
}

fn age(path: &PathBuf, by: StdDuration) {
    let file = std::fs::File::options().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() - by).unwrap();
}

fn orphan_key(owner: UserId) -> StorageKey {
    StorageKey::new(owner, MediaAssetId::from_uuid(Uuid::now_v7()), Utc::now())
}

fn write_file(env: &Env, key: &StorageKey) -> PathBuf {
    let path = env.store.resolve(key).unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"bytes").unwrap();
    path
}

/// An uploading row with a file at its key.
async fn session(env: &Env) -> (Media, PathBuf) {
    let id = MediaAssetId::from_uuid(Uuid::now_v7());
    let key = StorageKey::new(env.owner, id, Utc::now());
    let mut c = env.pool.acquire().await.unwrap();
    let media = MediaRepo::reserve(
        &mut c,
        &env.scope,
        NewMedia {
            id,
            storage_key: key.as_str().to_string(),
            original_filename: Some("a.png".into()),
            mime: "application/octet-stream".into(),
            bytes: 0,
            status: MediaStatus::Uploading,
            declared_bytes: Some(10),
            upload_expires_at: Some(Utc::now() + Duration::hours(1)),
            derived_from: None,
            fit_spec: None,
        },
    )
    .await
    .unwrap();
    let path = write_file(env, &key);
    (media, path)
}

async fn exists(env: &Env, id: MediaAssetId) -> bool {
    let mut c = env.pool.acquire().await.unwrap();
    MediaRepo::get(&mut c, &env.scope, id).await.is_ok()
}

async fn run(env: &Env) -> GcReport {
    run_media_gc(&env.pool, &env.store, Utc::now(), TTL)
        .await
        .unwrap()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn gc_removes_failed_expired_and_stale_processing_with_files(pool: PgPool) {
    let env = env(pool.clone()).await;
    let (failed, failed_file) = session(&env).await;
    let (expired, expired_file) = session(&env).await;
    let (stale, stale_file) = session(&env).await;
    let (fresh, fresh_file) = session(&env).await;
    let mut c = pool.acquire().await.unwrap();
    MediaRepo::set_failed(&mut c, &env.scope, failed.id)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE media_assets SET upload_expires_at = now() - interval '1 hour' WHERE id = $1",
    )
    .bind(expired.id.as_uuid())
    .execute(&pool)
    .await
    .unwrap();
    MediaRepo::set_processing(&mut c, &env.scope, stale.id)
        .await
        .unwrap();
    sqlx::query("UPDATE media_assets SET updated_at = now() - interval '2 days' WHERE id = $1")
        .bind(stale.id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    // A ready asset with a thumbnail must survive.
    let (ready, ready_file) = session(&env).await;
    MediaRepo::set_ready(
        &mut c,
        &env.scope,
        ready.id,
        ReadyMedia {
            mime: "image/png".into(),
            bytes: 5,
            sha256: vec![1; 32],
            width: None,
            height: None,
            duration_ms: None,
            thumbnail_key: None,
        },
    )
    .await
    .unwrap();
    drop(c);

    let report = run(&env).await;
    assert_eq!(report.rows, 3);
    for (id, file) in [
        (failed.id, failed_file),
        (expired.id, expired_file),
        (stale.id, stale_file),
    ] {
        assert!(!exists(&env, id).await);
        assert!(!file.exists());
    }
    for (id, file) in [(fresh.id, fresh_file), (ready.id, ready_file)] {
        assert!(exists(&env, id).await);
        assert!(file.exists());
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn gc_keeps_streaming_upload(pool: PgPool) {
    let env = env(pool).await;
    let (open, file) = session(&env).await;
    age(&file, StdDuration::from_hours(72));
    let report = run(&env).await;
    assert_eq!(
        report,
        GcReport {
            rows: 0,
            orphans: 0
        }
    );
    assert!(exists(&env, open.id).await);
    assert!(file.exists());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn gc_removes_old_orphan_file(pool: PgPool) {
    let env = env(pool).await;
    let file = write_file(&env, &orphan_key(env.owner));
    age(&file, StdDuration::from_hours(48));
    let report = run(&env).await;
    assert_eq!(report.orphans, 1);
    assert!(!file.exists());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn gc_keeps_young_orphan_file(pool: PgPool) {
    let env = env(pool).await;
    let file = write_file(&env, &orphan_key(env.owner));
    let report = run(&env).await;
    assert_eq!(
        report,
        GcReport {
            rows: 0,
            orphans: 0
        }
    );
    assert!(file.exists());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn gc_ignores_unparsable_names(pool: PgPool) {
    let env = env(pool).await;
    let notes = env.dir.path().join("notes.txt");
    std::fs::write(&notes, b"keep me").unwrap();
    let nested = env.dir.path().join("x").join("y.txt");
    std::fs::create_dir_all(nested.parent().unwrap()).unwrap();
    std::fs::write(&nested, b"keep me too").unwrap();
    age(&notes, StdDuration::from_hours(500));
    age(&nested, StdDuration::from_hours(500));
    let report = run(&env).await;
    assert_eq!(report.orphans, 0);
    assert!(notes.exists() && nested.exists());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn gc_removes_empty_month_dirs(pool: PgPool) {
    let env = env(pool.clone()).await;
    let file = write_file(&env, &orphan_key(env.owner));
    age(&file, StdDuration::from_hours(48));
    let month = file.parent().unwrap().to_path_buf();
    let year = month.parent().unwrap().to_path_buf();
    let owner_dir = year.parent().unwrap().to_path_buf();
    run(&env).await;
    assert!(!file.exists());
    // The directories are young (the delete just touched them), so they stay for now.
    assert!(month.exists());
    // A later run, once they have sat idle past the TTL, removes them but keeps the owner dir.
    run_media_gc(&pool, &env.store, Utc::now() + Duration::hours(48), TTL)
        .await
        .unwrap();
    assert!(!month.exists());
    assert!(!year.exists());
    assert!(owner_dir.exists());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn gc_is_idempotent(pool: PgPool) {
    let env = env(pool).await;
    let (failed, _file) = session(&env).await;
    let mut c = env.pool.acquire().await.unwrap();
    MediaRepo::set_failed(&mut c, &env.scope, failed.id)
        .await
        .unwrap();
    drop(c);
    let orphan = write_file(&env, &orphan_key(env.owner));
    age(&orphan, StdDuration::from_hours(48));
    let first = run(&env).await;
    assert_eq!(
        first,
        GcReport {
            rows: 1,
            orphans: 1
        }
    );
    let second = run(&env).await;
    assert_eq!(
        second,
        GcReport {
            rows: 0,
            orphans: 0
        }
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn register_adds_the_recurring_job(pool: PgPool) {
    use postit_jobs::testkit::jobs_settings;
    let env = env(pool.clone()).await;
    let mut registry = postit_jobs::JobRegistry::default();
    let store = std::sync::Arc::new(env.store.clone());
    postit_media::gc::register(
        &mut registry,
        pool,
        store,
        &jobs_settings().schedules,
        TTL,
        std::sync::Arc::new(postit_core::SystemClock),
    )
    .unwrap();
    let mut catalog = postit_jobs::ConsoleCatalog::default();
    postit_media::gc::console(&mut catalog, &jobs_settings().schedules);
    assert!(
        registry
            .registered_types()
            .snapshot()
            .contains(&"media_gc".to_string())
    );
    assert!(catalog.get("media_gc").is_some());
}
