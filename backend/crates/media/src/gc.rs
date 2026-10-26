//! The `media_gc` job: collects rows nobody will finish and files no row names.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use postit_config::JobSchedules;
use postit_core::Clock;
use postit_data::media::MediaRepo;
use postit_jobs::{
    ConsoleCatalog, ConsoleSpec, Job, JobContext, JobError, JobRegistry, JobsError, Queue,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::storage::{FsMediaStore, PARTS_DIR, StorageKey};

const BATCH: i64 = 500;

/// The daily job that collects failed uploads, expired sessions, and orphan files.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct MediaGc {}

impl Job for MediaGc {
    const JOB_TYPE: &'static str = "media_gc";
    const QUEUE: Queue = Queue::Maintenance;
}

/// What one run collected.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GcReport {
    /// Rows deleted: failed, expired, or stuck in processing.
    pub rows: u64,
    /// Files removed because no row named them, and stale upload chunks.
    pub orphans: u64,
}

fn retry(err: impl std::fmt::Display) -> JobError {
    JobError::Retry(err.to_string())
}

/// Removes the file at `key`, logging (by key only) a failure other than "already gone".
async fn remove_key(store: &FsMediaStore, key: &str) -> bool {
    let Some(parsed) = StorageKey::parse(key) else {
        return false;
    };
    let Ok(path) = store.resolve(&parsed) else {
        return false;
    };
    match tokio::fs::remove_file(&path).await {
        Ok(()) => true,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => false,
        Err(err) => {
            tracing::warn!(key, error = %err, "could not remove a collected media file");
            false
        }
    }
}

/// Deletes the rows nobody will finish, in batches, and their files.
async fn collect_rows(
    pool: &PgPool,
    store: &FsMediaStore,
    now: DateTime<Utc>,
    stale_before: DateTime<Utc>,
) -> Result<u64, JobError> {
    let mut total = 0;
    loop {
        let mut conn = pool.acquire().await.map_err(retry)?;
        let rows = MediaRepo::take_garbage(&mut conn, now, stale_before, BATCH)
            .await
            .map_err(retry)?;
        drop(conn);
        if rows.is_empty() {
            return Ok(total);
        }
        total += rows.len() as u64;
        for row in &rows {
            remove_key(store, &row.storage_key).await;
            if let Some(thumb) = &row.thumbnail_key {
                remove_key(store, thumb).await;
            }
        }
    }
}

fn relative_key(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let parts: Option<Vec<&str>> = rel.components().map(|c| c.as_os_str().to_str()).collect();
    Some(parts?.join("/"))
}

fn modified(meta: &std::fs::Metadata) -> Option<DateTime<Utc>> {
    meta.modified().ok().map(DateTime::<Utc>::from)
}

