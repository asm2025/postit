//! Routes compiled only for tests (`testkit` feature): they exercise middleware and helpers
//! that no plan 02 production route uses yet. Production builds never contain them.

use std::time::Duration;

use axum::Extension;
use axum::Router;
use axum::extract::Query;
use axum::routing::{get, post};

use crate::middleware::Authenticated;
use crate::state::AppState;

static MARKED_HITS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[derive(serde::Deserialize)]
struct Sleep {
    ms: u64,
}

async fn sleep(Query(q): Query<Sleep>) -> &'static str {
    tokio::time::sleep(Duration::from_millis(q.ms)).await;
    "done"
}

async fn echo(body: axum::body::Bytes) -> String {
    body.len().to_string()
}

/// Stands in for an authenticated route: marks the request, counts handler executions.
async fn marked(auth: Option<Extension<Authenticated>>) -> String {
    if let Some(Extension(a)) = auth {
        a.mark();
    }
    let hits = MARKED_HITS.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    hits.to_string()
}

/// How many times the `/marked` handler has run in this process.
#[must_use]
pub fn marked_hits() -> usize {
    MARKED_HITS.load(std::sync::atomic::Ordering::SeqCst)
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/sleep", get(sleep))
        .route("/echo", post(echo))
        .route("/marked", get(marked))
}
