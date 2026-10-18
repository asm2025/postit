//! The `vault_rotate` job: moves every sealed value to the vault's newest key version.
//!
//! An operator adds a key version to `vault.keys`, restarts, and enqueues this job. It holds
//! the rotation lock inside [`reseal`], so two runs never overlap, and it is idempotent: a
//! retry finds the finished rows already on the newest version and reseals nothing.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use postit_core::{AuditEventId, IdGenerator, UserId};
use postit_data::DataError;
use postit_data::audit::{AuditEvent, AuditEventKind, AuditLog};
use postit_data::reseal::{ResealReport, reseal};
use postit_data::vault::{SEALED_COLUMNS, SealedColumn, key_versions_in_use};
use postit_jobs::{
    ConsoleCatalog, ConsoleSpec, Job, JobContext, JobError, JobRegistry, Queue, RetryPolicy,
};
use postit_vault::Vault;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

/// Rows read and rewritten per batch.
const BATCH_SIZE: u32 = 500;

/// Payload of `vault_rotate`: who asked for the run, nothing else.
#[derive(Debug, Serialize, Deserialize)]
pub struct VaultRotate {
    /// The admin who requested the rotation.
    pub requested_by_user_id: Uuid,
}

impl Job for VaultRotate {
    const JOB_TYPE: &'static str = "vault_rotate";
    const QUEUE: Queue = Queue::Maintenance;
}

#[derive(Clone)]
struct Handler {
    pool: PgPool,
    vault: Vault,
    ids: Arc<dyn IdGenerator>,
    columns: &'static [SealedColumn],
    batch_size: u32,
}

fn failure(err: &DataError) -> JobError {
    // Fixed text only: the error's own message could carry a value.
    match err {
        DataError::RotationInProgress => {
            JobError::Retry("another vault rotation is running".into())
        }
        DataError::Sql(_) => JobError::Retry("database error during vault rotation".into()),
        DataError::VaultKeyBehind { .. } => {
            JobError::Fatal("the vault holds an older key version than the database".into())
        }
        _ => JobError::Fatal("vault rotation failed".into()),
    }
}

fn report_details(report: &ResealReport) -> Value {
    Value::Array(
        report
            .columns
            .iter()
            .map(|c| {
                serde_json::json!({
                    "column": c.column,
                    "resealed": c.resealed,
                    "skipped": c.skipped,
                })
            })
            .collect(),
    )
}

fn versions_details(versions: &BTreeMap<String, BTreeSet<u32>>) -> Value {
    Value::Object(
        versions
            .iter()
            .map(|(column, set)| {
                (
                    column.clone(),
                    Value::Array(set.iter().map(|v| Value::from(*v)).collect()),
                )
            })
            .collect(),
    )
}

