//! Comments, the inbox, metrics, native schedules, direct sends, and revoke on the mock agent.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::TimeDelta;
use common::{Progress, TOKEN, account, client, context, key, variant};
use postit_agent_kit::testkit::TestContext;
use postit_agent_kit::testkit::conformance::{Fault, Faults as _};
use postit_agent_mock::MockAgent;
use postit_core::{
    AgentClient, AgentError, Clock, DeliveryId, DirectRecipient, DirectSendId, DirectSendOutcome,
    EmailBlock, EmailDocument, PostKind, PublishMode, PublishOutcome, PublishReceipt,
    PublishRequest, ReconcileOutcome, ReconcileRequest, RetryClass, TestClock, Variant,
    ViolationCode,
};
use serde_json::json;
use url::Url;
use uuid::Uuid;

fn request(variant: &Variant, key: DeliveryId) -> PublishRequest<'_> {
    PublishRequest {
        variant,
        mode: PublishMode::Live,
        native_publish_at: None,
        container_ref: None,
        idempotency_key: key,
    }
}

fn receipt(outcome: PublishOutcome) -> PublishReceipt {
    match outcome {
        PublishOutcome::Published(r) | PublishOutcome::Processing(r) => r,
        PublishOutcome::Simulated { planned } => panic!("a receipt expected: {planned:?}"),
    }
}

/// Publishes a text post with `comments` seeded comments and returns its receipt.
async fn post(mock: &dyn AgentClient, comments: u32) -> PublishReceipt {
    let mut v = variant(None);
    v.options.insert("comments".to_owned(), json!(comments));
    receipt(
        mock.publish(request(&v, key()), &Progress(false))
            .await
            .unwrap(),
    )
}

/// A comment variant answering `target`.
fn reply(target: postit_core::RemoteTarget, outcome: Option<&str>) -> Variant {
    Variant {
        kind: PostKind::Comment,
        text: "thanks!".to_owned(),
        reply_to: Some(target.reply_to),
        ..variant(outcome)
    }
}

/// The message key of a single agent-rule violation.
fn rule_key(err: &AgentError) -> String {
    let AgentError::Validation(violations) = err else {
        panic!("Validation expected, got {err:?}");
    };
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert_eq!(violations[0].code, ViolationCode::AgentRule);
    violations[0].params["message"]["key"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn setup() -> (MockAgent, TestContext) {
    (MockAgent::broadcast(), context(TOKEN))
}

#[tokio::test]
async fn comment_on_resolved_target() {
    let (agent, ctx) = setup();
    let mock = client(&agent, &ctx);
    let posted = post(mock.as_ref(), 0).await;
    let commenter = mock.commenter().expect("mock comments");
    let url = posted.remote_url.clone().unwrap();
    let target = commenter
        .resolve_target(&account("mock"), &url)
        .await
        .unwrap();
    assert_eq!(target.reply_to.remote_post_id, "mock-post-1");
    assert_eq!(target.reply_to.remote_comment_id, None);

    let answer = reply(target, None);
    let k = key();
    let first = receipt(
        commenter
            .comment(request(&answer, k), &Progress(false))
            .await
            .unwrap(),
    );
    let comments = agent.script().comments();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].post_remote_id, "mock-post-1");
    assert_eq!(comments[0].text, "thanks!");
    assert_eq!(
        first.remote_id.as_deref(),
        Some(comments[0].remote_id.as_str())
    );

    // The same key returns the stored reply instead of posting twice, even when the platform
    // would now refuse.
    let refused = Variant {
        options: variant(Some("permanent_failure")).options,
        ..answer.clone()
    };
    let again = commenter
        .comment(request(&refused, k), &Progress(false))
        .await
        .unwrap();
    assert_eq!(receipt(again), first);
    assert_eq!(agent.script().comments().len(), 1);

    let found = commenter
        .reconcile_comment(ReconcileRequest {
            variant: &answer,
            container_ref: None,
            attempted_at: ctx.clock.now(),
            idempotency_key: k,
        })
        .await
        .unwrap();
    assert_eq!(found, ReconcileOutcome::Found(first));
}

