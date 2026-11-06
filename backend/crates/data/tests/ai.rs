#![allow(clippy::unwrap_used)]

use emixdb::dto::Pagination;
use postit_core::{AccountId, AiGenerationId, AiProviderId, AuditEventId, InboxItemId, UserId};
use postit_data::ai_generations::{
    AiGenerationRepo, GenerationOutcome, NewAiGeneration, prompt_hash,
};
use postit_data::ai_providers::{AiProviderChange, AiProviderRepo, NewAiProvider};
use postit_data::audit::{AuditEvent, AuditEventKind, AuditLog};
use postit_data::inbox_items::InboxItemRepo;
use postit_data::testkit::{callers, grant, seed_user};
use postit_data::{AccountLimit, DataError, Level, OwnerScope, ScopeError};
use postit_vault::{Sealed, Vault};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

fn vault() -> Vault {
    Vault::from_keys([(1, b"0123456789abcdef0123456789abcdef-test-key".to_vec())]).unwrap()
}

fn pid() -> AiProviderId {
    AiProviderId::from_uuid(Uuid::now_v7())
}

fn sealed(id: AiProviderId, key: &str) -> Sealed<Value> {
    vault()
        .seal(&json!({ "apiKey": key }), &AiProviderRepo::seal_context(id))
        .unwrap()
}

fn new_provider(name: &str) -> NewAiProvider {
    let id = pid();
    NewAiProvider {
        id,
        provider_key: "openai".to_owned(),
        config: json!({ "name": name }),
        secret: Some(sealed(id, "sk-test")),
    }
}

#[sqlx::test]
async fn provider_first_is_default_and_one_default_per_owner(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let other = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let mut c = pool.acquire().await.unwrap();

    let first = AiProviderRepo::create(&mut c, &scope, new_provider("one"))
        .await
        .unwrap();
    let second = AiProviderRepo::create(&mut c, &scope, new_provider("two"))
        .await
        .unwrap();
    assert!(first.is_default);
    assert!(!second.is_default);
    let theirs = AiProviderRepo::create(&mut c, &OwnerScope::own(other), new_provider("x"))
        .await
        .unwrap();
    assert!(theirs.is_default);

    let all = AiProviderRepo::list(&mut c, &scope).await.unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all.iter().filter(|p| p.is_default).count(), 1);
    let default = AiProviderRepo::default(&mut c, &scope).await.unwrap();
    assert_eq!(default.unwrap().id, first.id);
    // Another owner's provider is not visible.
    assert!(
        AiProviderRepo::get(&mut c, &scope, theirs.id)
            .await
            .unwrap()
            .is_none()
    );
    // Debug never shows the secret, not even sealed.
    let debug = format!("{first:?}");
    assert!(!debug.contains("sk-test"));
    assert!(!debug.contains("Sealed"), "{debug}");
}

/// The write-side types leave the sealed secret, the prompt, and its hash out of `Debug`.
#[test]
fn write_types_debug_shows_no_secret_or_prompt() {
    let made = new_provider("one");
    let debug = format!("{made:?}");
    assert!(debug.contains("NewAiProvider"), "{debug}");
    assert!(
        !debug.contains("Sealed") && !debug.contains("sk-test"),
        "{debug}"
    );

    let change = AiProviderChange {
        config: Some(json!({ "name": "renamed" })),
        secret: Some(Some(sealed(pid(), "sk-change"))),
    };
    let debug = format!("{change:?}");
    assert!(debug.contains("renamed"), "{debug}");
    assert!(
        !debug.contains("Sealed") && !debug.contains("sk-change"),
        "{debug}"
    );

    let mut row = generation(None, "Announce the secret launch");
    row.prompt = Some("Announce the secret launch".to_owned());
    let hash = row
        .prompt_hash
        .iter()
        .fold(String::new(), |out, b| out + &format!("{b:02x}"));
    let debug = format!("{row:?}");
    assert!(debug.contains("NewAiGeneration"), "{debug}");
    assert!(!debug.contains("secret launch"), "{debug}");
    assert!(!debug.contains("prompt"), "{debug}");
    assert!(!debug.contains(&hash), "{debug}");
    // Nor any of the hash's bytes printed as a decimal array.
    assert!(
        !debug.contains(&format!("{:?}", row.prompt_hash)),
        "{debug}"
    );
}

