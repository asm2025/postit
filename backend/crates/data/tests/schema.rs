use std::collections::BTreeSet;

use postit_core::{DeliveryState, JobStatus, ParkReason, PostKind};
use postit_data::testkit::seed_user;
use postit_data::vault::SEALED_COLUMNS;
use postit_vault::{SealContext, Vault};
use sqlx::PgPool;
use sqlx::error::ErrorKind;
use uuid::Uuid;

const KEY: &[u8] = b"0123456789abcdef0123456789abcdef-test-key";

/// Tables with the full owned-content shape: `id`, `owner_id`, both actor columns, and a
/// unique `(owner_id, id)`.
const OWNED: &[&str] = &[
    "account_grants",
    "connected_accounts",
    "account_tokens",
    "pending_connections",
    "delegations",
    "writing_styles",
    "brand_profiles",
    "target_groups",
    "campaigns",
    "media_assets",
    "posts",
    "recurrences",
    "publish_jobs",
    "deliveries",
    "delivery_attempts",
    "contacts",
    "contact_lists",
    "suppressions",
    "contact_imports",
    "direct_sends",
    "email_events",
    "inbox_items",
    "post_metric_snapshots",
    "account_metrics_daily",
    "ai_providers",
    "ai_generations",
    "platform_quota_usage",
];

/// Link tables: a composite primary key plus `owner_id`.
const LINKS: &[&str] = &[
    "delegation_accounts",
    "target_group_accounts",
    "post_media",
    "post_overrides",
    "post_targets",
    "recurrence_targets",
    "contact_list_members",
];

/// Tables that carry `owner_id` but are not owned content in the standard shape.
const OWNER_COLUMN_EXEMPT: &[&str] = &["audit_events", "idempotency_keys", "notifications"];

fn vault() -> Vault {
    Vault::from_keys([(1, KEY.to_vec())]).unwrap_or_else(|e| unreachable!("vault: {e}"))
}

fn kind(err: &sqlx::Error) -> Option<ErrorKind> {
    err.as_database_error()
        .map(sqlx::error::DatabaseError::kind)
}

async fn run(pool: &PgPool, sql: &'static str, binds: &[Uuid]) -> Result<(), sqlx::Error> {
    let mut query = sqlx::query(sql);
    for b in binds {
        query = query.bind(*b);
    }
    query.execute(pool).await.map(|_| ())
}

async fn run_ok(pool: &PgPool, sql: &'static str, binds: &[Uuid]) {
    run(pool, sql, binds)
        .await
        .unwrap_or_else(|e| unreachable!("{sql}: {e}"));
}

/// One owner with a row in every table the tests need to point at.
struct Seed {
    owner: Uuid,
    grant: Uuid,
    account: Uuid,
    media: Uuid,
    campaign: Uuid,
    post: Uuid,
    job: Uuid,
    contact: Uuid,
    list: Uuid,
    group: Uuid,
}

async fn seed(pool: &PgPool) -> Seed {
    let id = Uuid::now_v7;
    let s = Seed {
        owner: seed_user(pool, "active").await.as_uuid(),
        grant: id(),
        account: id(),
        media: id(),
        campaign: id(),
        post: id(),
        job: id(),
        contact: id(),
        list: id(),
        group: id(),
    };
    run_ok(
        pool,
        "INSERT INTO platforms (id, display_name) VALUES ('test', 'Test') ON CONFLICT DO NOTHING",
        &[],
    )
    .await;
    run_ok(
        pool,
        "INSERT INTO account_grants (id, owner_id, platform_id, kind, credential) \
         VALUES ($1, $2, 'test', 'oauth', '\\x00')",
        &[s.grant, s.owner],
    )
    .await;
    run_ok(
        pool,
        "INSERT INTO connected_accounts \
         (id, owner_id, grant_id, platform_id, remote_account_id, display_name) \
         VALUES ($1, $2, $3, 'test', $1::text, 'Account')",
        &[s.account, s.owner, s.grant],
    )
    .await;
    run_ok(
        pool,
        "INSERT INTO media_assets (id, owner_id, storage_key, mime, status) \
         VALUES ($1, $2, $1::text, 'image/png', 'ready')",
        &[s.media, s.owner],
    )
    .await;
    run_ok(
        pool,
        "INSERT INTO campaigns (id, owner_id, name) VALUES ($1, $2, 'Launch')",
        &[s.campaign, s.owner],
    )
    .await;
    run_ok(
        pool,
        "INSERT INTO posts (id, owner_id, campaign_id, text) VALUES ($1, $2, $3, 'hello')",
        &[s.post, s.owner, s.campaign],
    )
    .await;
    run_ok(
        pool,
        "INSERT INTO publish_jobs (id, owner_id, post_id, post_version, content_snapshot, scheduled_at) \
         VALUES ($1, $2, $3, 1, '{}'::jsonb, now())",
        &[s.job, s.owner, s.post],
    )
    .await;
    run_ok(
        pool,
        "INSERT INTO contacts (id, owner_id, email, email_hash) \
         VALUES ($1, $2, $1::text || '@example.test', '\\x00')",
        &[s.contact, s.owner],
    )
    .await;
    run_ok(
        pool,
        "INSERT INTO contact_lists (id, owner_id, name) VALUES ($1, $2, 'List')",
        &[s.list, s.owner],
    )
    .await;
    run_ok(
        pool,
        "INSERT INTO target_groups (id, owner_id, name) VALUES ($1, $2, 'Group')",
        &[s.group, s.owner],
    )
    .await;
    s
}