#[tokio::test]
async fn comment_follows_outcome_and_faults() {
    let (agent, ctx) = setup();
    let mock = client(&agent, &ctx);
    let posted = post(mock.as_ref(), 0).await;
    let commenter = mock.commenter().unwrap();
    let target = commenter
        .resolve_target(&account("mock"), posted.remote_url.as_ref().unwrap())
        .await
        .unwrap();

    let failing = reply(target.clone(), Some("retryable_failure"));
    let err = commenter
        .comment(request(&failing, key()), &Progress(false))
        .await
        .unwrap_err();
    assert_eq!(err.retry_class(), RetryClass::Retryable);
    assert!(agent.script().comments().is_empty());

    let landed = reply(target.clone(), Some("outcome_unknown_landed"));
    let k = key();
    let err = commenter
        .comment(request(&landed, k), &Progress(false))
        .await
        .unwrap_err();
    assert_eq!(err.retry_class(), RetryClass::Ambiguous);
    let reconciled = commenter
        .reconcile_comment(ReconcileRequest {
            variant: &landed,
            container_ref: None,
            attempted_at: ctx.clock.now(),
            idempotency_key: k,
        })
        .await
        .unwrap();
    assert!(
        matches!(reconciled, ReconcileOutcome::Found(_)),
        "{reconciled:?}"
    );

    let lost = reply(target.clone(), Some("outcome_unknown_lost"));
    let k = key();
    assert!(
        commenter
            .comment(request(&lost, k), &Progress(false))
            .await
            .is_err()
    );
    let reconciled = commenter
        .reconcile_comment(ReconcileRequest {
            variant: &lost,
            container_ref: None,
            attempted_at: ctx.clock.now(),
            idempotency_key: k,
        })
        .await
        .unwrap();
    assert_eq!(reconciled, ReconcileOutcome::Absent);

    let script = agent.script();
    script.arm(Fault::Unauthorized).await;
    let err = commenter
        .comment(
            request(&reply(target.clone(), None), key()),
            &Progress(false),
        )
        .await
        .unwrap_err();
    script.clear().await;
    assert_eq!(err.retry_class(), RetryClass::NeedsReconnect);

    let cancelled = commenter
        .comment(request(&reply(target, None), key()), &Progress(true))
        .await;
    assert_eq!(cancelled, Err(AgentError::Cancelled));
}

#[tokio::test]
async fn comment_dry_run_calls_nothing() {
    let (agent, ctx) = setup();
    let mock = client(&agent, &ctx);
    let commenter = mock.commenter().unwrap();
    let calls = agent.script().remote_calls();
    let target = postit_core::RemoteTarget {
        reply_to: postit_core::ReplyTo {
            remote_post_id: "mock-post-1".to_owned(),
            remote_comment_id: None,
            url: None,
        },
    };
    let answer = reply(target, None);
    let dry = |variant| PublishRequest {
        mode: PublishMode::DryRun,
        ..request(variant, key())
    };
    let outcome = commenter
        .comment(dry(&answer), &Progress(false))
        .await
        .unwrap();
    assert!(
        matches!(outcome, PublishOutcome::Simulated { .. }),
        "{outcome:?}"
    );

    // Without a reply target the dry run refuses, as the live call does.
    let orphan = Variant {
        kind: PostKind::Comment,
        ..variant(None)
    };
    let err = commenter
        .comment(dry(&orphan), &Progress(false))
        .await
        .unwrap_err();
    assert_eq!(rule_key(&err), "mock.reply_to_missing");
    assert_eq!(agent.script().remote_calls(), calls);
}

