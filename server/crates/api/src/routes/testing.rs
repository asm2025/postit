//! Routes compiled only for tests (`testkit` feature): they exercise middleware and helpers
//! that no plan 02 production route uses yet. Production builds never contain them.

use std::time::Duration;

use axum::Router;
use axum::extract::Query;
use axum::routing::{get, post};

use crate::state::AppState;

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

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/sleep", get(sleep))
        .route("/echo", post(echo))
}
