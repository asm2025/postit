//! The user deletion steps as the composition root orders them.

#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::sync::Arc;

use postit_config::{Environment, Settings, load_with};
use postit_jobs::JobQueue;
use postit_mail::MailOutbox;
use postit_server::compose;
use sqlx::PgPool;

fn settings(root: &Path) -> Settings {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config");
    let vars = [
        ("POSTIT__DATABASE__USERNAME", "unused"),
        ("POSTIT__DATABASE__PASSWORD", "unused"),
        ("POSTIT__AUDIT__PSEUDONYM_KEY", "deletion-test-key"),
        (
            "POSTIT__IDENTITY_MAIL__SIGNING_KEY",
            "identity-mail-test-key-0123456789",
        ),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .chain([(
        "POSTIT__MEDIA__STORAGE_ROOT".to_string(),
        root.display().to_string(),
    )]);
    load_with(Environment::Development, &dir, vars).unwrap()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn deletion_steps_run_publishing_then_oauth_then_media(pool: PgPool) {
    let dir = tempfile::tempdir().unwrap();
    let settings = settings(dir.path());
    let ids = compose::system_ids();
    let clock = compose::system_clock();
    let media = compose::media_service(&settings, &pool, &ids, &clock).unwrap();
    let http = postit_http::build_client(&settings.http).unwrap();
    let vault = postit_vault::Vault::from_settings(&settings.vault).unwrap();
    let agents = compose::agent_registry().unwrap();
    let configs = postit_server::platforms::resolve_platform_configs(&settings, &agents).unwrap();
    let accounts = compose::account_service(
        &settings,
        Environment::Development,
        &pool,
        &http,
        &vault,
        Arc::new(postit_oauth::PlatformCatalog::new(agents, configs)),
        (
            Arc::clone(&media.store) as Arc<dyn postit_core::MediaStore>,
            Arc::clone(&media.host) as Arc<dyn postit_core::MediaHost>,
        ),
        &ids,
        &clock,
    )
    .unwrap();
    let jobs = JobQueue::new(pool.clone(), Arc::clone(&ids), Arc::clone(&clock));
    let outbox = MailOutbox::new(jobs.clone());
    let notifier = compose::notifier(&settings, &ids, &clock, &jobs, &outbox);
    let publishing = compose::publishing(compose::PublishingParts {
        settings: &settings,
        pool: &pool,
        accounts: &accounts,
        media: &media,
        notifier: &notifier,
        jobs: &jobs,
        ids: &ids,
        clock: &clock,
        shutdown: tokio_util::sync::CancellationToken::new(),
        compiled_platforms: Vec::new(),
    });

    let names: Vec<&str> = compose::deletion_steps(&publishing, &accounts, &media)
        .iter()
        .map(|step| step.name())
        .collect();

    assert_eq!(names, ["publishing", "oauth", "media"]);
}
