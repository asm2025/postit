use std::sync::Arc;
use std::time::Duration;

use postit_config::{BootstrapSettings, UserinfoMode};
use postit_core::{IdGenerator, SystemIdGenerator};
use postit_data::users::UserStatus;
use postit_identity::admin::UserAdminService;
use postit_identity::jobs::{IdentityJobs, register};
use postit_identity::testkit::{TestIssuer, claims_transformer};
use postit_jobs::testkit::{RunningWorker, wait_until};
use postit_jobs::{JobQueue, JobRegistry};
use postit_mail::{MailLoaders, MailOutbox, MemoryMailer, SendEmailDeps, SendEmailHandler};
use secrecy::SecretString;
use sqlx::PgPool;

#[sqlx::test(migrations = "../data/migrations")]
async fn sign_up_approval_and_deletion_flow_through_workers(pool: PgPool) {
    let ids: Arc<dyn IdGenerator> = Arc::new(SystemIdGenerator);
    let jobs = JobQueue::new(pool.clone(), Arc::clone(&ids));
    let outbox = MailOutbox::new(jobs.clone());
    let mailer = MemoryMailer::default();
    let mut registry = JobRegistry::default();
    let mut loaders = MailLoaders::default();
    register(
        &mut registry,
        &mut loaders,
        IdentityJobs {
            pool: pool.clone(),
            ids: Arc::clone(&ids),
            jobs: jobs.clone(),
            pseudonym_key: SecretString::from("e2e-key".to_string()),
            pending_ttl: Duration::from_hours(30 * 24),
            approval_email_interval: Duration::from_hours(1),
            audit: postit_config::AuditSettings {
                retention: Duration::from_hours(365 * 24),
                ip_retention: Duration::from_hours(90 * 24),
                pseudonym_key: postit_config::RedactedSecret::from("e2e-key".to_string()),
            },
            schedules: postit_jobs::testkit::jobs_settings().schedules,
        },
    )
    .unwrap_or_else(|e| unreachable!("identity register: {e}"));
    postit_mail::register(
        &mut registry,
        SendEmailHandler::new(SendEmailDeps {
            pool: pool.clone(),
            ids: Arc::clone(&ids),
            mailer: Arc::new(mailer.clone()),
            loaders,
            outbox: outbox.clone(),
            app_url: url::Url::parse("https://app.postit.test")
                .unwrap_or_else(|e| unreachable!("url: {e}")),
            max_attempts: 3,
        }),
    )
    .unwrap_or_else(|e| unreachable!("mail register: {e}"));
    let worker = RunningWorker::start(pool.clone(), registry).await;

    // Bootstrap admin, then a member signs up.
    let issuer = TestIssuer::start().await;
    let bootstrap = BootstrapSettings {
        admin_email: Some("admin@postit.test".into()),
        admin_subject: None,
    };
    let transformer = claims_transformer(pool.clone(), &issuer, UserinfoMode::Never, bootstrap);
    let admin = sign_in(
        &issuer,
        &transformer,
        "admin-sub",
        "admin@postit.test",
        "Admin",
    )
    .await;
    let member = sign_in(
        &issuer,
        &transformer,
        "member-sub",
        "member@postit.test",
        "Member",
    )
    .await;
    assert_eq!(member.status, UserStatus::Pending);

    // Every active admin is emailed.
    assert!(
        wait_until(Duration::from_secs(15), || mailer
            .sent()
            .iter()
            .any(|m| m.to == "admin@postit.test"))
        .await
    );
    let to_admin = mailer
        .sent()
        .into_iter()
        .find(|m| m.to == "admin@postit.test");
    assert!(to_admin.is_some_and(|m| m.text.contains("Member <member@postit.test>")));

    // Approval emails the user.
    let admins =
        UserAdminService::new(pool.clone(), Arc::clone(&ids), jobs.clone(), outbox.clone());
    admins
        .approve(admin.id, member.id)
        .await
        .unwrap_or_else(|e| unreachable!("approve: {e}"));
    assert!(
        wait_until(Duration::from_secs(15), || mailer
            .sent()
            .iter()
            .any(|m| m.to == "member@postit.test"))
        .await
    );

    // Deletion completes through the delete_user job.
    admins
        .delete_user(admin.id, member.id)
        .await
        .unwrap_or_else(|e| unreachable!("delete: {e}"));
    let gone = || async {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users WHERE id = $1")
            .bind(member.id.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap_or(1)
            == 0
    };
    let mut deleted = false;
    for _ in 0..150 {
        if gone().await {
            deleted = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(deleted);
    worker.stop().await;
}

async fn sign_in(
    issuer: &TestIssuer,
    transformer: &postit_identity::claims::ClaimsTransformer<
        postit_identity::discovery::HttpJwksSource,
    >,
    sub: &str,
    email: &str,
    name: &str,
) -> postit_data::users::UserRecord {
    let (token, claims) = issuer.token_and_claims(sub, Some(email), Some(true), Some(name));
    transformer
        .transform(&claims, &token)
        .await
        .unwrap_or_else(|e| unreachable!("transform {sub}: {e}"))
}