async fn insert_delivery(
    pool: &PgPool,
    s: &Seed,
    state: &str,
    parked_reason: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO deliveries \
         (id, owner_id, job_id, post_id, account_id, operation, state, parked_reason, idempotency_key) \
         VALUES ($1, $2, $3, $4, $5, 'publish', $6, $7, $8)",
    )
    .bind(Uuid::now_v7())
    .bind(s.owner)
    .bind(s.job)
    .bind(s.post)
    .bind(s.account)
    .bind(state)
    .bind(parked_reason)
    .bind(Uuid::now_v7().as_bytes().to_vec())
    .execute(pool)
    .await
    .map(|_| ())
}

#[sqlx::test]
async fn sealed_comments_match_catalog(pool: PgPool) {
    let commented: Vec<(String, String)> = sqlx::query_as(
        "SELECT c.relname::text, a.attname::text \
         FROM pg_description d \
         JOIN pg_class c ON c.oid = d.objoid \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum = d.objsubid \
         WHERE d.description = 'postit:sealed' AND d.objsubid > 0 AND n.nspname = 'public'",
    )
    .fetch_all(&pool)
    .await
    .unwrap_or_else(|e| unreachable!("comments: {e}"));

    let commented: BTreeSet<_> = commented.into_iter().collect();
    let catalog: BTreeSet<_> = SEALED_COLUMNS
        .iter()
        .map(|c| (c.table.to_owned(), c.column.to_owned()))
        .collect();
    assert_eq!(commented, catalog);
    assert_eq!(catalog.len(), 9);
}

#[sqlx::test]
async fn sealed_columns_are_bytea(pool: PgPool) {
    for c in SEALED_COLUMNS {
        let udt: Option<String> = sqlx::query_scalar(
            "SELECT udt_name::text FROM information_schema.columns \
             WHERE table_schema = 'public' AND table_name = $1 AND column_name = $2",
        )
        .bind(c.table)
        .bind(c.column)
        .fetch_optional(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("udt: {e}"));
        assert_eq!(udt.as_deref(), Some("bytea"), "{}.{}", c.table, c.column);

        let id: Option<String> = sqlx::query_scalar(
            "SELECT udt_name::text FROM information_schema.columns \
             WHERE table_schema = 'public' AND table_name = $1 AND column_name = $2",
        )
        .bind(c.table)
        .bind(c.id_column)
        .fetch_optional(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("id udt: {e}"));
        assert_eq!(id.as_deref(), Some("uuid"), "{} id column", c.table);
    }
}

