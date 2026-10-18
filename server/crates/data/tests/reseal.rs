use postit_data::DataError;
use postit_data::locks::VAULT_ROTATE_LOCK_KEY;
use postit_data::reseal::{read_batch, reseal, write_batch};
use postit_data::vault::SealedColumn;
use postit_vault::{SealContext, Sealed, Vault};
use sqlx::PgPool;
use uuid::Uuid;

const KEY: &[u8] = b"0123456789abcdef0123456789abcdef-test-key";

fn vault(versions: &[u32]) -> Vault {
    Vault::from_keys(versions.iter().map(|v| (*v, KEY.to_vec())))
        .unwrap_or_else(|e| unreachable!("vault: {e}"))
}

const SCRATCH: SealedColumn = SealedColumn {
    table: "scratch_secrets",
    column: "secret",
    id_column: "id",
};

fn ctx(id: Uuid) -> SealContext {
    SealContext {
        table: "scratch_secrets",
        column: "secret",
        row: id,
    }
}

async fn scratch(pool: &PgPool) {
    sqlx::query("CREATE TABLE scratch_secrets (id UUID PRIMARY KEY, secret BYTEA)")
        .execute(pool)
        .await
        .unwrap_or_else(|e| unreachable!("create scratch: {e}"));
}

async fn insert(pool: &PgPool, vault: &Vault, value: &str) -> Uuid {
    let id = Uuid::now_v7();
    let sealed = vault
        .seal(&value.to_owned(), &ctx(id))
        .unwrap_or_else(|e| unreachable!("seal: {e}"));
    sqlx::query("INSERT INTO scratch_secrets (id, secret) VALUES ($1, $2)")
        .bind(id)
        .bind(sealed.as_bytes())
        .execute(pool)
        .await
        .unwrap_or_else(|e| unreachable!("insert: {e}"));
    id
}

