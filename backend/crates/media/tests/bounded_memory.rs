#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use bytes::Bytes;
use futures::StreamExt;
use peak_alloc::PeakAlloc;
use postit_core::{ByteStream, SystemClock};
use postit_data::OwnerScope;
use postit_data::testkit::seed_user;
use postit_media::testkit::{media_service, media_settings};
use sqlx::PgPool;

#[global_allocator]
static PEAK: PeakAlloc = PeakAlloc;

const MIB: u64 = 1024 * 1024;

/// `total` bytes in 64 KiB chunks, generated as they are pulled.
fn lazy(total: u64) -> ByteStream {
    futures::stream::unfold(0u64, move |sent| async move {
        if sent >= total {
            return None;
        }
        let n = (64 * 1024).min(total - sent);
        let chunk = Bytes::from(vec![0xAB; usize::try_from(n).unwrap()]);
        Some((Ok(chunk), sent + n))
    })
    .boxed()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn append_256_mib_stays_bounded(pool: PgPool) {
    let dir = tempfile::tempdir().unwrap();
    let mut settings = media_settings(dir.path());
    settings.chunk_max = 128 * MIB;
    let owner = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let service = media_service(pool, settings, Arc::new(SystemClock)).unwrap();
    let total = 256 * MIB;
    let session = service
        .start_session(&scope, "big.mp4", total)
        .await
        .unwrap();

    PEAK.reset_peak_usage();
    let base = PEAK.current_usage();
    for i in 0..2 {
        let start = i * 128 * MIB;
        let offset = service
            .append(
                &scope,
                session.id,
                start,
                start + 128 * MIB - 1,
                total,
                lazy(128 * MIB),
            )
            .await
            .unwrap();
        assert_eq!(offset, start + 128 * MIB);
    }
    let grew = PEAK.peak_usage() - base;
    assert!(
        grew < 32 * 1024 * 1024,
        "peak memory grew by {grew} bytes while appending 256 MiB"
    );
}
