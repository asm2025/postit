#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use postit_agent_mock::MockAgent;
use postit_api::testkit::{ADMIN_SUB, TestApp};
use postit_core::{Agent, PlatformId, PostId, PostKind, PostVersion};
use postit_data::OwnerScope;
use postit_data::audit::AuditContext;
use postit_data::platforms::PlatformRepo;
use postit_data::posts::{NewPost, PostRepo};
use postit_publishing::PublishInput;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

#[sqlx::test(migrations = "../data/migrations")]
async fn drain_runs_a_due_dry_run_job(pool: PgPool) {
    let app = TestApp::start_with_agents(
        pool.clone(),
        vec![Arc::new(MockAgent::broadcast()) as Arc<dyn Agent>],
        |_| {},
    )
    .await;
    let mock = PlatformId::new("mock").unwrap();
    PlatformRepo::upsert_registered(&pool, &[(mock.clone(), "Mock".to_owned())])
        .await
        .unwrap();
    // Create the admin user, then act as the owner of everything below.
    let token = app.token(ADMIN_SUB);
    let me = app
        .call(axum::http::Method::GET, "/api/v1/me", Some(&token), None)
        .await;
    let owner = postit_core::UserId::from_uuid(me.body["id"].as_str().unwrap().parse().unwrap());
    let scope = OwnerScope::own(owner);

    let Value::Object(values) = json!({"account_name": "main", "token": "s3cret-token-value"})
    else {
        unreachable!()
    };
    let check = app
        .state
        .accounts
        .start_credentials(&scope, &mock, values)
        .await
        .unwrap();
    let connected = app
        .state
        .accounts
        .confirm(
            &scope,
            check.pending.unwrap().0.as_str(),
            None,
            AuditContext::default(),
        )
        .await
        .unwrap();
    let account = connected.connected[0].0.id;

    let post = PostId::from_uuid(Uuid::now_v7());
    let mut conn = pool.acquire().await.unwrap();
    let row = PostRepo::create(
        &mut conn,
        &scope,
        NewPost {
            id: post,
            kind: PostKind::Post,
            reply_to: None,
            campaign_id: None,
            text: "hello".to_owned(),
            link: None,
            tags: vec![],
            disclose_ai_generated: false,
        },
    )
    .await
    .unwrap();
    drop(conn);
    postit_data::app_settings::AppSettingsRepo::seed_defaults(
        &mut pool.acquire().await.unwrap(),
        true,
    )
    .await
    .unwrap();

    let created = app
        .state
        .publishing
        .create_job(
            &scope,
            post,
            PublishInput {
                targets: vec![account],
                at: None,
                native_schedule: false,
                dry_run: true,
                confirm: false,
                if_match: PostVersion(row.version),
            },
            AuditContext::default(),
        )
        .await
        .unwrap();
    app.make_job_due(created.job).await;
    assert!(app.drain_publishing().await >= 1);
    let state: String = sqlx::query_scalar("SELECT state FROM deliveries WHERE job_id = $1")
        .bind(created.job.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "simulated");
}