impl Handler {
    /// Writes one audit event, naming the requester as the actor. If the requester has been
    /// deleted since, the event is written without an actor rather than failing the job.
    async fn audit(
        &self,
        kind: AuditEventKind,
        requester: UserId,
        details: Vec<(&'static str, Value)>,
    ) -> Result<(), DataError> {
        let build = |actor: Option<UserId>| -> Result<AuditEvent, DataError> {
            let mut event = AuditEvent::new(kind);
            if let Some(actor) = actor {
                event = event.actor(actor);
            }
            for (key, value) in &details {
                event = event
                    .detail(key, value.clone())
                    .map_err(|_| DataError::Conflict("invalid audit detail".into()))?;
            }
            Ok(event)
        };
        let mut conn = self.pool.acquire().await?;
        let first = AuditLog::record(
            &mut conn,
            AuditEventId::from(self.ids.generate()),
            build(Some(requester))?,
        )
        .await;
        match first {
            Err(DataError::Conflict(_)) => {
                AuditLog::record(
                    &mut conn,
                    AuditEventId::from(self.ids.generate()),
                    build(None)?,
                )
                .await
            }
            other => other,
        }
    }

    async fn handle(&self, job: VaultRotate, _ctx: JobContext) -> Result<(), JobError> {
        let requester = UserId::from(job.requested_by_user_id);
        self.audit(AuditEventKind::VaultRotationStarted, requester, Vec::new())
            .await
            .map_err(|e| failure(&e))?;

        // Run the re-seal on its own task: `reseal` holds a session advisory lock on a pooled
        // connection, and a handler future dropped mid-run (worker shutdown) would hand that
        // connection back to the pool still locked. The task always reaches the unlock.
        let this = self.clone();
        let report = tokio::spawn(async move {
            reseal(&this.pool, &this.vault, this.columns, this.batch_size).await
        })
        .await
        .map_err(|_| JobError::Retry("the vault rotation task stopped".into()))?
        .map_err(|e| failure(&e))?;

        let versions = key_versions_in_use(&self.pool, self.columns)
            .await
            .map_err(|e| failure(&e))?;
        self.audit(
            AuditEventKind::VaultRotationCompleted,
            requester,
            vec![
                ("columns", report_details(&report)),
                ("versions_in_use", versions_details(&versions)),
            ],
        )
        .await
        .map_err(|e| failure(&e))?;
        Ok(())
    }
}

fn retry_policy() -> RetryPolicy {
    RetryPolicy::Backoff {
        max_attempts: Some(3),
        initial: std::time::Duration::from_secs(60),
        max: std::time::Duration::from_secs(600),
    }
}

fn register_with(registry: &mut JobRegistry, handler: Handler) -> anyhow::Result<()> {
    registry.register(retry_policy(), move |job: VaultRotate, ctx: JobContext| {
        let handler = handler.clone();
        async move { handler.handle(job, ctx).await }
    })?;
    Ok(())
}

/// Registers `vault_rotate` over the production sealed-column catalog.
///
/// # Errors
///
/// Fails if `vault_rotate` is already registered.
pub fn register(
    registry: &mut JobRegistry,
    pool: PgPool,
    vault: Vault,
    ids: Arc<dyn IdGenerator>,
) -> anyhow::Result<()> {
    register_with(
        registry,
        Handler {
            pool,
            vault,
            ids,
            columns: SEALED_COLUMNS,
            batch_size: BATCH_SIZE,
        },
    )
}

/// Describes `vault_rotate` to the admin console: the requester, retry allowed, no schedule.
pub fn console(catalog: &mut ConsoleCatalog) {
    catalog.add::<VaultRotate>(ConsoleSpec::new().id_field("requested_by_user_id", "user_id"));
}

#[cfg(test)]
mod tests {
    use postit_core::SystemIdGenerator;
    use postit_jobs::JobId;
    use postit_vault::{SealContext, Sealed};

    use super::*;

    const SCRATCH: &[SealedColumn] = &[SealedColumn {
        table: "scratch_secrets",
        column: "secret",
        id_column: "id",
    }];

    const KEY: &[u8] = b"0123456789abcdef0123456789abcdef-test-key";

    fn vault(versions: &[u32]) -> Vault {
        Vault::from_keys(versions.iter().map(|v| (*v, KEY.to_vec())))
            .unwrap_or_else(|e| unreachable!("vault: {e}"))
    }

    fn handler(pool: &PgPool, vault: Vault) -> Handler {
        Handler {
            pool: pool.clone(),
            vault,
            ids: Arc::new(SystemIdGenerator),
            columns: SCRATCH,
            batch_size: 2,
        }
    }

    fn ctx() -> JobContext {
        JobContext {
            job_id: JobId(Uuid::now_v7()),
            attempt: 1,
            max_attempts: Some(3),
        }
    }

    async fn user(pool: &PgPool) -> Uuid {
        let id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO users (id, oidc_issuer, oidc_subject, display_name, role, status)
             VALUES ($1, 'https://i.test', $1::text, 'Ada', 'admin', 'active')",
        )
        .bind(id)
        .execute(pool)
        .await
        .unwrap_or_else(|e| unreachable!("insert user: {e}"));
        id
    }

    async fn seed(pool: &PgPool, old: &Vault, count: usize) {
        sqlx::query("CREATE TABLE scratch_secrets (id UUID PRIMARY KEY, secret BYTEA)")
            .execute(pool)
            .await
            .unwrap_or_else(|e| unreachable!("create scratch: {e}"));
        for i in 0..count {
            let id = Uuid::now_v7();
            let sealed = old
                .seal(
                    &format!("v{i}"),
                    &SealContext {
                        table: "scratch_secrets",
                        column: "secret",
                        row: id,
                    },
                )
                .unwrap_or_else(|e| unreachable!("seal: {e}"));
            sqlx::query("INSERT INTO scratch_secrets (id, secret) VALUES ($1, $2)")
                .bind(id)
                .bind(sealed.as_bytes())
                .execute(pool)
                .await
                .unwrap_or_else(|e| unreachable!("insert: {e}"));
        }
    }