#[sqlx::test]
async fn every_owned_table_has_owner_and_actor_columns(pool: PgPool) {
    for table in OWNED {
        let nullable: Vec<(String, String)> = sqlx::query_as(
            "SELECT column_name::text, is_nullable::text FROM information_schema.columns \
             WHERE table_schema = 'public' AND table_name = $1 \
             AND column_name IN ('owner_id', 'created_by', 'updated_by')",
        )
        .bind(*table)
        .fetch_all(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("{table}: {e}"));
        let nullable: std::collections::BTreeMap<_, _> = nullable.into_iter().collect();
        assert_eq!(
            nullable.get("owner_id").map(String::as_str),
            Some("NO"),
            "{table}.owner_id"
        );
        assert_eq!(
            nullable.get("created_by").map(String::as_str),
            Some("YES"),
            "{table}.created_by"
        );
        assert_eq!(
            nullable.get("updated_by").map(String::as_str),
            Some("YES"),
            "{table}.updated_by"
        );

        let unique: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_constraint c \
             WHERE c.conrelid = ('public.' || $1)::regclass AND c.contype = 'u' \
             AND (SELECT array_agg(a.attname::text ORDER BY a.attname) \
                  FROM unnest(c.conkey) k \
                  JOIN pg_attribute a ON a.attrelid = c.conrelid AND a.attnum = k) \
                 = ARRAY['id', 'owner_id']",
        )
        .bind(*table)
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("{table} unique: {e}"));
        assert_eq!(unique, 1, "{table} needs UNIQUE (owner_id, id)");
    }

    for table in LINKS {
        let nullable: Option<String> = sqlx::query_scalar(
            "SELECT is_nullable::text FROM information_schema.columns \
             WHERE table_schema = 'public' AND table_name = $1 AND column_name = 'owner_id'",
        )
        .bind(*table)
        .fetch_optional(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("{table}: {e}"));
        assert_eq!(nullable.as_deref(), Some("NO"), "{table}.owner_id");
    }
}

#[sqlx::test]
async fn no_table_with_an_owner_column_is_left_out(pool: PgPool) {
    let with_owner: Vec<String> = sqlx::query_scalar(
        "SELECT c.table_name::text FROM information_schema.columns c \
         JOIN information_schema.tables t \
           ON t.table_schema = c.table_schema AND t.table_name = c.table_name \
         WHERE c.table_schema = 'public' AND t.table_type = 'BASE TABLE' \
         AND c.column_name = 'owner_id'",
    )
    .fetch_all(&pool)
    .await
    .unwrap_or_else(|e| unreachable!("owner tables: {e}"));

    let known: BTreeSet<&str> = OWNED
        .iter()
        .chain(LINKS)
        .chain(OWNER_COLUMN_EXEMPT)
        .copied()
        .collect();
    let unknown: Vec<_> = with_owner
        .iter()
        .filter(|t| !known.contains(t.as_str()))
        .collect();
    assert!(
        unknown.is_empty(),
        "tables to classify in this test: {unknown:?}"
    );
}

#[sqlx::test]
async fn delivery_state_check_accepts_every_core_state(pool: PgPool) {
    let s = seed(&pool).await;
    let reasons = [
        ParkReason::OwnerDisabled,
        ParkReason::AccountNeedsReconnect,
        ParkReason::PlatformUnavailable,
        ParkReason::PlatformDisabled,
        ParkReason::LivePublishDisabled,
        ParkReason::QuotaExceeded,
        ParkReason::OutcomeUnresolvable,
    ];
    let mut states = vec![
        DeliveryState::Pending,
        DeliveryState::Claimed,
        DeliveryState::InFlight,
        DeliveryState::Processing,
        DeliveryState::OutcomeUnknown,
        DeliveryState::Published,
        DeliveryState::Failed,
        DeliveryState::DeadLetter,
        DeliveryState::Cancelled,
        DeliveryState::Simulated,
    ];
    states.extend(reasons.map(DeliveryState::Parked));

    for state in &states {
        let reason = match state {
            DeliveryState::Parked(r) => Some(r.as_str()),
            _ => None,
        };
        insert_delivery(&pool, &s, state.as_str(), reason)
            .await
            .unwrap_or_else(|e| unreachable!("{state:?}: {e}"));
    }

    let bogus = insert_delivery(&pool, &s, "bogus", None).await;
    assert_eq!(
        bogus.as_ref().err().and_then(kind),
        Some(ErrorKind::CheckViolation)
    );
    let parked_without_reason = insert_delivery(&pool, &s, "parked", None).await;
    assert_eq!(
        parked_without_reason.as_ref().err().and_then(kind),
        Some(ErrorKind::CheckViolation)
    );
    let reason_without_park = insert_delivery(&pool, &s, "pending", Some("quota_exceeded")).await;
    assert_eq!(
        reason_without_park.as_ref().err().and_then(kind),
        Some(ErrorKind::CheckViolation)
    );
    let unknown_reason = insert_delivery(&pool, &s, "parked", Some("because")).await;
    assert_eq!(
        unknown_reason.as_ref().err().and_then(kind),
        Some(ErrorKind::CheckViolation)
    );
}