#[tokio::test]
async fn unknown_target_rejected() {
    let (agent, ctx) = setup();
    let mock = client(&agent, &ctx);
    let commenter = mock.commenter().unwrap();
    for url in [
        "https://mock.test/posts/mock-post-99",
        "https://example.com/posts/mock-post-1",
        "https://mock.test/users/shop",
    ] {
        let err = commenter
            .resolve_target(&account("mock"), &Url::parse(url).unwrap())
            .await
            .unwrap_err();
        assert_eq!(rule_key(&err), "mock.target_unknown", "{url}");
    }

    // A reply link to a comment the platform does not have, or one on another post.
    post(mock.as_ref(), 1).await;
    post(mock.as_ref(), 0).await;
    for url in [
        "https://mock.test/posts/mock-post-1/comments/mock-comment-99",
        "https://mock.test/posts/mock-post-2/comments/mock-comment-1",
        "https://mock.test/posts/mock-post-1/comments/",
        "https://mock.test/posts/mock-post-1/other/mock-comment-1",
    ] {
        let err = commenter
            .resolve_target(&account("mock"), &Url::parse(url).unwrap())
            .await
            .unwrap_err();
        assert_eq!(rule_key(&err), "mock.target_unknown", "{url}");
    }

    // A live comment on a post the platform does not have is refused the same way.
    let missing = postit_core::RemoteTarget {
        reply_to: postit_core::ReplyTo {
            remote_post_id: "mock-post-99".to_owned(),
            remote_comment_id: None,
            url: None,
        },
    };
    let err = commenter
        .comment(request(&reply(missing, None), key()), &Progress(false))
        .await
        .unwrap_err();
    assert_eq!(rule_key(&err), "mock.target_unknown");
    assert_eq!(
        agent.script().comments().len(),
        1,
        "only the seeded comment"
    );
}

#[tokio::test]
async fn resolve_target_accepts_reply_links_and_trailing_slashes() {
    let (agent, ctx) = setup();
    let mock = client(&agent, &ctx);
    post(mock.as_ref(), 1).await;
    let commenter = mock.commenter().unwrap();

    let slash = Url::parse("https://mock.test/posts/mock-post-1/").unwrap();
    let target = commenter
        .resolve_target(&account("mock"), &slash)
        .await
        .unwrap();
    assert_eq!(target.reply_to.remote_post_id, "mock-post-1");
    assert_eq!(target.reply_to.remote_comment_id, None);

    // The link a reply's receipt carries resolves to that comment.
    let answer = reply(target, None);
    let sent = receipt(
        commenter
            .comment(request(&answer, key()), &Progress(false))
            .await
            .unwrap(),
    );
    for link in [
        sent.remote_url.clone().unwrap(),
        Url::parse("https://mock.test/posts/mock-post-1/comments/mock-comment-1/").unwrap(),
    ] {
        let target = commenter
            .resolve_target(&account("mock"), &link)
            .await
            .unwrap();
        assert_eq!(target.reply_to.remote_post_id, "mock-post-1");
        assert!(target.reply_to.remote_comment_id.is_some(), "{link}");
        assert_eq!(target.reply_to.url, Some(link));
    }
    assert_eq!(
        commenter
            .resolve_target(&account("mock"), &sent.remote_url.unwrap())
            .await
            .unwrap()
            .reply_to
            .remote_comment_id,
        sent.remote_id
    );
}

#[tokio::test]
async fn inbox_returns_seeded_comments_and_cursor() {
    let (agent, ctx) = setup();
    let mock = client(&agent, &ctx);
    post(mock.as_ref(), 2).await;
    post(mock.as_ref(), 1).await;
    let inbox = mock.inbox().expect("mock reads its inbox");

    let page = inbox.fetch_since(&account("mock"), None).await.unwrap();
    let ids: Vec<&str> = page
        .items
        .iter()
        .map(|i| i.remote_comment_id.as_str())
        .collect();
    assert_eq!(ids, ["mock-comment-1", "mock-comment-2", "mock-comment-3"]);
    let authors: Vec<&str> = page
        .items
        .iter()
        .map(|i| i.author_handle.as_str())
        .collect();
    assert_eq!(authors, ["@fan1", "@fan2", "@fan1"]);
    assert_eq!(page.items[2].remote_post_id, "mock-post-2");
    assert_eq!(page.cursor.as_deref(), Some("mock-comment-3"));

    let after = inbox
        .fetch_since(&account("mock"), Some("mock-comment-2"))
        .await
        .unwrap();
    assert_eq!(after.items.len(), 1);
    assert_eq!(after.items[0].remote_comment_id, "mock-comment-3");

    let caught_up = inbox
        .fetch_since(&account("mock"), page.cursor.as_deref())
        .await
        .unwrap();
    assert!(caught_up.items.is_empty());
    assert_eq!(caught_up.cursor.as_deref(), Some("mock-comment-3"));

    // Another account's posts are not in this inbox.
    let mut other = account("mock");
    other.remote_id = "mock-account-other".to_owned();
    assert!(
        inbox
            .fetch_since(&other, None)
            .await
            .unwrap()
            .items
            .is_empty()
    );
}

