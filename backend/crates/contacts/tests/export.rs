#![allow(clippy::unwrap_used, clippy::panic, clippy::too_many_lines)]

use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use csv_async::{AsyncReaderBuilder, StringRecord};
use emixdb::dto::Pagination;
use futures::TryStreamExt as _;
use postit_config::ContactsSettings;
use postit_contacts::{
    ColumnMap, ContactError, ContactImportJob, ContactService, ContactsDeps, ImportOptions,
    NewContactInput,
};
use postit_core::{ContactListId, SystemClock, SystemIdGenerator};
use postit_data::contacts::{Contact, ContactFilter};
use postit_data::testkit::{callers, seed_user};
use postit_data::{Capability, OwnerScope};
use postit_jobs::JobQueue;
use serde_json::{Value, json};
use sqlx::PgPool;

const KEY: &[u8] = b"contacts-test-key";

fn service(pool: &PgPool) -> ContactService {
    ContactService::new(ContactsDeps {
        pool: pool.clone(),
        clock: Arc::new(SystemClock),
        ids: Arc::new(SystemIdGenerator),
        queue: JobQueue::new(
            pool.clone(),
            Arc::new(SystemIdGenerator),
            Arc::new(SystemClock),
        ),
        signing_key: secrecy::SecretSlice::from(KEY.to_vec()),
        settings: ContactsSettings {
            csv_max_bytes: 4 * 1024 * 1024,
            csv_max_rows: 10_000,
        },
    })
}

async fn owner(pool: &PgPool) -> OwnerScope {
    let owner = seed_user(pool, "active").await;
    sqlx::query(
        "INSERT INTO user_limits (user_id, email_daily_cap, contacts_cap, storage_quota_bytes)
         VALUES ($1, 100, 0, 1000000)",
    )
    .bind(owner.as_uuid())
    .execute(pool)
    .await
    .unwrap();
    OwnerScope::own(owner)
}

/// A JSON object literal as the map a contact's fields are typed as.
fn object(value: Value) -> serde_json::Map<String, Value> {
    match value {
        Value::Object(map) => map,
        other => unreachable!("not an object: {other}"),
    }
}

fn input(email: &str) -> NewContactInput {
    NewContactInput {
        email: email.to_string(),
        first_name: None,
        last_name: None,
        fields: None,
        tags: None,
        locale: None,
        consent_source: "signup form".to_string(),
        consent_at: None,
    }
}

async fn export(svc: &ContactService, scope: &OwnerScope, list: Option<ContactListId>) -> Vec<u8> {
    let chunks: Vec<_> = svc
        .export(scope, list)
        .await
        .unwrap()
        .try_collect()
        .await
        .unwrap();
    chunks.concat()
}

async fn records(csv: &[u8]) -> (StringRecord, Vec<StringRecord>) {
    let mut reader = AsyncReaderBuilder::new().create_reader(csv);
    let header = reader.headers().await.unwrap().clone();
    let rows: Vec<StringRecord> = reader.records().try_collect().await.unwrap();
    (header, rows)
}

async fn contacts(svc: &ContactService, scope: &OwnerScope) -> Vec<Contact> {
    let mut all = svc
        .list(
            scope,
            Pagination {
                page: 1,
                page_size: 100,
            },
            ContactFilter::default(),
        )
        .await
        .unwrap()
        .data;
    all.sort_by(|a, b| a.email.cmp(&b.email));
    all
}

/// The mapping that reads an export back. A `field_<key>` header maps to `<key>`.
fn export_columns(fields: &[&str]) -> ColumnMap {
    let name = |s: &str| Some(s.to_string());
    ColumnMap {
        email: "email".to_string(),
        consent_source: "consent_source".to_string(),
        consent_at: name("consent_at"),
        first_name: name("first_name"),
        last_name: name("last_name"),
        tags: name("tags"),
        locale: name("locale"),
        fields: fields
            .iter()
            .map(|f| {
                (
                    (*f).to_string(),
                    f.strip_prefix("field_").unwrap_or(f).to_string(),
                )
            })
            .collect::<BTreeMap<_, _>>(),
    }
}

async fn reimport(svc: &ContactService, scope: &OwnerScope, csv: &[u8], fields: &[&str]) {
    let started = svc
        .start_import(
            scope,
            ImportOptions {
                attest_consent: true,
                list_id: None,
                columns: export_columns(fields),
            },
            csv,
        )
        .await
        .unwrap();
    svc.run_import(ContactImportJob {
        import: started.id.as_uuid(),
        owner: scope.owner.as_uuid(),
    })
    .await
    .unwrap();
}