#[sqlx::test]
async fn concurrent_first_creates_make_one_default(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let mut a = pool.acquire().await.unwrap();
    let mut b = pool.acquire().await.unwrap();
    let (ra, rb) = tokio::join!(
        AiProviderRepo::create(&mut a, &scope, new_provider("a")),
        AiProviderRepo::create(&mut b, &scope, new_provider("b")),
    );
    ra.unwrap();
    rb.unwrap();
    let mut c = pool.acquire().await.unwrap();
    let all = AiProviderRepo::list(&mut c, &scope).await.unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all.iter().filter(|p| p.is_default).count(), 1);
}

#[sqlx::test]
async fn set_default_moves_the_default(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let mut c = pool.acquire().await.unwrap();
    let first = AiProviderRepo::create(&mut c, &scope, new_provider("one"))
        .await
        .unwrap();
    let second = AiProviderRepo::create(&mut c, &scope, new_provider("two"))
        .await
        .unwrap();

    AiProviderRepo::set_default(&mut c, &scope, second.id)
        .await
        .unwrap();
    let all = AiProviderRepo::list(&mut c, &scope).await.unwrap();
    let defaults: Vec<_> = all.iter().filter(|p| p.is_default).map(|p| p.id).collect();
    assert_eq!(defaults, vec![second.id]);
    // Setting the current default again is fine.
    AiProviderRepo::set_default(&mut c, &scope, second.id)
        .await
        .unwrap();
    assert!(matches!(
        AiProviderRepo::set_default(&mut c, &scope, pid()).await,
        Err(DataError::NotFound)
    ));
    // The failed call changed nothing.
    let default = AiProviderRepo::default(&mut c, &scope).await.unwrap();
    assert_eq!(default.unwrap().id, second.id);

    // Deleting the default leaves none; nothing is promoted.
    assert!(
        AiProviderRepo::delete(&mut c, &scope, second.id)
            .await
            .unwrap()
    );
    assert!(
        !AiProviderRepo::delete(&mut c, &scope, second.id)
            .await
            .unwrap()
    );
    assert!(
        AiProviderRepo::default(&mut c, &scope)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        AiProviderRepo::get(&mut c, &scope, first.id)
            .await
            .unwrap()
            .is_some()
    );
}