#[tokio::test]
async fn push_comment_appears_after_cursor() {
    let (agent, ctx) = setup();
    let mock = client(&agent, &ctx);
    let posted = post(mock.as_ref(), 1).await;
    let inbox = mock.inbox().unwrap();
    let first = inbox.fetch_since(&account("mock"), None).await.unwrap();
    assert_eq!(first.items.len(), 1);

    let pushed = agent
        .script()
        .push_comment(posted.remote_id.as_deref().unwrap(), "late fan")
        .expect("the post exists");
    assert_eq!(pushed, "mock-comment-2");
    assert_eq!(agent.script().push_comment("mock-post-99", "nobody"), None);

    let next = inbox
        .fetch_since(&account("mock"), first.cursor.as_deref())
        .await
        .unwrap();
    assert_eq!(next.items.len(), 1);
    assert_eq!(next.items[0].remote_comment_id, "mock-comment-2");
    assert_eq!(next.items[0].text, "late fan");
    assert_eq!(next.items[0].author_handle, "@fan2");
    assert_eq!(next.cursor.as_deref(), Some("mock-comment-2"));
}

#[tokio::test]
async fn metrics_deterministic() {
    let (agent, ctx) = setup();
    let mock = client(&agent, &ctx);
    let one = post(mock.as_ref(), 2).await;
    let two = post(mock.as_ref(), 0).await;
    let metrics = mock.metrics().expect("mock reports metrics");

    let first = metrics.post_metrics(&account("mock"), &one).await.unwrap();
    assert_eq!(
        first,
        metrics.post_metrics(&account("mock"), &one).await.unwrap()
    );
    // "hello from the mock" is 19 characters: views 100 + 10 * 19.
    assert_eq!(first.views, Some(290));
    assert_eq!(first.likes, Some(29));
    assert_eq!(first.comments, Some(2));
    assert_eq!(first.shares, Some(7));
    assert_eq!(first.saves, Some(5));
    assert_eq!(first.clicks, None);
    assert_eq!(first.collected_at, ctx.clock.now());

    let second = metrics.post_metrics(&account("mock"), &two).await.unwrap();
    assert_eq!(second.comments, Some(0));

    let totals = metrics.account_metrics(&account("mock")).await.unwrap();
    assert_eq!(totals.reach, Some(580));
    assert_eq!(totals.followers, Some(58));

    let unknown = PublishReceipt {
        remote_id: Some("mock-post-99".to_owned()),
        ..one
    };
    let err = metrics
        .post_metrics(&account("mock"), &unknown)
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            AgentError::Remote {
                status: Some(404),
                ..
            }
        ),
        "{err:?}"
    );
}

async fn scheduled(mock: &dyn AgentClient, ctx: &TestContext, lead: TimeDelta) -> PublishReceipt {
    let at = ctx.clock.now() + lead;
    receipt(
        mock.publish(
            PublishRequest {
                native_publish_at: Some(at),
                ..request(&variant(None), key())
            },
            &Progress(false),
        )
        .await
        .unwrap(),
    )
}

