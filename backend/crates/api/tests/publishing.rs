#![allow(clippy::unwrap_used, clippy::too_many_lines)]

mod common;

use axum::http::Method;
use chrono::{Duration, Utc};
use common::{Who, World, assert_problem, etag, world, world_with};
use postit_api::testkit::TestResponse;
use postit_core::{AccountId, PublishJobId};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

fn ids(targets: &[AccountId]) -> Vec<String> {
    targets.iter().map(ToString::to_string).collect()
}

fn live(targets: &[AccountId]) -> Value {
    json!({"targets": ids(targets), "confirm": true})
}

fn dry(targets: &[AccountId]) -> Value {
    json!({"targets": ids(targets), "dry_run": true})
}

fn soon() -> String {
    (Utc::now() + Duration::minutes(2)).to_rfc3339()
}

async fn publish(w: &World, who: Who, post: &str, tag: &str, body: Value) -> TestResponse {
    w.call_with(
        who,
        Method::POST,
        &format!("/api/v1/posts/{post}/publish"),
        Some(body),
        &[("if-match", tag)],
    )
    .await
}

/// A new post to `targets` published with `body` as the owner; returns the post ID and the
/// job's response.
async fn published(w: &World, targets: &[AccountId], body: Value) -> (String, TestResponse) {
    let (post, tag) = w.post_to("hello", targets).await;
    let res = publish(w, Who::Owner, &post, &tag, body).await;
    assert_eq!(res.status, 201, "{:?}", res.body);
    (post, res)
}

fn job_id(res: &TestResponse) -> PublishJobId {
    PublishJobId::from(Uuid::parse_str(res.body["id"].as_str().unwrap()).unwrap())
}