    async fn versions(pool: &PgPool) -> BTreeSet<u32> {
        let rows: Vec<Vec<u8>> = sqlx::query_scalar("SELECT secret FROM scratch_secrets")
            .fetch_all(pool)
            .await
            .unwrap_or_else(|e| unreachable!("rows: {e}"));
        rows.into_iter()
            .map(|b| {
                Sealed::<String>::from_bytes(b)
                    .unwrap_or_else(|e| unreachable!("{e}"))
                    .key_version()
            })
            .collect()
    }

    async fn events(pool: &PgPool, kind: &str) -> Vec<Value> {
        sqlx::query_scalar("SELECT details FROM audit_events WHERE kind = $1 ORDER BY at, id")
            .bind(kind)
            .fetch_all(pool)
            .await
            .unwrap_or_else(|e| unreachable!("events: {e}"))
    }

    async fn run(h: &Handler, requester: Uuid) {
        h.handle(
            VaultRotate {
                requested_by_user_id: requester,
            },
            ctx(),
        )
        .await
        .unwrap_or_else(|e| unreachable!("handle: {e}"));
    }

    #[sqlx::test(migrations = "../data/migrations")]
    async fn rotate_job_reseals_and_audits(pool: PgPool) {
        let admin = user(&pool).await;
        seed(&pool, &vault(&[1]), 5).await;
        run(&handler(&pool, vault(&[1, 2])), admin).await;
        assert_eq!(versions(&pool).await, BTreeSet::from([2]));
        assert_eq!(events(&pool, "vault_rotation_started").await.len(), 1);
        let done = events(&pool, "vault_rotation_completed").await;
        assert_eq!(done.len(), 1);
        assert_eq!(done[0]["columns"][0]["column"], "scratch_secrets.secret");
        assert_eq!(done[0]["columns"][0]["resealed"], 5);
        assert_eq!(done[0]["columns"][0]["skipped"], 0);
        assert_eq!(
            done[0]["versions_in_use"]["scratch_secrets.secret"],
            serde_json::json!([2])
        );
        let actor: Option<Uuid> = sqlx::query_scalar(
            "SELECT actor_user_id FROM audit_events WHERE kind = 'vault_rotation_started'",
        )
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("actor: {e}"));
        assert_eq!(actor, Some(admin));
    }

    #[sqlx::test(migrations = "../data/migrations")]
    async fn rotate_job_retry_is_idempotent(pool: PgPool) {
        let admin = user(&pool).await;
        seed(&pool, &vault(&[1]), 3).await;
        let h = handler(&pool, vault(&[1, 2]));
        run(&h, admin).await;
        run(&h, admin).await;
        let done = events(&pool, "vault_rotation_completed").await;
        assert_eq!(done.len(), 2);
        assert_eq!(done[0]["columns"][0]["resealed"], 3);
        assert_eq!(done[1]["columns"][0]["resealed"], 0);
    }

    #[sqlx::test(migrations = "../data/migrations")]
    async fn rotate_job_survives_a_deleted_requester(pool: PgPool) {
        seed(&pool, &vault(&[1]), 1).await;
        run(&handler(&pool, vault(&[1, 2])), Uuid::now_v7()).await;
        assert_eq!(events(&pool, "vault_rotation_completed").await.len(), 1);
    }

    #[test]
    fn console_spec_allows_retry_and_has_no_schedule() {
        let mut catalog = ConsoleCatalog::default();
        console(&mut catalog);
        let spec = catalog
            .get("vault_rotate")
            .unwrap_or_else(|| unreachable!("spec registered"));
        assert!(spec.allowed().retry);
        assert!(spec.schedule().is_none());
        assert_eq!(spec.queue(), "maintenance");
        let id = Uuid::now_v7();
        let summary = spec.summarize(&serde_json::json!({ "requested_by_user_id": id }));
        assert_eq!(summary.0.get("user_id"), Some(&id.to_string()));
    }
}
