//! An email campaign end to end, as the composition root wires it: the email agent from
//! `compose::agent_registry`, an account connected through `AccountService` against an
//! in-process SMTP sink, contacts from `ContactService`, and the campaign created, materialized,
//! and sent in batches through `PublishingService`.
//!
//! The jobs run by hand (`run_publish`, `run_batch`, `run_sweep`) instead of through a worker,
//! so each test controls the clock and the order. The sink echoes the address in every refusal,
//! as real servers often do, so the leak tests prove no server text reaches postit's records.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::too_many_lines
)]

mod campaign;

use campaign::*;
use chrono::{DateTime, Duration, Utc};
use postit_agent_email::testkit::{Received, Script};
use postit_config::Environment;
use postit_contacts::{ContactPatch, SettableStatus};
use postit_data::contacts::{SuppressionReason, SuppressionRepo};
use postit_http::testkit::leak::find_leak;
use postit_publishing::DirectSendBatch;
use postit_publishing::direct::run_batch;
use postit_publishing::sweep::run_sweep;
use postit_server::{Role, StartOptions, start};
use serde_json::json;
use sqlx::PgPool;

#[sqlx::test(migrations = "../data/migrations")]
async fn campaign_reaches_every_subscribed_contact(pool: PgPool) {
    let fx = fixture(&pool, Options::default()).await;
    fx.contact("ana@reader.example", Some("Ana")).await;
    fx.contact("ben@reader.example", Some("Ben")).await;
    fx.contact("cy@reader.example", None).await;
    let gone = fx.contact("dee@reader.example", Some("Dee")).await;
    fx.contacts
        .update(
            &fx.scope,
            gone,
            ContactPatch {
                status: Some(SettableStatus::Unsubscribed),
                ..ContactPatch::default()
            },
        )
        .await
        .unwrap();

    let dispatch = fx.campaign().await;
    fx.drain(dispatch).await;

    let received = fx.sink.received();
    assert_eq!(
        recipients(&received),
        [
            "ana@reader.example",
            "ben@reader.example",
            "cy@reader.example"
        ]
    );
    for mail in &received {
        assert_eq!(mail.from, FROM);
    }
    // Each mail is rendered for its recipient, with the fallback where a field is empty.
    assert_eq!(
        mail_to(&received, "ana@reader.example")
            .header("Subject")
            .as_deref(),
        Some("Fresh bread, Ana")
    );
    assert_eq!(
        mail_to(&received, "cy@reader.example")
            .header("Subject")
            .as_deref(),
        Some("Fresh bread, friend")
    );
    assert!(
        mail_to(&received, "ben@reader.example")
            .header("List-Unsubscribe")
            .is_some()
    );
    assert_eq!(
        fx.sink.connections(),
        2,
        "the check, then one batch session"
    );
    assert_eq!(fx.state(dispatch).await, "published");
    assert_eq!(
        fx.counts(dispatch).await,
        json!({"audience": 3, "queued": 0, "sent": 3, "failed": 0, "suppressed": 0, "bounced": 0})
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn suppressed_contact_is_skipped(pool: PgPool) {
    let fx = fixture(&pool, Options::default()).await;
    fx.contact("ana@reader.example", Some("Ana")).await;
    fx.contact("ben@reader.example", Some("Ben")).await;
    {
        let mut conn = pool.acquire().await.unwrap();
        SuppressionRepo::add(
            &mut conn,
            &fx.scope,
            &fx.signing_key(),
            "ben@reader.example",
            SuppressionReason::Manual,
        )
        .await
        .unwrap();
    }

    let dispatch = fx.campaign().await;
    fx.drain(dispatch).await;

    assert_eq!(recipients(&fx.sink.received()), ["ana@reader.example"]);
    assert_eq!(
        fx.rows(dispatch).await,
        [("ana@reader.example".to_owned(), "sent".to_owned(), 0, None)]
    );
    assert_eq!(fx.counts(dispatch).await["audience"], json!(1));
    assert_eq!(fx.state(dispatch).await, "published");
}

/// The URL inside a `List-Unsubscribe` header's angle brackets.
fn unsubscribe_url(mail: &Received) -> url::Url {
    let header = mail.header("List-Unsubscribe").unwrap();
    let inner: String = header
        .trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .split_whitespace()
        .collect();
    url::Url::parse(&inner).unwrap()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn unsubscribe_link_from_the_sent_mail_works(pool: PgPool) {
    let fx = fixture(&pool, Options::default()).await;
    let ana = fx.contact("ana@reader.example", Some("Ana")).await;
    let first = fx.campaign().await;
    fx.drain(first).await;
    let received = fx.sink.received();
    assert_eq!(received.len(), 1);
    let link = unsubscribe_url(&received[0]);
    assert!(link.path().starts_with("/public/u/"), "{link}");

    // The one-click unsubscribe a mail client sends, against the API the server starts.
    let running = start(StartOptions {
        env: Environment::Development,
        role: Role::Api,
        settings: fx.settings.clone(),
        // Its own pool: the server closes the pool it was given when it stops.
        db: Some(postit_data::Db::from_pool(
            sqlx::postgres::PgPoolOptions::new()
                .max_connections(4)
                .connect_with((*pool.connect_options()).clone())
                .await
                .unwrap(),
        )),
        api_listener: Some(std::net::TcpListener::bind("127.0.0.1:0").unwrap()),
        worker_listener: None,
        internal_listener: None,
    })
    .await
    .unwrap();
    let api = running.api_addr.unwrap();
    let target = format!("http://{api}{}", link.path());
    let response = postit_http::build_client(&fx.settings.http)
        .unwrap()
        .post(&target)
        .header("content-type", "application/x-www-form-urlencoded")
        .body("List-Unsubscribe=One-Click")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    running.shutdown.cancel();
    running.handle.await.unwrap().unwrap();

    assert_eq!(fx.contact_status(ana).await, "unsubscribed");
    let reason: String = sqlx::query_scalar("SELECT reason FROM suppressions WHERE owner_id = $1")
        .bind(fx.owner.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(reason, "unsubscribed");

    // The next campaign leaves the address out.
    let second = fx.campaign().await;
    fx.drain(second).await;
    assert_eq!(fx.counts(second).await["audience"], json!(0));
    assert_eq!(fx.sink.received().len(), 1, "nothing more was sent");
}

/// The sink refuses: an unknown mailbox (bounce), a full mailbox (soft, every try), and a
/// policy refusal after the message (rejected).
fn refusing() -> Script {
    Script {
        rcpt: vec![
            ("gone@reader.example".to_owned(), "550 5.1.1".to_owned()),
            ("busy@reader.example".to_owned(), "452 4.2.2".to_owned()),
        ],
        data: vec![("spam@reader.example".to_owned(), "554 5.7.1".to_owned())],
        ..Script::default()
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn per_recipient_results_are_exact(pool: PgPool) {
    let fx = fixture(
        &pool,
        Options {
            script: refusing(),
            ..Options::default()
        },
    )
    .await;
    for address in [
        "ana@reader.example",
        "gone@reader.example",
        "busy@reader.example",
        "spam@reader.example",
    ] {
        fx.contact(address, None).await;
    }

    let dispatch = fx.campaign().await;
    fx.drain(dispatch).await;

    assert_eq!(
        fx.rows(dispatch).await,
        [
            ("ana@reader.example".to_owned(), "sent".to_owned(), 0, None),
            (
                "busy@reader.example".to_owned(),
                "failed".to_owned(),
                2,
                Some("smtp_4_2_2".to_owned())
            ),
            (
                "gone@reader.example".to_owned(),
                "bounced".to_owned(),
                0,
                Some("smtp_5_1_1".to_owned())
            ),
            (
                "spam@reader.example".to_owned(),
                "failed".to_owned(),
                0,
                Some("smtp_5_7_1".to_owned())
            ),
        ]
    );
    let sent: (Option<String>, bool) = sqlx::query_as(
        "SELECT provider_message_id, sent_at IS NOT NULL FROM direct_sends \
         WHERE delivery_id = $1 AND state = 'sent'",
    )
    .bind(dispatch.delivery.as_uuid())
    .fetch_one(&pool)
    .await
    .unwrap();
    let id = sent.0.unwrap();
    assert!(id.ends_with("@bakery.example"), "{id}");
    assert!(sent.1);
    assert_eq!(
        mail_to(&fx.sink.received(), "ana@reader.example")
            .header("Message-ID")
            .as_deref(),
        Some(format!("<{id}>").as_str())
    );
    assert_eq!(
        fx.counts(dispatch).await,
        json!({"audience": 4, "queued": 0, "sent": 1, "failed": 2, "suppressed": 0, "bounced": 1})
    );
    assert_eq!(fx.state(dispatch).await, "published");
    // Only the bounce suppresses.
    let reasons: Vec<String> =
        sqlx::query_scalar("SELECT reason FROM suppressions WHERE owner_id = $1")
            .bind(fx.owner.as_uuid())
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(reasons, ["bounced"]);
    // The soft failure was offered once per try.
    let busy = fx
        .sink
        .rcpts()
        .iter()
        .filter(|(_, a)| a == "busy@reader.example")
        .count();
    assert_eq!(busy, 3);
}

/// The owner's first midnight after `at`, in UTC: the seeded owner has no preferences row,
/// so their time zone is UTC.
fn next_midnight(at: DateTime<Utc>) -> DateTime<Utc> {
    (at.date_naive() + chrono::Days::new(1))
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn cap_crossing_continues_next_day(pool: PgPool) {
    let fx = fixture(
        &pool,
        Options {
            daily_cap: 2,
            ..Options::default()
        },
    )
    .await;
    for address in [
        "ana@reader.example",
        "ben@reader.example",
        "cy@reader.example",
    ] {
        fx.contact(address, None).await;
    }

    // Today's batches run on one clock reading, so the day cannot change under the test.
    let start = Utc::now();
    let today = fx.publishing_at(start);
    let dispatch = fx.campaign().await;
    fx.drain_with(&today, dispatch).await;
    assert_eq!(fx.sink.received().len(), 2, "the cap stops today's sends");
    assert_eq!(fx.state(dispatch).await, "processing");
    let waiting: Vec<_> = fx
        .rows(dispatch)
        .await
        .into_iter()
        .filter(|(_, state, _, _)| state == "queued")
        .collect();
    assert_eq!(waiting.len(), 1, "{waiting:?}");
    assert_eq!((waiting[0].2, waiting[0].3.as_deref()), (0, None));
    // The live progress (the stored counts are a snapshot taken at the start and the end).
    let progress = {
        let mut conn = pool.acquire().await.unwrap();
        postit_data::publishing::direct::DirectRepo::progress(&mut conn, dispatch.delivery)
            .await
            .unwrap()
    };
    assert_eq!((progress.sent, progress.queued), (2, 1));
    let midnight = next_midnight(start);
    assert_eq!(fx.batch_jobs(dispatch).await.last(), Some(&midnight));

    let tomorrow = fx.publishing_at(midnight + Duration::seconds(1));
    fx.drain_with(&tomorrow, dispatch).await;
    assert_eq!(
        recipients(&fx.sink.received()),
        [
            "ana@reader.example",
            "ben@reader.example",
            "cy@reader.example"
        ]
    );
    assert_eq!(fx.state(dispatch).await, "published");
    assert_eq!(fx.counts(dispatch).await["sent"], json!(3));
}

/// A campaign to `addresses` whose first batch the sink refuses as a sender problem under
/// `script`: it pauses with every row queued and the account waiting for a reconnect. Once
/// `fix` clears the refusal and the user reconnects, the sweep resumes it and everyone gets
/// the mail.
async fn pauses_and_resumes(
    pool: &PgPool,
    script: Script,
    addresses: &[&str],
    fix: fn(&mut Script),
) {
    let fx = fixture(
        pool,
        Options {
            script,
            daily_cap: 10,
        },
    )
    .await;
    for address in addresses {
        fx.contact(address, None).await;
    }

    let dispatch = fx.campaign().await;
    run_batch(&fx.publishing, DirectSendBatch::from(dispatch))
        .await
        .unwrap();

    // Paused: nobody sent, nobody failed or suppressed, the account waits for a reconnect.
    assert!(fx.sink.received().is_empty());
    let queued: Vec<_> = addresses
        .iter()
        .map(|a| ((*a).to_owned(), "queued".to_owned(), 0, None))
        .collect();
    assert_eq!(fx.rows(dispatch).await, queued);
    let suppressions: i64 =
        sqlx::query_scalar("SELECT count(*) FROM suppressions WHERE owner_id = $1")
            .bind(fx.owner.as_uuid())
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(suppressions, 0);
    assert_eq!(fx.account_status().await, "needs_reconnect");
    assert_eq!(fx.state(dispatch).await, "processing");
    assert_eq!(fx.cap_units().await, 0, "the batch's cap units are back");
    let notified: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM notifications WHERE user_id = $1 AND kind = 'account_needs_reconnect'",
    )
    .bind(fx.owner.as_uuid())
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(notified, 1);

    // The relay now accepts the sender; the user reconnects the same setup.
    fx.sink.script(fix);
    let again = connect(&fx.accounts, &fx.scope, &fx.sink).await;
    assert_eq!(again, fx.account);
    assert_eq!(fx.account_status().await, "active");

    // The sweep finds the paused campaign and queues its next batch.
    let later = fx.publishing_at(Utc::now() + Duration::minutes(10));
    assert_eq!(run_sweep(&later).await.unwrap().orphaned_direct, 1);
    fx.drain(dispatch).await;
    assert_eq!(recipients(&fx.sink.received()), addresses);
    assert_eq!(fx.state(dispatch).await, "published");
    assert_eq!(
        fx.cap_units().await,
        i64::try_from(addresses.len()).unwrap(),
        "only the sends use the cap"
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn sender_refusal_pauses_and_resumes_after_reconnect(pool: PgPool) {
    let script = Script {
        mail_refusal: Some((0, "550 5.7.1".to_owned())),
        ..Script::default()
    };
    pauses_and_resumes(
        &pool,
        script,
        &["ana@reader.example", "ben@reader.example"],
        |script| script.mail_refusal = None,
    )
    .await;
}

#[sqlx::test(migrations = "../data/migrations")]
async fn three_leading_policy_refusals_pause_and_resume_after_reconnect(pool: PgPool) {
    let addresses = [
        "ana@reader.example",
        "ben@reader.example",
        "cy@reader.example",
        "dee@reader.example",
    ];
    let script = Script {
        rcpt: addresses
            .iter()
            .map(|a| ((*a).to_owned(), "550 5.7.1".to_owned()))
            .collect(),
        ..Script::default()
    };
    pauses_and_resumes(&pool, script, &addresses, |script| script.rcpt.clear()).await;
}

#[sqlx::test(migrations = "../data/migrations")]
async fn no_address_in_error_codes(pool: PgPool) {
    let fx = fixture(
        &pool,
        Options {
            script: echoing(),
            ..Options::default()
        },
    )
    .await;
    let (first, _) = leaky_campaigns(&fx).await;

    let mut first_codes: Vec<String> = sqlx::query_scalar(
        "SELECT error_code FROM direct_sends WHERE delivery_id = $1 AND error_code IS NOT NULL",
    )
    .bind(first.delivery.as_uuid())
    .fetch_all(&pool)
    .await
    .unwrap();
    first_codes.sort();
    assert_eq!(
        first_codes,
        ["smtp_4_2_2", "smtp_5_1_1", "smtp_5_7_1", "smtp_connection"],
        "each row keeps the agent's fixed code"
    );
    let codes: Vec<String> =
        sqlx::query_scalar("SELECT error_code FROM direct_sends WHERE error_code IS NOT NULL")
            .fetch_all(&pool)
            .await
            .unwrap();
    for code in &codes {
        assert!(
            code.starts_with("smtp_")
                && code
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
            "{code}"
        );
    }
    let mut records: Vec<(String, String)> = codes
        .into_iter()
        .map(|c| ("direct_sends.error_code".to_owned(), c))
        .collect();
    for (label, sql) in [
        (
            "deliveries",
            "SELECT coalesce(last_error::text, '') || coalesce(direct_counts::text, '') \
             FROM deliveries",
        ),
        (
            "delivery_attempts",
            "SELECT coalesce(error::text, '') || coalesce(request_summary::text, '') \
             FROM delivery_attempts",
        ),
        (
            "job_attempts",
            "SELECT coalesce(error_code, '') || coalesce(error_message, '') FROM job_attempts",
        ),
        ("jobs", "SELECT payload::text FROM jobs"),
        ("notifications", "SELECT payload::text FROM notifications"),
        ("audit_events", "SELECT details::text FROM audit_events"),
    ] {
        let texts: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
            .fetch_all(&pool)
            .await
            .unwrap();
        records.extend(texts.into_iter().map(|t| (label.to_owned(), t)));
    }
    let secrets = leaky_secrets();
    let secrets: Vec<&str> = secrets.iter().map(String::as_str).collect();
    let outputs: Vec<(&str, &str)> = records
        .iter()
        .map(|(label, text)| (label.as_str(), text.as_str()))
        .collect();
    if let Some(leak) = find_leak(&secrets, &outputs) {
        panic!("{leak}");
    }
}