fn delivery_ids(job: &Value) -> Vec<String> {
    job["deliveries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["id"].as_str().unwrap().to_owned())
        .collect()
}

async fn job(w: &World, who: Who, id: &str) -> TestResponse {
    w.call(
        who,
        Method::GET,
        &format!("/api/v1/publish-jobs/{id}"),
        None,
    )
    .await
}

#[sqlx::test(migrations = "../data/migrations")]
async fn live_publish_without_confirm_is_400(pool: PgPool) {
    let w = world(pool).await;
    let (post, tag) = w.post_to("hello", &[w.a]).await;
    let res = publish(&w, Who::Owner, &post, &tag, json!({"targets": ids(&[w.a])})).await;
    assert_problem(&res, 400, "confirmation_required");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn publish_with_etag_from_before_override_is_412(pool: PgPool) {
    let w = world(pool).await;
    let (post, before) = w.post_to("hello", &[w.a]).await;
    let put = w
        .call_with(
            Who::Owner,
            Method::PUT,
            &format!("/api/v1/posts/{post}/overrides/{}", w.a),
            Some(json!({"patch": {"text": "for a"}})),
            &[("if-match", &before)],
        )
        .await;
    let after = etag(&put);
    assert_ne!(after, before);
    let stale = publish(&w, Who::Owner, &post, &before, live(&[w.a])).await;
    assert_problem(&stale, 412, "version_conflict");
    let fresh = publish(&w, Who::Owner, &post, &after, live(&[w.a])).await;
    assert_eq!(fresh.status, 201, "{:?}", fresh.body);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn publish_violations_are_422_with_every_violation(pool: PgPool) {
    let w = world(pool).await;
    let (post, tag) = w
        .create_post(json!({"text": "x".repeat(501), "targets": ids(&[w.a, w.b])}))
        .await
        .pipe_post();
    let res = publish(&w, Who::Owner, &post, &tag, live(&[w.a, w.b])).await;
    assert_problem(&res, 422, "validation_failed");
    let violations = res.body["violations"].as_array().unwrap();
    assert_eq!(violations.len(), 2, "{violations:?}");
    for v in violations {
        assert_eq!(v["code"], "text_too_long");
        assert!(v["params"]["account_id"].is_string());
    }
}

trait PostResponse {
    fn pipe_post(self) -> (String, String);
}

impl PostResponse for TestResponse {
    fn pipe_post(self) -> (String, String) {
        assert_eq!(self.status, 201, "{:?}", self.body);
        (self.body["id"].as_str().unwrap().to_owned(), etag(&self))
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn dry_run_publish_simulates_and_leaves_status_draft(pool: PgPool) {
    let w = world(pool).await;
    let (post, res) = published(&w, &[w.a], dry(&[w.a])).await;
    assert_eq!(res.body["dry_run"], true);
    assert!(w.app.drain_publishing().await >= 1);
    let after = job(&w, Who::Owner, res.body["id"].as_str().unwrap()).await;
    assert_eq!(after.body["deliveries"][0]["state"], "simulated");
    let post = w
        .call(
            Who::Owner,
            Method::GET,
            &format!("/api/v1/posts/{post}"),
            None,
        )
        .await;
    assert_eq!(post.body["status"], "draft");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn scheduled_publish_then_drain_publishes(pool: PgPool) {
    let w = world(pool).await;
    let mut body = live(&[w.a]);
    body["at"] = json!(soon());
    let (_, res) = published(&w, &[w.a], body).await;
    assert_eq!(res.body["status"], "scheduled");
    assert_eq!(w.app.drain_publishing().await, 0, "nothing is due yet");
    let id = res.body["id"].as_str().unwrap().to_owned();
    assert_eq!(job(&w, Who::Owner, &id).await.body["status"], "scheduled");
    w.app.make_job_due(job_id(&res)).await;
    assert!(w.app.drain_publishing().await >= 1);
    let done = job(&w, Who::Owner, &id).await;
    assert_eq!(done.body["status"], "published");
    assert_eq!(done.body["deliveries"][0]["state"], "published");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn publish_local_time_without_preferences_row_uses_utc(pool: PgPool) {
    let w = world(pool.clone()).await;
    sqlx::query("DELETE FROM user_preferences WHERE user_id = $1")
        .bind(w.owner.id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    let mut body = live(&[w.a]);
    body["at"] = json!({"local": "2036-06-16T12:00"});
    let (_, res) = published(&w, &[w.a], body).await;
    assert!(
        res.body["scheduled_at"]
            .as_str()
            .unwrap()
            .starts_with("2036-06-16T12:00:00"),
        "{:?}",
        res.body["scheduled_at"]
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn delegate_zone_is_their_own(pool: PgPool) {
    let w = world(pool).await;
    for (actor, zone) in [(&w.owner, "Asia/Tokyo"), (&w.publish, "Europe/Berlin")] {
        let res = w
            .app
            .call(
                Method::PUT,
                "/api/v1/me/preferences",
                Some(&actor.token),
                Some(json!({"timezone": zone})),
            )
            .await;
        assert_eq!(res.status, 200, "{:?}", res.body);
    }
    let (post, tag) = w.post_to("hello", &[w.a]).await;
    let mut body = live(&[w.a]);
    body["at"] = json!({"local": "2036-06-16T12:00"});
    let res = publish(&w, Who::Publish, &post, &tag, body).await;
    assert_eq!(res.status, 201, "{:?}", res.body);
    // Berlin is UTC+2 in June, so 12:00 there is 10:00 UTC; Tokyo would have been 03:00 UTC.
    assert!(
        res.body["scheduled_at"]
            .as_str()
            .unwrap()
            .starts_with("2036-06-16T10:00:00"),
        "{:?}",
        res.body["scheduled_at"]
    );
    assert_eq!(res.body["created_by"], w.publish.id.to_string());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn publish_schedule_gap_is_422_violation(pool: PgPool) {
    let w = world(pool).await;
    let (post, tag) = w.post_to("hello", &[w.a]).await;
    let mut body = live(&[w.a]);
    body["at"] = json!({"local": "2036-03-30T02:30", "zone": "Europe/Berlin"});
    let res = publish(&w, Who::Owner, &post, &tag, body).await;
    assert_problem(&res, 422, "validation_failed");
    assert_eq!(
        res.body["violations"][0]["code"],
        "schedule_time_nonexistent"
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn get_job_304_on_matching_etag(pool: PgPool) {
    let w = world(pool).await;
    let (_, res) = published(&w, &[w.a], live(&[w.a])).await;
    let id = res.body["id"].as_str().unwrap().to_owned();
    let tag = etag(&res);
    let path = format!("/api/v1/publish-jobs/{id}");
    let same = w
        .call_with(
            Who::Owner,
            Method::GET,
            &path,
            None,
            &[("if-none-match", &tag)],
        )
        .await;
    assert_eq!(same.status, 304);
    assert!(w.app.drain_publishing().await >= 1);
    let changed = w
        .call_with(
            Who::Owner,
            Method::GET,
            &path,
            None,
            &[("if-none-match", &tag)],
        )
        .await;
    assert_eq!(changed.status, 200, "the job changed after a delivery ran");
    assert_ne!(etag(&changed), tag);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn job_list_filters_by_campaign_and_dry_run(pool: PgPool) {
    let w = world(pool).await;
    let campaign = w
        .call(
            Who::Owner,
            Method::POST,
            "/api/v1/campaigns",
            Some(json!({"name": "c"})),
        )
        .await;
    let campaign = campaign.body["id"].as_str().unwrap().to_owned();
    let created = w
        .create_post(json!({"text": "in", "campaign_id": campaign, "targets": ids(&[w.a])}))
        .await
        .pipe_post();
    let first = publish(&w, Who::Owner, &created.0, &created.1, dry(&[w.a])).await;
    assert_eq!(first.status, 201, "{:?}", first.body);
    published(&w, &[w.a], dry(&[w.a])).await;
    let by_campaign = w
        .call(
            Who::Owner,
            Method::GET,
            &format!("/api/v1/publish-jobs?campaign_id={campaign}"),
            None,
        )
        .await;
    assert_eq!(by_campaign.body["total"], 1);
    for (query, total) in [("dry_run=true", 2), ("dry_run=false", 0)] {
        let res = w
            .call(
                Who::Owner,
                Method::GET,
                &format!("/api/v1/publish-jobs?{query}"),
                None,
            )
            .await;
        assert_eq!(res.body["total"], total, "{query}");
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn limited_delegate_sees_only_their_deliveries(pool: PgPool) {
    let w = world(pool).await;
    let (_, res) = published(&w, &[w.a, w.b], live(&[w.a, w.b])).await;
    let id = res.body["id"].as_str().unwrap().to_owned();
    assert_eq!(res.body["deliveries"].as_array().unwrap().len(), 2);
    let limited = job(&w, Who::Limited, &id).await;
    assert_eq!(limited.status, 200, "{:?}", limited.body);
    let deliveries = limited.body["deliveries"].as_array().unwrap();
    assert_eq!(deliveries.len(), 1);
    assert_eq!(deliveries[0]["account_id"], w.b.to_string());
    let list = w
        .call(Who::Limited, Method::GET, "/api/v1/publish-jobs", None)
        .await;
    assert_eq!(list.body["total"], 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn reschedule_pending_job(pool: PgPool) {
    let w = world(pool).await;
    let mut body = live(&[w.a, w.b]);
    body["at"] = json!(soon());
    let (_, res) = published(&w, &[w.a, w.b], body).await;
    let id = res.body["id"].as_str().unwrap().to_owned();
    let path = format!("/api/v1/publish-jobs/{id}");
    let moved = w
        .call_with(
            Who::Owner,
            Method::PATCH,
            &path,
            Some(json!({"at": {"local": "2036-06-16T12:00", "zone": "UTC"}})),
            &[("if-match", &etag(&res))],
        )
        .await;
    assert_eq!(moved.status, 200, "{:?}", moved.body);
    assert_eq!(moved.body["moved"].as_array().unwrap().len(), 2);
    assert_eq!(moved.body["failed"], json!([]));
    let after = job(&w, Who::Owner, &id).await;
    assert!(
        after.body["scheduled_at"]
            .as_str()
            .unwrap()
            .starts_with("2036-06-16T12:00:00")
    );
    let stale = w
        .call_with(
            Who::Owner,
            Method::PATCH,
            &path,
            Some(json!({"at": soon()})),
            &[("if-match", &etag(&res))],
        )
        .await;
    assert_problem(&stale, 412, "version_conflict");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn reschedule_started_job_is_409_not_reschedulable(pool: PgPool) {
    let w = world(pool).await;
    // A delivery that is neither pending nor a native upload holds the job still.
    let (id, _) = outcome_unknown_job(&w).await;
    let current = job(&w, Who::Owner, &id).await;
    let moved = w
        .call_with(
            Who::Owner,
            Method::PATCH,
            &format!("/api/v1/publish-jobs/{id}"),
            Some(json!({"at": soon()})),
            &[("if-match", &etag(&current))],
        )
        .await;
    assert_problem(&moved, 409, "not_reschedulable");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn cancel_job_reports_each_delivery(pool: PgPool) {
    let w = world(pool).await;
    let mut body = live(&[w.a, w.b]);
    body["at"] = json!(soon());
    let (_, res) = published(&w, &[w.a, w.b], body).await;
    let id = res.body["id"].as_str().unwrap().to_owned();
    let cancelled = w
        .call(
            Who::Owner,
            Method::POST,
            &format!("/api/v1/publish-jobs/{id}/cancel"),
            None,
        )
        .await;
    assert_eq!(cancelled.status, 200, "{:?}", cancelled.body);
    assert_eq!(cancelled.body["cancelled"].as_array().unwrap().len(), 2);
    assert_eq!(cancelled.body["requested"], json!([]));
    assert_eq!(job(&w, Who::Owner, &id).await.body["status"], "cancelled");
}

/// A published job whose single delivery ended in an unknown outcome, the mock unable to
/// look it up: the delivery's ID.
async fn outcome_unknown(w: &World) -> String {
    outcome_unknown_job(w).await.1
}

/// Like [`outcome_unknown`], with the job's ID first.
async fn outcome_unknown_job(w: &World) -> (String, String) {
    let (post, tag) = w.post_to("hello", &[w.a]).await;
    let put = w
        .call_with(
            Who::Owner,
            Method::PUT,
            &format!("/api/v1/posts/{post}/overrides/{}", w.a),
            Some(json!({"patch": {}, "options": {
                "outcome": "outcome_unknown_lost",
                "reconcile": "unsupported",
            }})),
            &[("if-match", &tag)],
        )
        .await;
    let res = publish(w, Who::Owner, &post, &etag(&put), live(&[w.a])).await;
    assert_eq!(res.status, 201, "{:?}", res.body);
    w.app.drain_publishing().await;
    let id = res.body["id"].as_str().unwrap().to_owned();
    let after = job(w, Who::Owner, &id).await;
    let delivery = delivery_ids(&after.body).remove(0);
    (id, delivery)
}

#[sqlx::test(migrations = "../data/migrations")]
async fn retry_without_confirm_is_400(pool: PgPool) {
    let w = world(pool).await;
    let delivery = outcome_unknown(&w).await;
    let res = w
        .call(
            Who::Owner,
            Method::POST,
            &format!("/api/v1/deliveries/{delivery}/retry"),
            Some(json!({})),
        )
        .await;
    assert_problem(&res, 400, "confirmation_required");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn retry_outcome_unknown_without_ack_is_400_acknowledgement_required(pool: PgPool) {
    let w = world(pool).await;
    let delivery = outcome_unknown(&w).await;
    let res = w
        .call(
            Who::Owner,
            Method::POST,
            &format!("/api/v1/deliveries/{delivery}/retry"),
            Some(json!({"confirm": true})),
        )
        .await;
    assert_problem(&res, 400, "acknowledgement_required");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn retry_published_delivery_is_409_not_retryable(pool: PgPool) {
    let w = world(pool).await;
    let (_, res) = published(&w, &[w.a], live(&[w.a])).await;
    assert!(w.app.drain_publishing().await >= 1);
    let delivery = delivery_ids(&res.body).remove(0);
    let retry = w
        .call(
            Who::Owner,
            Method::POST,
            &format!("/api/v1/deliveries/{delivery}/retry"),
            Some(json!({"confirm": true, "acknowledge_outcome_unknown": true})),
        )
        .await;
    assert_problem(&retry, 409, "not_retryable");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn resolve_pending_delivery_is_409_not_resolvable(pool: PgPool) {
    let w = world(pool).await;
    let mut body = live(&[w.a]);
    body["at"] = json!(soon());
    let (_, res) = published(&w, &[w.a], body).await;
    let delivery = delivery_ids(&res.body).remove(0);
    let resolve = w
        .call(
            Who::Owner,
            Method::POST,
            &format!("/api/v1/deliveries/{delivery}/resolve"),
            Some(json!({"outcome": "published"})),
        )
        .await;
    assert_problem(&resolve, 409, "not_resolvable");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn resolve_outcome_unknown_as_published(pool: PgPool) {
    let w = world(pool).await;
    let delivery = outcome_unknown(&w).await;
    let before = w
        .call(
            Who::Owner,
            Method::GET,
            &format!("/api/v1/deliveries/{delivery}"),
            None,
        )
        .await;
    assert!(
        ["outcome_unknown", "parked"].contains(&before.body["state"].as_str().unwrap()),
        "{:?}",
        before.body
    );
    let resolved = w
        .call(
            Who::Owner,
            Method::POST,
            &format!("/api/v1/deliveries/{delivery}/resolve"),
            Some(json!({"outcome": "published"})),
        )
        .await;
    assert_eq!(resolved.status, 200, "{:?}", resolved.body);
    assert_eq!(resolved.body["state"], "published");
    assert!(resolved.body["last_error"].is_object() || resolved.body["last_error"].is_null());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn delivery_cancel_answers_cancel_report(pool: PgPool) {
    let w = world(pool).await;
    let mut body = live(&[w.a]);
    body["at"] = json!(soon());
    let (_, res) = published(&w, &[w.a], body).await;
    let delivery = delivery_ids(&res.body).remove(0);
    let cancel = w
        .call(
            Who::Owner,
            Method::POST,
            &format!("/api/v1/deliveries/{delivery}/cancel"),
            None,
        )
        .await;
    assert_eq!(cancel.status, 200, "{:?}", cancel.body);
    assert_eq!(cancel.body["cancelled"], json!([delivery]));
    assert_eq!(cancel.body["not_cancellable"], json!([]));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn retry_answers_the_delivery_read_back(pool: PgPool) {
    let w = world(pool).await;
    let delivery = outcome_unknown(&w).await;
    let retry = w
        .call(
            Who::Owner,
            Method::POST,
            &format!("/api/v1/deliveries/{delivery}/retry"),
            Some(json!({"confirm": true, "acknowledge_outcome_unknown": true})),
        )
        .await;
    assert_eq!(retry.status, 200, "{:?}", retry.body);
    assert_eq!(retry.body["id"], delivery);
    assert_eq!(retry.body["state"], "pending");
    assert_eq!(retry.body["account_id"], w.a.to_string());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn publish_actions_are_rate_limited(pool: PgPool) {
    let w = world_with(pool, |s| {
        s.rate_limit.publish_actions = postit_config::RateBucket {
            rate_per_minute: 1,
            burst: 1,
        };
    })
    .await;
    let mut body = live(&[w.a]);
    body["at"] = json!(soon());
    let (post, res) = published(&w, &[w.a], body).await;
    let _ = post;
    let id = res.body["id"].as_str().unwrap().to_owned();
    let limited = w
        .call(
            Who::Owner,
            Method::POST,
            &format!("/api/v1/publish-jobs/{id}/cancel"),
            None,
        )
        .await;
    assert_problem(&limited, 429, "rate_limited");
    assert!(limited.headers.contains_key("retry-after"));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn publishing_access(pool: PgPool) {
    let w = world(pool).await;
    let (post, tag) = w.post_to("hello", &[w.a]).await;

    // A dry run needs edit; a live publish needs publish.
    let as_view = publish(&w, Who::View, &post, &tag, dry(&[w.a])).await;
    assert_problem(&as_view, 403, "capability_required");
    let as_edit_live = publish(&w, Who::Edit, &post, &tag, live(&[w.a])).await;
    assert_problem(&as_edit_live, 403, "capability_required");
    let as_edit_dry = publish(&w, Who::Edit, &post, &tag, dry(&[w.a])).await;
    assert_eq!(as_edit_dry.status, 201, "{:?}", as_edit_dry.body);
    // The limited delegate cannot name an account outside their limit.
    let hidden = publish(&w, Who::Limited, &post, &tag, dry(&[w.a])).await;
    assert_problem(&hidden, 403, "account_not_in_scope");
    let stranger = publish(&w, Who::Stranger, &post, &tag, dry(&[w.a])).await;
    assert_problem(&stranger, 404, "not_found");

    // A live job on both accounts, scheduled ahead.
    let (post2, tag2) = w.post_to("again", &[w.a, w.b]).await;
    let mut body = live(&[w.a, w.b]);
    body["at"] = json!(soon());
    let res = publish(&w, Who::Publish, &post2, &tag2, body).await;
    assert_eq!(res.status, 201, "{:?}", res.body);
    let id = res.body["id"].as_str().unwrap().to_owned();
    let job_path = format!("/api/v1/publish-jobs/{id}");
    for who in [Who::Owner, Who::View, Who::Edit, Who::Publish, Who::Limited] {
        assert_eq!(job(&w, who, &id).await.status, 200, "{who:?}");
    }
    assert_problem(&job(&w, Who::Stranger, &id).await, 404, "not_found");

    // Reschedule and cancel need the whole job inside the limit.
    let patch = |who| {
        let tag = etag(&res);
        let path = job_path.clone();
        let w = &w;
        async move {
            w.call_with(
                who,
                Method::PATCH,
                &path,
                Some(json!({"at": soon()})),
                &[("if-match", &tag)],
            )
            .await
        }
    };
    assert_problem(&patch(Who::Edit).await, 403, "capability_required");
    assert_problem(&patch(Who::Limited).await, 403, "account_not_in_scope");
    assert_problem(&patch(Who::Stranger).await, 404, "not_found");
    let cancel_path = format!("{job_path}/cancel");
    assert_problem(
        &w.call(Who::Limited, Method::POST, &cancel_path, None).await,
        403,
        "account_not_in_scope",
    );
    assert_problem(
        &w.call(Who::Edit, Method::POST, &cancel_path, None).await,
        403,
        "capability_required",
    );

    // A delivery on an account outside the limit is not there for the limited delegate.
    let deliveries = delivery_ids(&res.body);
    let hidden_delivery = res.body["deliveries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["account_id"] == w.a.to_string())
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(deliveries.contains(&hidden_delivery));
    let delivery_path = format!("/api/v1/deliveries/{hidden_delivery}");
    assert_problem(
        &w.call(Who::Limited, Method::GET, &delivery_path, None)
            .await,
        404,
        "not_found",
    );
    for (action, body) in [
        ("cancel", None),
        ("retry", Some(json!({"confirm": true}))),
        ("resolve", Some(json!({"outcome": "failed"}))),
    ] {
        let res = w
            .call(
                Who::Limited,
                Method::POST,
                &format!("{delivery_path}/{action}"),
                body,
            )
            .await;
        assert_problem(&res, 404, "not_found");
    }
    assert_problem(
        &w.call(Who::Stranger, Method::GET, &delivery_path, None)
            .await,
        404,
        "not_found",
    );
    assert_eq!(
        w.call(Who::View, Method::GET, &delivery_path, None)
            .await
            .status,
        200
    );
    assert_problem(
        &w.call(
            Who::Edit,
            Method::POST,
            &format!("{delivery_path}/cancel"),
            None,
        )
        .await,
        403,
        "capability_required",
    );
}

fn calendar_path(from: &str, to: &str) -> String {
    format!("/api/v1/calendar?from={from}&to={to}")
}

const DAY_START: &str = "2036-06-01T00:00:00Z";
const DAY_END: &str = "2036-06-02T00:00:00Z";

async fn scheduled_at(w: &World, targets: &[AccountId], at: &str) -> TestResponse {
    let mut body = live(targets);
    body["at"] = json!(at);
    published(w, targets, body).await.1
}

#[sqlx::test(migrations = "../data/migrations")]
async fn calendar_range_is_half_open_and_capped(pool: PgPool) {
    let w = world(pool).await;
    let at_from = scheduled_at(&w, &[w.a], DAY_START).await;
    scheduled_at(&w, &[w.a], DAY_END).await;
    scheduled_at(&w, &[w.a], "2036-05-31T23:59:59Z").await;
    let res = w
        .call(
            Who::Owner,
            Method::GET,
            &calendar_path(DAY_START, DAY_END),
            None,
        )
        .await;
    assert_eq!(res.status, 200, "{:?}", res.body);
    let items = res.body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1, "from is included, to is not");
    assert_eq!(items[0]["job_id"], at_from.body["id"]);
    assert_eq!(items[0]["kind"], "job");

    let sixty_two = w
        .call(
            Who::Owner,
            Method::GET,
            &calendar_path("2036-06-01T00:00:00Z", "2036-08-02T00:00:00Z"),
            None,
        )
        .await;
    assert_eq!(sixty_two.status, 200, "62 days are accepted");
    let too_long = w
        .call(
            Who::Owner,
            Method::GET,
            &calendar_path("2036-06-01T00:00:00Z", "2036-08-02T00:00:01Z"),
            None,
        )
        .await;
    assert_problem(&too_long, 422, "validation_failed");
    assert_eq!(too_long.body["field"], "to");
    let empty = w
        .call(
            Who::Owner,
            Method::GET,
            &calendar_path(DAY_START, DAY_START),
            None,
        )
        .await;
    assert_problem(&empty, 422, "validation_failed");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn calendar_excludes_dry_runs(pool: PgPool) {
    let w = world(pool).await;
    let mut body = dry(&[w.a]);
    body["at"] = json!("2036-06-01T12:00:00Z");
    published(&w, &[w.a], body).await;
    let res = w
        .call(
            Who::Owner,
            Method::GET,
            &calendar_path(DAY_START, DAY_END),
            None,
        )
        .await;
    assert_eq!(res.body["items"], json!([]));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn calendar_item_has_campaign_color_and_80_grapheme_head(pool: PgPool) {
    let w = world(pool).await;
    let campaign = w
        .call(
            Who::Owner,
            Method::POST,
            "/api/v1/campaigns",
            Some(json!({"name": "c", "color": "#ff8800"})),
        )
        .await;
    let campaign = campaign.body["id"].as_str().unwrap().to_owned();
    let text = format!("{}\nsecond line", "\u{1F600}".repeat(100));
    let (post, tag) = w
        .create_post(json!({
            "text": text,
            "campaign_id": campaign,
            "targets": ids(&[w.a]),
        }))
        .await
        .pipe_post();
    let mut body = live(&[w.a]);
    body["at"] = json!("2036-06-01T12:00:00Z");
    let _ = &post;
    let res = publish(&w, Who::Owner, &post, &tag, body).await;
    assert_eq!(res.status, 201, "{:?}", res.body);
    let cal = w
        .call(
            Who::Owner,
            Method::GET,
            &calendar_path(DAY_START, DAY_END),
            None,
        )
        .await;
    let item = &cal.body["items"][0];
    assert_eq!(item["campaign_id"], campaign);
    assert_eq!(item["campaign_color"], "#ff8800");
    assert_eq!(item["text_head"].as_str().unwrap().chars().count(), 80);
    assert_eq!(item["account_ids"], json!([w.a.to_string()]));
    assert_eq!(item["status"], "scheduled");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn calendar_304_on_matching_etag_and_changes_after_reschedule(pool: PgPool) {
    let w = world(pool).await;
    let res = scheduled_at(&w, &[w.a], "2036-06-01T12:00:00Z").await;
    let path = calendar_path(DAY_START, DAY_END);
    let first = w.call(Who::Owner, Method::GET, &path, None).await;
    let tag = etag(&first);
    let same = w
        .call_with(
            Who::Owner,
            Method::GET,
            &path,
            None,
            &[("if-none-match", &tag)],
        )
        .await;
    assert_eq!(same.status, 304);
    let id = res.body["id"].as_str().unwrap().to_owned();
    let moved = w
        .call_with(
            Who::Owner,
            Method::PATCH,
            &format!("/api/v1/publish-jobs/{id}"),
            Some(json!({"at": "2036-06-01T18:00:00Z"})),
            &[("if-match", &etag(&res))],
        )
        .await;
    assert_eq!(moved.status, 200, "{:?}", moved.body);
    let changed = w
        .call_with(
            Who::Owner,
            Method::GET,
            &path,
            None,
            &[("if-none-match", &tag)],
        )
        .await;
    assert_eq!(changed.status, 200);
    assert_ne!(etag(&changed), tag);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn calendar_hides_jobs_outside_limit(pool: PgPool) {
    let w = world(pool).await;
    scheduled_at(&w, &[w.a], "2036-06-01T10:00:00Z").await;
    scheduled_at(&w, &[w.a, w.b], "2036-06-01T11:00:00Z").await;
    let path = calendar_path(DAY_START, DAY_END);
    let owner = w.call(Who::Owner, Method::GET, &path, None).await;
    assert_eq!(owner.body["items"].as_array().unwrap().len(), 2);
    let limited = w.call(Who::Limited, Method::GET, &path, None).await;
    let items = limited.body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1, "the job on account a alone is hidden");
    assert_eq!(items[0]["account_ids"], json!([w.b.to_string()]));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn calendar_access(pool: PgPool) {
    let w = world(pool).await;
    scheduled_at(&w, &[w.a], "2036-06-01T10:00:00Z").await;
    let path = calendar_path(DAY_START, DAY_END);
    for who in [Who::Owner, Who::View, Who::Edit, Who::Publish] {
        let res = w.call(who, Method::GET, &path, None).await;
        assert_eq!(res.body["items"].as_array().unwrap().len(), 1, "{who:?}");
    }
    let stranger = w.call(Who::Stranger, Method::GET, &path, None).await;
    assert_eq!(stranger.status, 200);
    assert_eq!(stranger.body["items"], json!([]));
}