/// Removes every parseable file older than `cutoff` that no row names, and the empty month and
/// year directories that have sat idle past it.
async fn sweep_orphans(
    pool: &PgPool,
    store: &FsMediaStore,
    cutoff: DateTime<Utc>,
) -> Result<u64, JobError> {
    let root = store.root().to_path_buf();
    let mut queue = VecDeque::from([(root.clone(), 0usize)]);
    let mut dirs: Vec<(PathBuf, usize)> = Vec::new();
    let mut candidates: Vec<(String, PathBuf)> = Vec::new();
    while let Some((dir, depth)) = queue.pop_front() {
        let mut entries = tokio::fs::read_dir(&dir).await.map_err(retry)?;
        while let Some(entry) = entries.next_entry().await.map_err(retry)? {
            let path = entry.path();
            let kind = entry.file_type().await.map_err(retry)?;
            if kind.is_dir() {
                if depth == 0 && entry.file_name() == PARTS_DIR {
                    continue;
                }
                queue.push_back((path.clone(), depth + 1));
                dirs.push((path, depth + 1));
            } else if kind.is_file() {
                let Some(key) = relative_key(&root, &path) else {
                    continue;
                };
                if StorageKey::parse(&key).is_none() {
                    tracing::info!(key, "left a file in the media root that is not a media key");
                    continue;
                }
                let Ok(meta) = entry.metadata().await else {
                    continue;
                };
                if modified(&meta).is_some_and(|m| m < cutoff) {
                    candidates.push((key, path));
                }
            }
        }
    }

    let mut removed = 0;
    for batch in candidates.chunks(usize::try_from(BATCH).unwrap_or(500)) {
        let keys: Vec<String> = batch.iter().map(|(k, _)| k.clone()).collect();
        let mut conn = pool.acquire().await.map_err(retry)?;
        let known = MediaRepo::known_keys(&mut conn, &keys)
            .await
            .map_err(retry)?;
        drop(conn);
        for (key, path) in batch {
            if known.contains(key) {
                continue;
            }
            match tokio::fs::remove_file(path).await {
                Ok(()) => removed += 1,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => tracing::warn!(key, error = %err, "could not remove an orphan file"),
            }
        }
    }

    // Deepest first; only year (depth 2) and month (depth 3) directories, and only ones that
    // have been idle past the cutoff, so a directory a running upload just made stays.
    dirs.sort_by_key(|(_, depth)| std::cmp::Reverse(*depth));
    for (dir, depth) in dirs {
        if !(2..=3).contains(&depth) {
            continue;
        }
        let idle = tokio::fs::metadata(&dir)
            .await
            .ok()
            .and_then(|m| modified(&m))
            .is_some_and(|m| m < cutoff);
        if idle {
            // Fails on a directory that still holds files, which is the point.
            let _ = tokio::fs::remove_dir(&dir).await;
        }
    }
    Ok(removed)
}

/// Removes upload chunks older than `cutoff`: a request that wrote them died before it could.
async fn sweep_parts(store: &FsMediaStore, cutoff: DateTime<Utc>) -> Result<u64, JobError> {
    let mut entries = match tokio::fs::read_dir(store.parts_dir()).await {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(err) => return Err(retry(err)),
    };
    let mut removed = 0;
    while let Some(entry) = entries.next_entry().await.map_err(retry)? {
        let Ok(meta) = entry.metadata().await else {
            continue;
        };
        if !meta.is_file() || modified(&meta).is_none_or(|m| m >= cutoff) {
            continue;
        }
        match tokio::fs::remove_file(entry.path()).await {
            Ok(()) => removed += 1,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => tracing::warn!(error = %err, "could not remove a stale upload chunk"),
        }
    }
    Ok(removed)
}

/// Runs one collection: rows first, then the sweep for files no row names.
///
/// # Errors
///
/// [`JobError::Retry`] on a database or directory failure.
pub async fn run_media_gc(
    pool: &PgPool,
    store: &FsMediaStore,
    now: DateTime<Utc>,
    ttl: Duration,
) -> Result<GcReport, JobError> {
    let ttl = chrono::Duration::from_std(ttl).map_err(|e| JobError::Fatal(e.to_string()))?;
    let cutoff = now - ttl;
    let rows = collect_rows(pool, store, now, cutoff).await?;
    let orphans = sweep_orphans(pool, store, cutoff).await? + sweep_parts(store, cutoff).await?;
    tracing::info!(rows, orphans, "media collected");
    Ok(GcReport { rows, orphans })
}

/// Registers the recurring `media_gc` job.
///
/// # Errors
///
/// [`JobsError`] when the schedule is invalid or the job type is already registered.
pub fn register(
    registry: &mut JobRegistry,
    pool: PgPool,
    store: Arc<FsMediaStore>,
    schedules: &JobSchedules,
    ttl: Duration,
    clock: Arc<dyn Clock>,
) -> Result<(), JobsError> {
    registry.register_recurring(
        &schedules.media_gc,
        postit_jobs::maintenance::maintenance_retry(),
        move |_: MediaGc, _: JobContext| {
            let pool = pool.clone();
            let store = Arc::clone(&store);
            let clock = Arc::clone(&clock);
            async move {
                run_media_gc(&pool, &store, clock.now(), ttl)
                    .await
                    .map(drop)
            }
        },
    )
}

/// Describes `media_gc` to the admin console.
pub fn console(catalog: &mut ConsoleCatalog, schedules: &JobSchedules) {
    catalog.add::<MediaGc>(ConsoleSpec::new().recurring(&schedules.media_gc));
}
