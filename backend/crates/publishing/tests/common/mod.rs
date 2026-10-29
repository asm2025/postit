#![allow(dead_code, clippy::unwrap_used)]

use std::sync::Arc;

use bytes::Bytes;
use futures::StreamExt;
use postit_config::PublishingSettings;
use postit_core::{
    AccountId, AccountRef, ByteStream, Clock, IdGenerator, MediaAssetId, MediaItem, PlatformId,
    PostId, PostKind, PostVersion, PublishSnapshot, SystemClock, SystemIdGenerator, UserId,
    Variant,
};
use postit_data::OwnerScope;
use postit_data::testkit::seed_user;
use postit_jobs::JobQueue;
use postit_media::MediaService;
use postit_media::testkit::{media_service, media_settings};
use postit_notify::Notifier;
use postit_oauth::testkit::{account_service, test_accounts_settings};
use postit_publishing::{PublishingDeps, PublishingService};
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub struct Env {
    pub dir: tempfile::TempDir,
    pub pool: PgPool,
    pub owner: UserId,
    pub scope: OwnerScope,
    pub media: MediaService,
}

pub async fn env(pool: &PgPool) -> Env {
    let dir = tempfile::tempdir().unwrap();
    let owner = seed_user(pool, "active").await;
    let media = media_service(
        pool.clone(),
        media_settings(dir.path()),
        Arc::new(SystemClock),
    )
    .unwrap();
    Env {
        dir,
        pool: pool.clone(),
        owner,
        scope: OwnerScope::own(owner),
        media,
    }
}

pub fn publishing_settings() -> PublishingSettings {
    PublishingSettings {
        live: true,
        max_attempts: 5,
        lease: std::time::Duration::from_mins(5),
        sweep_grace: std::time::Duration::from_mins(2),
        park_grace: std::time::Duration::from_mins(15),
        processing_timeout: std::time::Duration::from_hours(24),
    }
}

pub fn service(env: &Env) -> PublishingService {
    let ids: Arc<dyn IdGenerator> = Arc::new(SystemIdGenerator);
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    PublishingService::new(PublishingDeps {
        pool: env.pool.clone(),
        accounts: account_service(env.pool.clone(), Vec::new(), test_accounts_settings()),
        media: env.media.clone(),
        notifier: Notifier::for_tests(env.pool.clone()),
        queue: JobQueue::new(env.pool.clone(), Arc::clone(&ids), Arc::clone(&clock)),
        ids,
        clock,
        settings: publishing_settings(),
        shutdown: CancellationToken::new(),
        compiled_platforms: vec![PlatformId::new("mock").unwrap()],
    })
}

fn png(w: u32, h: u32) -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(w, h, image::Rgba([10, 200, 30, 255]));
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

fn whole(bytes: &[u8]) -> ByteStream {
    futures::stream::iter(vec![Ok::<_, std::io::Error>(Bytes::copy_from_slice(bytes))]).boxed()
}

/// A ready image the owner uploaded.
pub async fn upload(env: &Env, name: &str) -> MediaAssetId {
    let bytes = png(40, 20);
    env.media
        .upload(&env.scope, name, bytes.len() as u64, whole(&bytes))
        .await
        .unwrap()
        .id
}

pub fn account_ref(account: AccountId) -> AccountRef {
    AccountRef {
        platform: PlatformId::new("mock").unwrap(),
        account,
        remote_id: "remote".to_owned(),
    }
}

pub fn variant(account: AccountId, media: &[MediaAssetId]) -> Variant {
    Variant {
        target: account_ref(account),
        kind: PostKind::Post,
        text: "hello".to_owned(),
        media: media
            .iter()
            .map(|asset| MediaItem {
                asset: *asset,
                alt_text: None,
            })
            .collect(),
        link: None,
        tags: Vec::new(),
        disclose_ai_generated: false,
        reply_to: None,
        email: None,
        options: serde_json::Map::new(),
    }
}

pub fn snapshot(variants: Vec<Variant>) -> PublishSnapshot {
    PublishSnapshot {
        post_id: PostId::from_uuid(Uuid::now_v7()),
        post_version: PostVersion(1),
        variants,
    }
}