#[sqlx::test]
async fn job_status_and_post_kind_checks_accept_core_strings(pool: PgPool) {
    let s = seed(&pool).await;
    for status in [
        JobStatus::Scheduled,
        JobStatus::Publishing,
        JobStatus::Published,
        JobStatus::Cancelled,
        JobStatus::Failed,
        JobStatus::Partial,
    ] {
        // Dry runs are exempt from the one-live-job index.
        sqlx::query(
            "INSERT INTO publish_jobs \
             (id, owner_id, post_id, post_version, content_snapshot, scheduled_at, dry_run, status) \
             VALUES ($1, $2, $3, 1, '{}'::jsonb, now(), TRUE, $4)",
        )
        .bind(Uuid::now_v7())
        .bind(s.owner)
        .bind(s.post)
        .bind(status.as_str())
        .execute(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("{status:?}: {e}"));
    }
    for post_kind in [PostKind::Post, PostKind::Comment] {
        sqlx::query("INSERT INTO posts (id, owner_id, kind) VALUES ($1, $2, $3)")
            .bind(Uuid::now_v7())
            .bind(s.owner)
            .bind(post_kind.as_str())
            .execute(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("{post_kind:?}: {e}"));
    }
}

#[sqlx::test]
async fn one_live_job_per_post_ignores_dry_runs(pool: PgPool) {
    let s = seed(&pool).await;
    let second_live = run(
        &pool,
        "INSERT INTO publish_jobs (id, owner_id, post_id, post_version, content_snapshot, scheduled_at) \
         VALUES ($1, $2, $3, 1, '{}'::jsonb, now())",
        &[Uuid::now_v7(), s.owner, s.post],
    )
    .await;
    assert_eq!(
        second_live.err().and_then(|e| kind(&e)),
        Some(ErrorKind::UniqueViolation)
    );

    run_ok(
        &pool,
        "INSERT INTO publish_jobs \
         (id, owner_id, post_id, post_version, content_snapshot, scheduled_at, dry_run) \
         VALUES ($1, $2, $3, 1, '{}'::jsonb, now(), TRUE)",
        &[Uuid::now_v7(), s.owner, s.post],
    )
    .await;
}

#[sqlx::test]
async fn composite_fks_reject_cross_owner_refs(pool: PgPool) {
    let a = seed(&pool).await;
    let b = seed(&pool).await;
    let (n1, n2) = (Uuid::now_v7(), Uuid::now_v7());

    // (name, sql, binds that stay inside owner A, binds that reach into owner B).
    let cases: Vec<(&str, &'static str, [Uuid; 3], [Uuid; 3])> = vec![
        (
            "post_media -> media",
            "INSERT INTO post_media (post_id, media_id, owner_id, position) VALUES ($1, $2, $3, (SELECT count(*)::int FROM post_media))",
            [a.post, a.media, a.owner],
            [a.post, b.media, a.owner],
        ),
        (
            "post_overrides -> account",
            "INSERT INTO post_overrides (post_id, account_id, owner_id) VALUES ($1, $2, $3)",
            [a.post, a.account, a.owner],
            [a.post, b.account, a.owner],
        ),
        (
            "connected_accounts -> grant",
            "INSERT INTO connected_accounts \
             (id, owner_id, grant_id, platform_id, remote_account_id, display_name) \
             VALUES ($1, $2, $3, 'test', $1::text, 'Other')",
            [n1, a.owner, a.grant],
            [n2, a.owner, b.grant],
        ),
        (
            "contact_list_members -> contact",
            "INSERT INTO contact_list_members (list_id, contact_id, owner_id) VALUES ($1, $2, $3)",
            [a.list, a.contact, a.owner],
            [a.list, b.contact, a.owner],
        ),
        (
            "target_group_accounts -> account",
            "INSERT INTO target_group_accounts (group_id, account_id, owner_id) VALUES ($1, $2, $3)",
            [a.group, a.account, a.owner],
            [a.group, b.account, a.owner],
        ),
    ];

    for (name, sql, inside, across) in cases {
        run_ok(&pool, sql, &inside).await;
        let err = run(&pool, sql, &across).await.err();
        assert_eq!(
            err.as_ref().and_then(kind),
            Some(ErrorKind::ForeignKeyViolation),
            "{name}: {err:?}"
        );
    }
}

