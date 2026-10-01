use std::fmt::Write as _;
use std::future::Future;

use axum::Json;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use emixcrypto::HashAlgorithm as _;
use postit_data::idempotency::{
    BeginOutcome, IdempotencyRecord, IdempotencyRepo, IdempotencyState,
};
use postit_data::{DataError, OwnerScope};
use sqlx::PgPool;

use crate::error::{ApiError, ErrorCode};

pub const KEY_TTL: chrono::Duration = chrono::Duration::hours(24);

/// Releases an `in_progress` key unless disarmed. `run`'s future can be dropped mid-`f` —
/// the request timeout, a client disconnect, or a panic — and `complete` itself can fail;
/// without this the key would answer 409 `idempotency_in_progress` until it expires. Drop
/// cannot await, so the delete runs on a spawned task.
struct ReleaseOnDrop {
    pool: PgPool,
    id: uuid::Uuid,
    armed: bool,
}

impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let (pool, id) = (self.pool.clone(), self.id);
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                match pool.acquire().await {
                    Ok(mut conn) => {
                        if let Err(err) = IdempotencyRepo::delete(&mut conn, id).await {
                            tracing::warn!(error = %err, "releasing an idempotency key failed");
                        }
                    }
                    Err(err) => tracing::warn!(error = %err, "releasing an idempotency key failed"),
                }
            });
        }
    }
}

pub struct IdempotencyKey(pub Option<String>);

impl<S: Send + Sync> FromRequestParts<S> for IdempotencyKey {
    type Rejection = ApiError;

    #[allow(clippy::unused_async_trait_impl)] // the trait method is async; nothing here awaits
    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        let Some(value) = parts.headers.get("idempotency-key") else {
            return Ok(Self(None));
        };
        let key = value.to_str().unwrap_or_default();
        let valid = (1..=255).contains(&key.len()) && key.bytes().all(|b| b.is_ascii_graphic());
        if !valid {
            return Err(ApiError::new(ErrorCode::ValidationFailed)
                .with_detail("Idempotency-Key must be 1-255 visible ASCII characters"));
        }
        Ok(Self(Some(key.to_owned())))
    }
}

/// A JSON response that can be stored and replayed.
#[derive(Debug, Clone)]
pub struct StoredResponse {
    pub status: StatusCode,
    pub body: serde_json::Value,
}

impl IntoResponse for StoredResponse {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}

/// Hex SHA-256 over the method, the matched route, and the raw body.
#[must_use]
pub fn request_hash(method: &Method, route: &str, body: &[u8]) -> String {
    let mut input = Vec::with_capacity(method.as_str().len() + route.len() + body.len() + 2);
    input.extend_from_slice(method.as_str().as_bytes());
    input.push(b'\n');
    input.extend_from_slice(route.as_bytes());
    input.push(b'\n');
    input.extend_from_slice(body);
    emixcrypto::Sha256Hash::new()
        .compute_hash_bytes(&input)
        .unwrap_or_default()
        .iter()
        .fold(String::with_capacity(64), |mut hex, b| {
            let _ = write!(hex, "{b:02x}");
            hex
        })
}

/// Runs `f` at most once per `(owner, actor, key)`: plan 01's `Idempotency-Key` contract.
///
/// # Errors
///
/// `idempotency_key_reused` for the same key with another route or body,
/// `idempotency_in_progress` while the first request runs, or `f`'s own error (which frees
/// the key so the client can retry).
pub async fn run<F, Fut>(
    pool: &PgPool,
    scope: &OwnerScope,
    key: Option<&str>,
    route: &str,
    hash: &str,
    f: F,
) -> Result<StoredResponse, ApiError>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<StoredResponse, ApiError>>,
{
    let Some(key) = key else {
        return f().await;
    };
    let record = match begin(pool, scope, key, route, hash).await? {
        Claim::Replay(stored) => return Ok(stored),
        Claim::Run(record) => record,
    };
    let mut guard = ReleaseOnDrop {
        pool: pool.clone(),
        id: record.id,
        armed: true,
    };

    let result = f().await;
    let mut conn = pool.acquire().await.map_err(|e| ApiError::internal(&e))?;
    match &result {
        Ok(response) => {
            IdempotencyRepo::complete(
                &mut conn,
                record.id,
                i16::try_from(response.status.as_u16()).unwrap_or(200),
                response.body.clone(),
            )
            .await?;
        }
        Err(_) => IdempotencyRepo::delete(&mut conn, record.id).await?,
    }
    // Completed or deleted: nothing left to release.
    guard.armed = false;
    result
}

enum Claim {
    /// This request owns the key: run the work.
    Run(IdempotencyRecord),
    /// A completed run with the same route and body: answer with its stored response.
    Replay(StoredResponse),
}

/// Claims `key`, or answers from the existing row. Up to three attempts cover two races: a
/// row that has expired (the daily purge has not run yet) is deleted and the claim retried,
/// and a row deleted between `IdempotencyRepo::begin`'s insert and its select
/// (`DataError::NotFound`: a failed run releasing its key) is retried.
async fn begin(
    pool: &PgPool,
    scope: &OwnerScope,
    key: &str,
    route: &str,
    hash: &str,
) -> Result<Claim, ApiError> {
    let mut conn = pool.acquire().await.map_err(|e| ApiError::internal(&e))?;
    for _ in 0..3 {
        let now = chrono::Utc::now();
        let outcome = IdempotencyRepo::begin(
            &mut conn,
            uuid::Uuid::now_v7(),
            scope.owner,
            scope.actor,
            key,
            route,
            hash,
            now + KEY_TTL,
        )
        .await;
        let existing = match outcome {
            Ok(BeginOutcome::Started(record)) => return Ok(Claim::Run(record)),
            Ok(BeginOutcome::Conflict(existing)) => existing,
            Err(DataError::NotFound) => continue,
            Err(err) => return Err(err.into()),
        };
        if existing.expires_at <= now {
            IdempotencyRepo::delete(&mut conn, existing.id).await?;
            continue;
        }
        if existing.route != route || existing.request_hash != hash {
            return Err(ApiError::new(ErrorCode::IdempotencyKeyReused));
        }
        return match (
            existing.state,
            existing.response_status,
            existing.response_body,
        ) {
            (IdempotencyState::Completed, Some(status), Some(body)) => {
                Ok(Claim::Replay(StoredResponse {
                    status: u16::try_from(status)
                        .ok()
                        .and_then(|s| StatusCode::from_u16(s).ok())
                        .unwrap_or(StatusCode::OK),
                    body,
                }))
            }
            _ => Err(ApiError::new(ErrorCode::IdempotencyInProgress)),
        };
    }
    Err(ApiError::new(ErrorCode::IdempotencyInProgress))
}