async fn stored(pool: &PgPool, id: Uuid) -> Vec<u8> {
    sqlx::query_scalar::<_, Option<Vec<u8>>>("SELECT secret FROM scratch_secrets WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap_or_else(|e| unreachable!("stored: {e}"))
        .unwrap_or_default()
}

fn open(vault: &Vault, id: Uuid, bytes: Vec<u8>) -> String {
    let sealed = Sealed::<String>::from_bytes(bytes).unwrap_or_else(|e| unreachable!("{e}"));
    vault
        .open(&sealed, &ctx(id))
        .unwrap_or_else(|e| unreachable!("open: {e}"))
}

fn version(bytes: &[u8]) -> u32 {
    Sealed::<String>::from_bytes(bytes.to_vec())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .key_version()
}

#[sqlx::test]
async fn reseals_every_row_to_newest(pool: PgPool) {
    scratch(&pool).await;
    let old = vault(&[1]);
    let mut ids = Vec::new();
    for i in 0..1_200 {
        ids.push((insert(&pool, &old, &format!("v{i}")).await, format!("v{i}")));
    }
    let both = vault(&[1, 2]);
    let report = reseal(&pool, &both, &[SCRATCH], 500)
        .await
        .unwrap_or_else(|e| unreachable!("reseal: {e}"));
    assert_eq!(report.columns.len(), 1);
    assert_eq!(report.columns[0].column, "scratch_secrets.secret");
    assert_eq!(report.columns[0].scanned, 1_200);
    assert_eq!(report.columns[0].resealed, 1_200);
    assert_eq!(report.columns[0].skipped, 0);
    for (id, value) in ids {
        let bytes = stored(&pool, id).await;
        assert_eq!(version(&bytes), 2);
        assert_eq!(open(&both, id, bytes), value);
    }
}

#[sqlx::test]
async fn nulls_and_newest_rows_are_skipped(pool: PgPool) {
    scratch(&pool).await;
    let both = vault(&[1, 2]);
    sqlx::query("INSERT INTO scratch_secrets (id, secret) VALUES ($1, NULL)")
        .bind(Uuid::now_v7())
        .execute(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("null: {e}"));
    let newest = insert(&pool, &both, "new").await;
    let old = insert(&pool, &vault(&[1]), "old").await;
    let report = reseal(&pool, &both, &[SCRATCH], 10)
        .await
        .unwrap_or_else(|e| unreachable!("reseal: {e}"));
    let c = &report.columns[0];
    assert_eq!((c.scanned, c.resealed, c.skipped), (2, 1, 1));
    assert_eq!(version(&stored(&pool, old).await), 2);
    assert_eq!(version(&stored(&pool, newest).await), 2);
}

#[sqlx::test]
async fn second_run_reseals_nothing(pool: PgPool) {
    scratch(&pool).await;
    insert(&pool, &vault(&[1]), "a").await;
    let both = vault(&[1, 2]);
    reseal(&pool, &both, &[SCRATCH], 10)
        .await
        .unwrap_or_else(|e| unreachable!("first: {e}"));
    let report = reseal(&pool, &both, &[SCRATCH], 10)
        .await
        .unwrap_or_else(|e| unreachable!("second: {e}"));
    assert_eq!(report.columns[0].resealed, 0);
}

#[sqlx::test]
async fn concurrent_update_is_not_overwritten(pool: PgPool) {
    scratch(&pool).await;
    let old = vault(&[1]);
    let a = insert(&pool, &old, "a").await;
    let b = insert(&pool, &old, "b").await;
    let both = vault(&[1, 2]);

    let rows = read_batch(&pool, &SCRATCH, None, 10)
        .await
        .unwrap_or_else(|e| unreachable!("read: {e}"));
    // An application write lands between the read and the write.
    let fresh = both
        .seal(&"app".to_owned(), &ctx(a))
        .unwrap_or_else(|e| unreachable!("seal: {e}"));
    sqlx::query("UPDATE scratch_secrets SET secret = $1 WHERE id = $2")
        .bind(fresh.as_bytes())
        .bind(a)
        .execute(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("app write: {e}"));

    let outcome = write_batch(&pool, &both, &SCRATCH, rows)
        .await
        .unwrap_or_else(|e| unreachable!("write: {e}"));
    assert_eq!(
        (outcome.scanned, outcome.resealed, outcome.skipped),
        (2, 1, 1)
    );
    assert_eq!(open(&both, a, stored(&pool, a).await), "app");
    assert_eq!(open(&both, b, stored(&pool, b).await), "b");
}

#[sqlx::test]
async fn rows_deleted_mid_run_are_skipped(pool: PgPool) {
    scratch(&pool).await;
    let old = vault(&[1]);
    let a = insert(&pool, &old, "a").await;
    insert(&pool, &old, "b").await;
    let both = vault(&[1, 2]);
    let rows = read_batch(&pool, &SCRATCH, None, 10)
        .await
        .unwrap_or_else(|e| unreachable!("read: {e}"));
    sqlx::query("DELETE FROM scratch_secrets WHERE id = $1")
        .bind(a)
        .execute(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("delete: {e}"));
    let outcome = write_batch(&pool, &both, &SCRATCH, rows)
        .await
        .unwrap_or_else(|e| unreachable!("write: {e}"));
    assert_eq!((outcome.resealed, outcome.skipped), (1, 1));
}

#[sqlx::test]
async fn second_concurrent_run_refused(pool: PgPool) {
    scratch(&pool).await;
    let mut holder = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(VAULT_ROTATE_LOCK_KEY)
        .execute(&mut *holder)
        .await
        .unwrap_or_else(|e| unreachable!("lock: {e}"));
    let err = reseal(&pool, &vault(&[1, 2]), &[SCRATCH], 10).await.err();
    assert!(matches!(err, Some(DataError::RotationInProgress)));
    sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(VAULT_ROTATE_LOCK_KEY)
        .execute(&mut *holder)
        .await
        .unwrap_or_else(|e| unreachable!("unlock: {e}"));
    drop(holder);
    reseal(&pool, &vault(&[1, 2]), &[SCRATCH], 10)
        .await
        .unwrap_or_else(|e| unreachable!("after release: {e}"));
}

#[sqlx::test]
async fn vault_behind_state_refused(pool: PgPool) {
    scratch(&pool).await;
    sqlx::query("INSERT INTO vault_state (id, newest_key_version) VALUES (TRUE, 3)")
        .execute(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("state: {e}"));
    let err = reseal(&pool, &vault(&[1, 2]), &[SCRATCH], 10).await.err();
    assert!(matches!(
        err,
        Some(DataError::VaultKeyBehind {
            configured: 2,
            stored: 3
        })
    ));
}
