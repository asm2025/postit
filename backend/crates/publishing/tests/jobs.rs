#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use std::collections::BTreeSet;

use common::{env, service};
use postit_config::JobSchedules;
use postit_core::DeliveryId;
use postit_data::publishing::Dispatch;
use postit_jobs::{ConsoleCatalog, JobRegistry, Queue};
use postit_publishing::{
    DirectSendBatch, PollDeliveryStatus, PublishDelivery, ReconcileDelivery, console, register,
};
use sqlx::PgPool;
use uuid::Uuid;

const TYPES: [&str; 6] = [
    "account_preflight",
    "delivery_due_sweep",
    "direct_send_batch",
    "poll_delivery_status",
    "publish_delivery",
    "reconcile_delivery",
];

fn schedules() -> JobSchedules {
    let every = "0 * * * * *".to_owned();
    JobSchedules {
        job_history_purge: every.clone(),
        purge_pending_users: every.clone(),
        audit_retention: every.clone(),
        data_retention: every.clone(),
        media_gc: every.clone(),
        delegation_expiry: every.clone(),
        refresh_credentials: every.clone(),
        oauth_state_purge: every.clone(),
        delivery_due_sweep: every.clone(),
        account_preflight: "0 */10 * * * *".to_owned(),
        notification_digest: every,
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn publishing_jobs_are_registered_with_console_disabled(pool: PgPool) {
    let env = env(&pool).await;
    let schedules = schedules();
    let mut registry = JobRegistry::default();
    register(&mut registry, service(&env), &schedules).unwrap();
    let mut catalog = ConsoleCatalog::default();
    console(&mut catalog, &schedules);

    assert_eq!(registry.job_types(), TYPES);
    let described: BTreeSet<_> = catalog.job_types().into_iter().collect();
    assert_eq!(described, TYPES.into_iter().collect());
    for job_type in TYPES {
        let spec = catalog.get(job_type).unwrap();
        assert_eq!(spec.queue(), Queue::Publishing.as_str(), "{job_type}");
        assert_eq!(
            spec.disabled_reason()
                .map(postit_jobs::DisabledReason::as_str),
            Some("publishing_managed"),
            "{job_type}"
        );
    }
    let mut recurring = catalog.recurring();
    recurring.sort();
    assert_eq!(
        recurring,
        [
            ("account_preflight", "0 */10 * * * *".to_owned()),
            ("delivery_due_sweep", "0 * * * * *".to_owned()),
        ]
    );
}

#[test]
fn payloads_build_from_a_dispatch() {
    let delivery = DeliveryId::from_uuid(Uuid::now_v7());
    let dispatch = Dispatch {
        delivery,
        dispatch_gen: 3,
        run_at: chrono::Utc::now(),
    };

    let publish = PublishDelivery::from(dispatch);
    assert_eq!(publish.delivery, delivery.as_uuid());
    assert_eq!(publish.dispatch_gen, 3);
    assert_eq!(PollDeliveryStatus::from(dispatch).poll, 0);
    assert_eq!(ReconcileDelivery::from(dispatch).attempt, 0);
    assert_eq!(DirectSendBatch::from(dispatch).dispatch_gen, 3);
}