#[tokio::test]
async fn reschedule_and_delete() {
    let (agent, ctx) = setup();
    let mock = client(&agent, &ctx);
    let first = scheduled(mock.as_ref(), &ctx, TimeDelta::hours(2)).await;
    let scheduler = mock.native_scheduler().expect("mock schedules natively");

    let later = ctx.clock.now() + TimeDelta::days(2);
    let moved = scheduler
        .reschedule(&account("mock"), &first, later)
        .await
        .unwrap();
    assert_eq!(moved.remote_publish_at, Some(later));
    assert_eq!(moved.remote_id, first.remote_id);
    assert_eq!(agent.script().posts()[0].publish_at, Some(later));

    scheduler.delete(&account("mock"), &moved).await.unwrap();
    assert!(agent.script().posts().is_empty());
    let err = mock.status(&account("mock"), &moved).await.unwrap_err();
    assert!(
        matches!(
            err,
            AgentError::Remote {
                status: Some(404),
                ..
            }
        ),
        "{err:?}"
    );
    let err = scheduler
        .delete(&account("mock"), &moved)
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            AgentError::Remote {
                status: Some(404),
                ..
            }
        ),
        "{err:?}"
    );
}

#[tokio::test]
async fn reschedule_outside_window_rejected() {
    let (agent, ctx) = setup();
    let mock = client(&agent, &ctx);
    let first = scheduled(mock.as_ref(), &ctx, TimeDelta::hours(2)).await;
    let scheduler = mock.native_scheduler().unwrap();
    for lead in [TimeDelta::minutes(5), TimeDelta::days(31)] {
        let err = scheduler
            .reschedule(&account("mock"), &first, ctx.clock.now() + lead)
            .await
            .unwrap_err();
        assert_eq!(rule_key(&err), "mock.schedule_outside_window");
    }
    assert_eq!(
        agent.script().posts()[0].publish_at,
        first.remote_publish_at
    );
}

#[tokio::test]
async fn scheduled_post_published_after_clock_passes() {
    let (agent, ctx) = setup();
    let mock = client(&agent, &ctx);
    let receipt = scheduled(mock.as_ref(), &ctx, TimeDelta::hours(2)).await;
    assert_eq!(
        mock.status(&account("mock"), &receipt).await.unwrap(),
        PublishOutcome::Processing(receipt.clone())
    );

    // The same grant, seen later: the clock has passed `remote_publish_at`.
    let mut later = context(TOKEN);
    later.clock = Arc::new(TestClock::new(
        receipt.remote_publish_at.unwrap() + TimeDelta::minutes(1),
    ));
    let mock_later = client(&agent, &later);
    assert_eq!(
        mock_later.status(&account("mock"), &receipt).await.unwrap(),
        PublishOutcome::Published(receipt.clone())
    );
    assert!(agent.script().posts()[0].live);
}

#[tokio::test]
async fn scheduled_post_liveness_agrees_across_status_metrics_and_inbox() {
    let (agent, ctx) = setup();
    let mock = client(&agent, &ctx);
    let at = ctx.clock.now() + TimeDelta::hours(2);
    let mut seeded = variant(None);
    seeded.options.insert("comments".to_owned(), json!(1));
    let posted = receipt(
        mock.publish(
            PublishRequest {
                native_publish_at: Some(at),
                ..request(&seeded, key())
            },
            &Progress(false),
        )
        .await
        .unwrap(),
    );
    let before = mock.metrics().unwrap();
    assert_eq!(
        before
            .post_metrics(&account("mock"), &posted)
            .await
            .unwrap()
            .views,
        Some(0)
    );
    assert!(
        mock.inbox()
            .unwrap()
            .fetch_since(&account("mock"), None)
            .await
            .unwrap()
            .items
            .is_empty()
    );

    // Past its time the post is live for every reader, though nobody asked for its status.
    let mut later = context(TOKEN);
    later.clock = Arc::new(TestClock::new(at + TimeDelta::minutes(1)));
    let mock_later = client(&agent, &later);
    let views = mock_later
        .metrics()
        .unwrap()
        .post_metrics(&account("mock"), &posted)
        .await
        .unwrap()
        .views;
    assert!(views.is_some_and(|v| v > 0), "{views:?}");
    assert_eq!(
        mock_later
            .inbox()
            .unwrap()
            .fetch_since(&account("mock"), None)
            .await
            .unwrap()
            .items
            .len(),
        1
    );
}