#[sqlx::test(migrations = "../data/migrations")]
async fn export_round_trips_through_import(pool: PgPool) {
    let svc = service(&pool);
    let source = owner(&pool).await;
    let consent_at: DateTime<Utc> = "2026-03-04T05:06:07.123456Z".parse().unwrap();
    svc.create(
        &source,
        NewContactInput {
            first_name: Some("Ada".to_string()),
            last_name: Some("Lovelace, \"the\" countess".to_string()),
            fields: Some(object(
                json!({"plan": "pro", "city": "Cairo", "email": "work", "status": "vip"}),
            )),
            tags: Some(vec!["vip".to_string(), "early".to_string()]),
            locale: Some("fr".to_string()),
            consent_at: Some(consent_at),
            ..input("ada@example.com")
        },
    )
    .await
    .unwrap();
    svc.create(&source, input("bob@example.com")).await.unwrap();

    let csv = export(&svc, &source, None).await;
    let (header, rows) = records(&csv).await;
    assert_eq!(
        header.iter().collect::<Vec<_>>(),
        [
            "email",
            "first_name",
            "last_name",
            "tags",
            "locale",
            "consent_source",
            "consent_at",
            "status",
            "city",
            "field_email",
            "plan",
            "field_status"
        ]
    );
    assert_eq!(rows.len(), 2);

    let target = owner(&pool).await;
    reimport(
        &svc,
        &target,
        &csv,
        &["city", "plan", "field_email", "field_status"],
    )
    .await;
    let (before, after) = (contacts(&svc, &source).await, contacts(&svc, &target).await);
    assert_eq!(before.len(), after.len());
    for (a, b) in before.iter().zip(&after) {
        assert_eq!(a.email, b.email);
        assert_eq!(a.first_name, b.first_name);
        assert_eq!(a.last_name, b.last_name);
        assert_eq!(a.fields, b.fields);
        assert_eq!(a.tags, b.tags);
        assert_eq!(a.locale, b.locale);
        assert_eq!(a.consent_source, b.consent_source);
        assert_eq!(a.consent_at, b.consent_at);
    }
    assert_eq!(
        after[0].fields,
        json!({"plan": "pro", "city": "Cairo", "email": "work", "status": "vip"})
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn export_round_trips_formula_cells(pool: PgPool) {
    let svc = service(&pool);
    let source = owner(&pool).await;
    let values = [
        "=1+1", "+1", "-1", "@sum", "\tx", "\rx", "'=quoted", "''+twice", "'plain",
    ];
    for (n, value) in values.iter().enumerate() {
        svc.create(
            &source,
            NewContactInput {
                first_name: Some((*value).to_string()),
                fields: Some(object(json!({ "note": value }))),
                ..input(&format!("c{n}@example.com"))
            },
        )
        .await
        .unwrap();
    }
    let csv = export(&svc, &source, None).await;
    let target = owner(&pool).await;
    reimport(&svc, &target, &csv, &["note"]).await;
    let after = contacts(&svc, &target).await;
    assert_eq!(after.len(), values.len());
    for (n, value) in values.iter().enumerate() {
        assert_eq!(after[n].first_name.as_deref(), Some(*value), "{value:?}");
        assert_eq!(after[n].fields["note"], Value::from(*value), "{value:?}");
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn export_neutralizes_formula_cells(pool: PgPool) {
    let svc = service(&pool);
    let scope = owner(&pool).await;
    for (n, value) in ["=a", "+a", "-a", "@a", "\ta", "\ra", "safe"]
        .into_iter()
        .enumerate()
    {
        svc.create(
            &scope,
            NewContactInput {
                first_name: Some(value.to_string()),
                ..input(&format!("c{n}@example.com"))
            },
        )
        .await
        .unwrap();
    }
    let (_, rows) = records(&export(&svc, &scope, None).await).await;
    let names: Vec<&str> = rows.iter().map(|r| r.get(1).unwrap()).collect();
    assert_eq!(names, ["'=a", "'+a", "'-a", "'@a", "'\ta", "'\ra", "safe"]);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn export_by_list(pool: PgPool) {
    let svc = service(&pool);
    let scope = owner(&pool).await;
    let list = svc.create_list(&scope, "news").await.unwrap();
    let a = svc.create(&scope, input("a@example.com")).await.unwrap();
    svc.create(&scope, input("b@example.com")).await.unwrap();
    svc.add_member(&scope, list.id, &[a.id]).await.unwrap();

    let (_, rows) = records(&export(&svc, &scope, Some(list.id)).await).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get(0), Some("a@example.com"));
    let (_, all) = records(&export(&svc, &scope, None).await).await;
    assert_eq!(all.len(), 2);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn export_pages_past_one_chunk(pool: PgPool) {
    let svc = service(&pool);
    let scope = owner(&pool).await;
    for n in 0..1203 {
        svc.create(&scope, input(&format!("c{n:05}@example.com")))
            .await
            .unwrap();
    }
    let (_, rows) = records(&export(&svc, &scope, None).await).await;
    assert_eq!(rows.len(), 1203);
    let mut emails: Vec<_> = rows.iter().map(|r| r.get(0).unwrap().to_string()).collect();
    emails.sort();
    emails.dedup();
    assert_eq!(emails.len(), 1203);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn export_audits_before_the_first_byte(pool: PgPool) {
    let svc = service(&pool);
    let scope = owner(&pool).await;
    for n in 0..3 {
        svc.create(&scope, input(&format!("c{n}@example.com")))
            .await
            .unwrap();
    }
    // The stream is never polled: the download was dropped.
    let stream = svc.export(&scope, None).await.unwrap();
    let details: Value =
        sqlx::query_scalar("SELECT details FROM audit_events WHERE kind = 'contacts_exported'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(details["count"], 3);
    let text = details.to_string();
    assert!(!text.contains('@'), "{text}");
    drop(stream);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn delegate_export_refused(pool: PgPool) {
    let svc = service(&pool);
    let scope = owner(&pool).await;
    let who = callers(&pool, scope.owner, &[]).await;
    svc.create(&scope, input("a@example.com")).await.unwrap();
    for delegate in [&who.view, &who.edit, &who.publish] {
        let err = svc.export(delegate, None).await.err().unwrap();
        assert!(
            matches!(err, ContactError::Forbidden(Capability::OwnerOnly)),
            "{err:?}"
        );
    }
    let audits: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM audit_events WHERE kind = 'contacts_exported'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(audits, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn export_of_another_owners_list_is_not_found(pool: PgPool) {
    let svc = service(&pool);
    let scope = owner(&pool).await;
    let other = owner(&pool).await;
    let list = svc.create_list(&other, "theirs").await.unwrap();
    let err = svc.export(&scope, Some(list.id)).await.err().unwrap();
    assert!(matches!(err, ContactError::NotFound), "{err:?}");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn export_round_trips_formula_field_keys(pool: PgPool) {
    let svc = service(&pool);
    let source = owner(&pool).await;
    svc.create(
        &source,
        NewContactInput {
            fields: Some(object(json!({"=k": "v"}))),
            ..input("a@example.com")
        },
    )
    .await
    .unwrap();
    let csv = export(&svc, &source, None).await;
    let (header, _) = records(&csv).await;
    assert_eq!(header.get(8), Some("'=k"));

    let target = owner(&pool).await;
    let columns = ColumnMap {
        fields: BTreeMap::from([("=k".to_string(), "=k".to_string())]),
        ..export_columns(&[])
    };
    let started = svc
        .start_import(
            &target,
            ImportOptions {
                attest_consent: true,
                list_id: None,
                columns,
            },
            &csv[..],
        )
        .await
        .unwrap();
    svc.run_import(ContactImportJob {
        import: started.id.as_uuid(),
        owner: target.owner.as_uuid(),
    })
    .await
    .unwrap();
    assert_eq!(contacts(&svc, &target).await[0].fields, json!({"=k": "v"}));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn non_string_custom_values_export_as_text(pool: PgPool) {
    let svc = service(&pool);
    let scope = owner(&pool).await;
    svc.create(
        &scope,
        NewContactInput {
            fields: Some(object(json!({"n": 5, "neg": -1, "no": null, "yes": true}))),
            ..input("a@example.com")
        },
    )
    .await
    .unwrap();
    let (header, rows) = records(&export(&svc, &scope, None).await).await;
    let cell = |name: &str| {
        let at = header.iter().position(|h| h == name).unwrap();
        rows[0].get(at).unwrap().to_string()
    };
    assert_eq!(cell("n"), "5");
    assert_eq!(cell("neg"), "'-1");
    assert_eq!(cell("no"), "");
    assert_eq!(cell("yes"), "true");
}