#[sqlx::test]
async fn nullable_composite_fk_keeps_owner_on_parent_delete(pool: PgPool) {
    let s = seed(&pool).await;

    run_ok(&pool, "DELETE FROM campaigns WHERE id = $1", &[s.campaign]).await;

    let (owner, campaign): (Uuid, Option<Uuid>) =
        sqlx::query_as("SELECT owner_id, campaign_id FROM posts WHERE id = $1")
            .bind(s.post)
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("post: {e}"));
    assert_eq!(owner, s.owner);
    assert_eq!(campaign, None);
}

#[sqlx::test]
async fn deleting_a_user_removes_the_workspace_and_nulls_actor_columns(pool: PgPool) {
    let s = seed(&pool).await;
    let other = seed(&pool).await;
    insert_delivery(&pool, &s, "pending", None)
        .await
        .unwrap_or_else(|e| unreachable!("delivery: {e}"));

    // A delegate authored a post in the owner's workspace, then both are deleted in turn.
    let delegate = seed_user(&pool, "active").await.as_uuid();
    run_ok(
        &pool,
        "UPDATE posts SET created_by = $2, updated_by = $2 WHERE id = $1",
        &[s.post, delegate],
    )
    .await;
    run_ok(&pool, "DELETE FROM users WHERE id = $1", &[delegate]).await;
    let (created_by, owner): (Option<Uuid>, Uuid) =
        sqlx::query_as("SELECT created_by, owner_id FROM posts WHERE id = $1")
            .bind(s.post)
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("post: {e}"));
    assert_eq!((created_by, owner), (None, s.owner));

    run_ok(&pool, "DELETE FROM users WHERE id = $1", &[s.owner]).await;
    for table in OWNED.iter().chain(LINKS) {
        let sql = format!("SELECT count(*) FROM {table} WHERE owner_id = $1");
        let left: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
            .bind(s.owner)
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("{table}: {e}"));
        assert_eq!(left, 0, "{table}");
    }
    let posts: i64 = sqlx::query_scalar("SELECT count(*) FROM posts WHERE owner_id = $1")
        .bind(other.owner)
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("other posts: {e}"));
    assert_eq!(posts, 1);
    let accounts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM connected_accounts WHERE owner_id = $1")
            .bind(other.owner)
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("other accounts: {e}"));
    assert_eq!(accounts, 1);
}

/// `table.column` of every text-like column in the public schema that contains `needle`.
async fn plaintext_hits(pool: &PgPool, needle: &str) -> Vec<String> {
    let columns: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT c.table_name::text, c.column_name::text, c.udt_name::text \
         FROM information_schema.columns c \
         JOIN information_schema.tables t \
           ON t.table_schema = c.table_schema AND t.table_name = c.table_name \
         WHERE c.table_schema = 'public' AND t.table_type = 'BASE TABLE' \
         AND c.udt_name IN ('text', 'varchar', 'bpchar', 'citext', 'jsonb', 'json', 'bytea', '_text')",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_else(|e| unreachable!("columns: {e}"));

    let mut hits = Vec::new();
    for (table, column, udt) in columns {
        let expr = if udt == "bytea" {
            format!("encode(\"{column}\", 'escape')")
        } else {
            format!("\"{column}\"::text")
        };
        let sql = format!("SELECT count(*) FROM \"{table}\" WHERE {expr} LIKE '%{needle}%'");
        let found: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
            .fetch_one(pool)
            .await
            .unwrap_or_else(|e| unreachable!("{table}.{column}: {e}"));
        if found > 0 {
            hits.push(format!("{table}.{column}"));
        }
    }
    hits
}