#[tokio::test]
async fn inbox_cursor_does_not_skip_comments_of_a_post_that_goes_live_later() {
    let (agent, ctx) = setup();
    let mock = client(&agent, &ctx);
    let at = ctx.clock.now() + TimeDelta::hours(2);
    let mut seeded = variant(None);
    seeded.options.insert("comments".to_owned(), json!(2));
    mock.publish(
        PublishRequest {
            native_publish_at: Some(at),
            ..request(&seeded, key())
        },
        &Progress(false),
    )
    .await
    .unwrap();
    post(mock.as_ref(), 1).await;
    let first = mock
        .inbox()
        .unwrap()
        .fetch_since(&account("mock"), None)
        .await
        .unwrap();
    assert_eq!(first.items.len(), 1, "only the live post's comment");

    let mut later = context(TOKEN);
    later.clock = Arc::new(TestClock::new(at + TimeDelta::minutes(1)));
    let page = client(&agent, &later)
        .inbox()
        .unwrap()
        .fetch_since(&account("mock"), first.cursor.as_deref())
        .await
        .unwrap();
    assert_eq!(page.items.len(), 2, "the scheduled post's seeded comments");
    assert!(page.items.iter().all(|i| i.remote_post_id == "mock-post-1"));
}

#[tokio::test]
async fn reschedule_and_delete_refuse_a_rejected_token() {
    let (agent, ctx) = setup();
    let first = scheduled(client(&agent, &ctx).as_ref(), &ctx, TimeDelta::hours(2)).await;
    let expired = context("expired");
    let mock = client(&agent, &expired);
    let scheduler = mock.native_scheduler().unwrap();
    let later = expired.clock.now() + TimeDelta::days(2);

    let err = scheduler
        .reschedule(&account("mock"), &first, later)
        .await
        .unwrap_err();
    assert_eq!(err.retry_class(), RetryClass::NeedsReconnect);
    let err = scheduler
        .delete(&account("mock"), &first)
        .await
        .unwrap_err();
    assert_eq!(err.retry_class(), RetryClass::NeedsReconnect);
    let posts = agent.script().posts();
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].publish_at, first.remote_publish_at);
}

fn recipient(address: &str) -> DirectRecipient {
    DirectRecipient {
        send_id: DirectSendId::from_uuid(Uuid::now_v7()),
        address: address.to_owned(),
        merge_fields: BTreeMap::new(),
        locale: "en".to_owned(),
    }
}

fn email() -> Variant {
    Variant {
        target: account("mock_direct"),
        email: Some(EmailDocument {
            subject: "Our autumn sale".to_owned(),
            preheader: None,
            blocks: vec![EmailBlock::Text {
                text: "Everything is 20% off.".to_owned(),
            }],
        }),
        ..variant(None)
    }
}

#[tokio::test]
async fn direct_outcomes_by_address() {
    let agent = MockAgent::direct();
    let ctx = context(TOKEN);
    let mock = client(&agent, &ctx);
    assert!(mock.commenter().is_none() && mock.inbox().is_none());
    let sender = mock.direct().expect("mock_direct sends directly");
    let recipients = [
        recipient("ok@example.com"),
        recipient("soft-bounce@example.com"),
        recipient("bounce@example.com"),
        recipient("fail@example.com"),
    ];
    let results = sender
        .send_batch(&account("mock_direct"), &recipients, &email())
        .await
        .unwrap();
    let ids: Vec<_> = results.iter().map(|r| r.send_id).collect();
    let want: Vec<_> = recipients.iter().map(|r| r.send_id).collect();
    assert_eq!(ids, want);
    assert_eq!(
        results[0].outcome,
        DirectSendOutcome::Sent {
            remote_id: Some(format!("mock-send-{}", recipients[0].send_id)),
        }
    );
    assert!(matches!(
        results[1].outcome,
        DirectSendOutcome::Bounced {
            permanent: false,
            ..
        }
    ));
    assert!(matches!(
        results[2].outcome,
        DirectSendOutcome::Bounced {
            permanent: true,
            ..
        }
    ));
    assert!(matches!(
        results[3].outcome,
        DirectSendOutcome::Failed {
            retryable: true,
            ..
        }
    ));
    for (result, to) in results.iter().zip(&recipients) {
        let shown = format!("{:?}", result.outcome);
        assert!(!shown.contains(&to.address), "{shown}");
        assert!(!shown.contains("example.com"), "{shown}");
    }
    let sends = agent.script().sends();
    assert_eq!(sends.len(), 1);
    assert_eq!(sends[0].send_id, recipients[0].send_id);

    // An email without a subject is refused before anything is sent.
    let mut blank = email();
    blank.email.as_mut().unwrap().subject = "  ".to_owned();
    let err = sender
        .send_batch(&account("mock_direct"), &recipients, &blank)
        .await
        .unwrap_err();
    assert_eq!(rule_key(&err), "mock_direct.email_subject_missing");
    assert_eq!(
        mock.validate(&blank, &[]).await.unwrap().len(),
        1,
        "validate reports the missing subject"
    );
    assert!(mock.validate(&email(), &[]).await.unwrap().is_empty());
}