#[sqlx::test]
async fn provider_update_changes_config_and_secret(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let mut c = pool.acquire().await.unwrap();
    let made = AiProviderRepo::create(&mut c, &scope, new_provider("one"))
        .await
        .unwrap();

    let renamed = AiProviderRepo::update(
        &mut c,
        &scope,
        made.id,
        AiProviderChange {
            config: Some(json!({ "name": "renamed" })),
            secret: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(renamed.config, json!({ "name": "renamed" }));
    assert_eq!(
        renamed.secret.as_ref().unwrap(),
        made.secret.as_ref().unwrap()
    );

    let replaced = AiProviderRepo::update(
        &mut c,
        &scope,
        made.id,
        AiProviderChange {
            config: None,
            secret: Some(Some(sealed(made.id, "sk-new"))),
        },
    )
    .await
    .unwrap();
    let opened: Value = vault()
        .open(
            replaced.secret.as_ref().unwrap(),
            &AiProviderRepo::seal_context(made.id),
        )
        .unwrap();
    assert_eq!(opened, json!({ "apiKey": "sk-new" }));
    assert_eq!(replaced.config, json!({ "name": "renamed" }));

    let cleared = AiProviderRepo::update(
        &mut c,
        &scope,
        made.id,
        AiProviderChange {
            config: None,
            secret: Some(None),
        },
    )
    .await
    .unwrap();
    assert!(cleared.secret.is_none());

    assert!(matches!(
        AiProviderRepo::update(&mut c, &scope, pid(), AiProviderChange::default()).await,
        Err(DataError::NotFound)
    ));
}

#[sqlx::test]
async fn provider_reads_need_edit_and_writes_owner_only(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let made = AiProviderRepo::create(&mut c, &who.owner, new_provider("one"))
        .await
        .unwrap();

    assert!(matches!(
        AiProviderRepo::list(&mut c, &who.view).await,
        Err(DataError::Scope(ScopeError::InsufficientAccess))
    ));
    assert!(matches!(
        AiProviderRepo::get(&mut c, &who.view, made.id).await,
        Err(DataError::Scope(ScopeError::InsufficientAccess))
    ));
    assert_eq!(
        AiProviderRepo::list(&mut c, &who.edit).await.unwrap().len(),
        1
    );
    assert!(
        AiProviderRepo::default(&mut c, &who.edit)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        AiProviderRepo::get(&mut c, &who.edit, made.id)
            .await
            .unwrap()
            .is_some()
    );
    // A stranger sees their own workspace, which is empty.
    assert!(
        AiProviderRepo::list(&mut c, &who.stranger)
            .await
            .unwrap()
            .is_empty()
    );

    for scope in [&who.edit, &who.publish] {
        assert!(matches!(
            AiProviderRepo::create(&mut c, scope, new_provider("no")).await,
            Err(DataError::Scope(ScopeError::OwnerOnly))
        ));
        assert!(matches!(
            AiProviderRepo::update(&mut c, scope, made.id, AiProviderChange::default()).await,
            Err(DataError::Scope(ScopeError::OwnerOnly))
        ));
        assert!(matches!(
            AiProviderRepo::set_default(&mut c, scope, made.id).await,
            Err(DataError::Scope(ScopeError::OwnerOnly))
        ));
        assert!(matches!(
            AiProviderRepo::delete(&mut c, scope, made.id).await,
            Err(DataError::Scope(ScopeError::OwnerOnly))
        ));
    }
}

#[sqlx::test]
async fn provider_secret_bound_to_row(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let mut c = pool.acquire().await.unwrap();
    let a = AiProviderRepo::create(&mut c, &scope, new_provider("a"))
        .await
        .unwrap();
    let b = AiProviderRepo::create(&mut c, &scope, new_provider("b"))
        .await
        .unwrap();

    let own: Value = vault()
        .open(
            a.secret.as_ref().unwrap(),
            &AiProviderRepo::seal_context(a.id),
        )
        .unwrap();
    assert_eq!(own, json!({ "apiKey": "sk-test" }));
    // The secret of A, copied onto B, does not open there.
    let copied = AiProviderRepo::update(
        &mut c,
        &scope,
        b.id,
        AiProviderChange {
            config: None,
            secret: Some(a.secret.clone()),
        },
    )
    .await
    .unwrap();
    assert!(
        vault()
            .open::<Value>(
                copied.secret.as_ref().unwrap(),
                &AiProviderRepo::seal_context(b.id),
            )
            .is_err()
    );
}

fn generation(provider: Option<AiProviderId>, prompt: &str) -> NewAiGeneration {
    NewAiGeneration {
        id: AiGenerationId::from_uuid(Uuid::now_v7()),
        provider_id: provider,
        model: "gpt-test".to_owned(),
        mode: "post".to_owned(),
        prompt_hash: prompt_hash(b"key", UserId::from_uuid(Uuid::nil()), prompt),
        prompt: None,
        input_tokens: Some(10),
        output_tokens: Some(5),
        outcome: GenerationOutcome::Ok,
    }
}

/// The provider was deleted while it answered: the paid generation is still recorded, with no
/// provider, inside the caller's transaction too.
#[sqlx::test]
async fn generation_for_a_gone_provider_is_kept_without_one(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let gone = pid();
    let mut tx = pool.begin().await.unwrap();
    AiGenerationRepo::insert(&mut tx, &scope, generation(Some(gone), "p"))
        .await
        .unwrap();
    // The transaction is still usable after the retried statement.
    AiGenerationRepo::insert(&mut tx, &scope, generation(None, "q"))
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let providers: Vec<Option<Uuid>> =
        sqlx::query_scalar("SELECT provider_id FROM ai_generations WHERE owner_id = $1")
            .bind(owner.as_uuid())
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(providers, [None, None]);
}

#[sqlx::test]
async fn deleting_provider_keeps_generations_with_null_provider(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let mut c = pool.acquire().await.unwrap();
    let made = AiProviderRepo::create(&mut c, &scope, new_provider("gone soon"))
        .await
        .unwrap();
    AiGenerationRepo::insert(&mut c, &scope, generation(Some(made.id), "p"))
        .await
        .unwrap();
    let before = AiGenerationRepo::list(&mut c, &scope, Pagination::default())
        .await
        .unwrap();
    assert_eq!(before.data[0].provider_name.as_deref(), Some("gone soon"));

    assert!(
        AiProviderRepo::delete(&mut c, &scope, made.id)
            .await
            .unwrap()
    );
    let after = AiGenerationRepo::list(&mut c, &scope, Pagination::default())
        .await
        .unwrap();
    assert_eq!(after.total, 1);
    assert_eq!(after.data[0].provider_id, None);
    assert_eq!(after.data[0].provider_name, None);
}

#[sqlx::test]
async fn generation_list_newest_first_with_actor_and_provider_name(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let made = AiProviderRepo::create(&mut c, &who.owner, new_provider("main"))
        .await
        .unwrap();

    let mut ids = Vec::new();
    for i in 0..3 {
        let g = generation(Some(made.id), &format!("prompt {i}"));
        ids.push(g.id);
        // Delegates generate through the owner's workspace; their ID is the actor.
        let scope = if i == 1 { &who.edit } else { &who.owner };
        AiGenerationRepo::insert(&mut c, scope, g).await.unwrap();
    }

    let page = |n| Pagination {
        page: n,
        page_size: 2,
    };
    let first = AiGenerationRepo::list(&mut c, &who.owner, page(1))
        .await
        .unwrap();
    assert_eq!(first.total, 3);
    assert_eq!(
        first.data.iter().map(|g| g.id).collect::<Vec<_>>(),
        vec![ids[2], ids[1]]
    );
    assert_eq!(first.data[0].actor, Some(owner));
    assert_eq!(first.data[1].actor, who.edit.actor.into());
    assert_eq!(first.data[0].provider_name.as_deref(), Some("main"));
    assert_eq!(first.data[0].outcome, GenerationOutcome::Ok);
    assert_eq!(first.data[0].input_tokens, Some(10));

    let second = AiGenerationRepo::list(&mut c, &who.owner, page(2))
        .await
        .unwrap();
    assert_eq!(
        second.data.iter().map(|g| g.id).collect::<Vec<_>>(),
        vec![ids[0]]
    );

    // History is the owner's alone.
    assert!(matches!(
        AiGenerationRepo::list(&mut c, &who.edit, page(1)).await,
        Err(DataError::Scope(ScopeError::OwnerOnly))
    ));
    assert_eq!(
        AiGenerationRepo::list(&mut c, &who.stranger, page(1))
            .await
            .unwrap()
            .total,
        0
    );
    // A viewer cannot record one.
    assert!(matches!(
        AiGenerationRepo::insert(&mut c, &who.view, generation(None, "v")).await,
        Err(DataError::Scope(ScopeError::InsufficientAccess))
    ));
}

#[sqlx::test]
async fn generation_outcome_check_rejects_unknown(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let mut c = pool.acquire().await.unwrap();
    for outcome in [
        GenerationOutcome::Ok,
        GenerationOutcome::InvalidOutput,
        GenerationOutcome::Failed,
    ] {
        let mut g = generation(None, "p");
        g.outcome = outcome;
        AiGenerationRepo::insert(&mut c, &scope, g).await.unwrap();
    }
    let listed = AiGenerationRepo::list(&mut c, &scope, Pagination::default())
        .await
        .unwrap();
    assert_eq!(listed.total, 3);

    let rejected = sqlx::query(
        "INSERT INTO ai_generations (id, owner_id, model, mode, prompt_hash, outcome) \
         VALUES ($1, $2, 'm', 'post', '\\x00', 'bogus')",
    )
    .bind(Uuid::now_v7())
    .bind(owner.as_uuid())
    .execute(&pool)
    .await;
    assert!(rejected.is_err());
}

#[sqlx::test]
async fn generation_links_to_a_post(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let mut c = pool.acquire().await.unwrap();
    let g = generation(None, "p");
    let id = g.id;
    AiGenerationRepo::insert(&mut c, &scope, g).await.unwrap();

    let post = Uuid::now_v7();
    sqlx::query("INSERT INTO posts (id, owner_id) VALUES ($1, $2)")
        .bind(post)
        .bind(owner.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    let post = postit_core::PostId::from_uuid(post);
    assert!(
        AiGenerationRepo::link_post(&mut c, &scope, id, post)
            .await
            .unwrap()
    );
    let missing = AiGenerationId::from_uuid(Uuid::now_v7());
    assert!(
        !AiGenerationRepo::link_post(&mut c, &scope, missing, post)
            .await
            .unwrap()
    );
    let listed = AiGenerationRepo::list(&mut c, &scope, Pagination::default())
        .await
        .unwrap();
    assert_eq!(listed.data[0].post_id, Some(post));
}

async fn account(pool: &PgPool, owner: UserId, remote: &str) -> AccountId {
    sqlx::query(
        "INSERT INTO platforms (id, display_name) VALUES ('mastodon', 'Mastodon') \
         ON CONFLICT DO NOTHING",
    )
    .execute(pool)
    .await
    .unwrap();
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO connected_accounts (id, owner_id, platform_id, remote_account_id, display_name) \
         VALUES ($1, $2, 'mastodon', $3, $3)",
    )
    .bind(id)
    .bind(owner.as_uuid())
    .bind(remote)
    .execute(pool)
    .await
    .unwrap();
    AccountId::from_uuid(id)
}

async fn inbox_item(pool: &PgPool, owner: UserId, account: AccountId, text: &str) -> InboxItemId {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO inbox_items (id, owner_id, account_id, remote_comment_id, author_name, \
                                  author_handle, text, received_at) \
         VALUES ($1, $2, $3, $4, 'Sam', '@sam', $5, now())",
    )
    .bind(id)
    .bind(owner.as_uuid())
    .bind(account.as_uuid())
    .bind(id.to_string())
    .bind(text)
    .execute(pool)
    .await
    .unwrap();
    InboxItemId::from_uuid(id)
}

#[sqlx::test]
async fn inbox_get_respects_account_limit(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let x = account(&pool, owner, "x").await;
    let y = account(&pool, owner, "y").await;
    let on_x = inbox_item(&pool, owner, x, "nice post").await;
    let on_y = inbox_item(&pool, owner, y, "hello").await;
    let delegate = seed_user(&pool, "active").await;
    grant(&pool, owner, delegate, Level::View, AccountLimit::only([x])).await;
    let mut c = pool.acquire().await.unwrap();
    let limited = postit_data::delegations::DelegationRepo::resolve(
        &mut c,
        delegate,
        owner,
        chrono::Utc::now(),
    )
    .await
    .unwrap()
    .unwrap();

    let own = InboxItemRepo::get(&mut c, &OwnerScope::own(owner), on_y)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(own.account_id, y);
    assert_eq!(own.text, "hello");
    assert_eq!(own.author_name.as_deref(), Some("Sam"));
    assert_eq!(own.delivery_id, None);

    let seen = InboxItemRepo::get(&mut c, &limited, on_x)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(seen.text, "nice post");
    assert!(
        InboxItemRepo::get(&mut c, &limited, on_y)
            .await
            .unwrap()
            .is_none()
    );
    // Another owner's scope sees nothing.
    let stranger = OwnerScope::own(seed_user(&pool, "active").await);
    assert!(
        InboxItemRepo::get(&mut c, &stranger, on_x)
            .await
            .unwrap()
            .is_none()
    );
}

#[sqlx::test]
async fn ai_audit_kinds_round_trip(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let mut c = pool.acquire().await.unwrap();
    for (kind, name) in [
        (AuditEventKind::AiProviderCreated, "ai_provider_created"),
        (AuditEventKind::AiProviderUpdated, "ai_provider_updated"),
        (AuditEventKind::AiProviderDeleted, "ai_provider_deleted"),
        (
            AuditEventKind::AiProviderKeyChanged,
            "ai_provider_key_changed",
        ),
    ] {
        assert_eq!(kind.as_str(), name);
        assert_eq!(name.parse::<AuditEventKind>().unwrap(), kind);
        let id = AuditEventId::from_uuid(Uuid::now_v7());
        AuditLog::record(&mut c, id, AuditEvent::new(kind).actor(owner).owner(owner))
            .await
            .unwrap();
        let stored: String = sqlx::query_scalar("SELECT kind FROM audit_events WHERE id = $1")
            .bind(id.as_uuid())
            .fetch_one(&mut *c)
            .await
            .unwrap();
        assert_eq!(stored, name);
    }
}