#[sqlx::test]
async fn plaintext_never_stored(pool: PgPool) {
    let vault = vault();
    let owner = seed_user(&pool, "active").await.as_uuid();
    run_ok(
        &pool,
        "INSERT INTO platforms (id, display_name) VALUES ('test', 'Test')",
        &[],
    )
    .await;

    let fixture =
        |table: &str, column: &str| format!("postit-fixture-secret-{table}-{column}-0123456789");
    let seal = |table: &'static str, column: &'static str, row: Uuid| -> Vec<u8> {
        vault
            .seal(&fixture(table, column), &SealContext { table, column, row })
            .unwrap_or_else(|e| unreachable!("seal {table}.{column}: {e}"))
            .as_bytes()
            .to_vec()
    };

    let grant = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO account_grants (id, owner_id, platform_id, kind, credential) \
         VALUES ($1, $2, 'test', 'oauth', $3)",
    )
    .bind(grant)
    .bind(owner)
    .bind(seal("account_grants", "credential", grant))
    .execute(&pool)
    .await
    .unwrap_or_else(|e| unreachable!("grant: {e}"));

    let account = Uuid::now_v7();
    run_ok(
        &pool,
        "INSERT INTO connected_accounts \
         (id, owner_id, grant_id, platform_id, remote_account_id, display_name) \
         VALUES ($1, $2, $3, 'test', $1::text, 'Account')",
        &[account, owner, grant],
    )
    .await;

    let token = Uuid::now_v7();
    sqlx::query("INSERT INTO account_tokens (id, owner_id, account_id, access_token) VALUES ($1, $2, $3, $4)")
        .bind(token)
        .bind(owner)
        .bind(account)
        .bind(seal("account_tokens", "access_token", token))
        .execute(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("token: {e}"));

    let state = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO oauth_states \
         (id, state_hash, user_id, platform_id, pkce_verifier, grant_tokens, expires_at) \
         VALUES ($1, $2, $3, 'test', $4, $5, now() + interval '10 minutes')",
    )
    .bind(state)
    .bind(state.as_bytes().to_vec())
    .bind(owner)
    .bind(seal("oauth_states", "pkce_verifier", state))
    .bind(seal("oauth_states", "grant_tokens", state))
    .execute(&pool)
    .await
    .unwrap_or_else(|e| unreachable!("oauth state: {e}"));

    let pending = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO pending_connections \
         (id, owner_id, connect_id_hash, platform_id, credential, expires_at) \
         VALUES ($1, $2, $3, 'test', $4, now() + interval '10 minutes')",
    )
    .bind(pending)
    .bind(owner)
    .bind(pending.as_bytes().to_vec())
    .bind(seal("pending_connections", "credential", pending))
    .execute(&pool)
    .await
    .unwrap_or_else(|e| unreachable!("pending: {e}"));

    let provider = Uuid::now_v7();
    sqlx::query("INSERT INTO ai_providers (id, owner_id, provider_key, api_key) VALUES ($1, $2, 'anthropic', $3)")
        .bind(provider)
        .bind(owner)
        .bind(seal("ai_providers", "api_key", provider))
        .execute(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("provider: {e}"));

    let push = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO push_endpoints (id, user_id, endpoint, p256dh, auth, platform) \
         VALUES ($1, $2, $3, $4, $5, 'android')",
    )
    .bind(push)
    .bind(owner)
    .bind(seal("push_endpoints", "endpoint", push))
    .bind(seal("push_endpoints", "p256dh", push))
    .bind(seal("push_endpoints", "auth", push))
    .execute(&pool)
    .await
    .unwrap_or_else(|e| unreachable!("push: {e}"));

    // The scan can see plaintext: a control row with a fixture in the clear is found.
    let control = fixture("control", "kind");
    sqlx::query("INSERT INTO notifications (id, user_id, kind) VALUES ($1, $2, $3)")
        .bind(Uuid::now_v7())
        .bind(owner)
        .bind(&control)
        .execute(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("control: {e}"));
    assert_eq!(
        plaintext_hits(&pool, &control).await,
        ["notifications.kind"]
    );

    for c in SEALED_COLUMNS {
        let hits = plaintext_hits(&pool, &fixture(c.table, c.column)).await;
        assert!(
            hits.is_empty(),
            "{}.{} leaked into {hits:?}",
            c.table,
            c.column
        );
    }
}