#[tokio::test]
async fn direct_repeated_send_id_sends_once_and_matches_case_insensitively() {
    let agent = MockAgent::direct();
    let ctx = context(TOKEN);
    let mock = client(&agent, &ctx);
    let sender = mock.direct().unwrap();
    let ok = recipient("ok@example.com");
    let first = sender
        .send_batch(&account("mock_direct"), std::slice::from_ref(&ok), &email())
        .await
        .unwrap();
    let again = sender
        .send_batch(&account("mock_direct"), std::slice::from_ref(&ok), &email())
        .await
        .unwrap();
    assert_eq!(first, again);
    assert_eq!(agent.script().sends().len(), 1);

    // Keywords match in any case, and soft bounces win over hard ones.
    let results = sender
        .send_batch(
            &account("mock_direct"),
            &[
                recipient("SOFT-Bounce@Example.com"),
                recipient("BOUNCE@example.com"),
                recipient("Fail@example.com"),
            ],
            &email(),
        )
        .await
        .unwrap();
    assert!(matches!(
        results[0].outcome,
        DirectSendOutcome::Bounced {
            permanent: false,
            ..
        }
    ));
    assert!(matches!(
        results[1].outcome,
        DirectSendOutcome::Bounced {
            permanent: true,
            ..
        }
    ));
    assert!(matches!(
        results[2].outcome,
        DirectSendOutcome::Failed {
            retryable: true,
            ..
        }
    ));
    assert_eq!(agent.script().sends().len(), 1);
}

#[tokio::test]
async fn direct_rejected_token_fails_the_batch() {
    let agent = MockAgent::direct();
    let ctx = context("expired");
    let mock = client(&agent, &ctx);
    let err = mock
        .direct()
        .unwrap()
        .send_batch(
            &account("mock_direct"),
            &[recipient("ok@example.com")],
            &email(),
        )
        .await
        .unwrap_err();
    assert_eq!(err.retry_class(), RetryClass::NeedsReconnect);
    assert!(agent.script().sends().is_empty());
}

#[tokio::test]
async fn revoke_counts_and_fails() {
    let (agent, ctx) = setup();
    let mock = client(&agent, &ctx);
    let script = agent.script();
    mock.revoke().await.unwrap();
    assert_eq!(script.revoke_calls(), 1);
    script.fail_revoke(true);
    let err = mock.revoke().await.unwrap_err();
    assert_eq!(err.retry_class(), RetryClass::Retryable);
    assert_eq!(script.revoke_calls(), 2);
    script.fail_revoke(false);
    mock.revoke().await.unwrap();
    assert_eq!(script.revoke_calls(), 3);
}

#[test]
fn exposed_capabilities_match_descriptor() {
    use postit_core::{Agent, exposed_capabilities};
    for agent in [MockAgent::broadcast(), MockAgent::direct()] {
        let ctx = context(TOKEN);
        let mock = client(&agent, &ctx);
        assert_eq!(
            exposed_capabilities(mock.as_ref()),
            agent.descriptor().capabilities
        );
    }
}
