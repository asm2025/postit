//! HTTP route handlers.

pub mod accounts;
pub mod audit;
pub mod auth;
pub mod jobs;
pub mod me;
pub mod probes;
#[cfg(any(test, feature = "testkit"))]
pub mod testing;
pub mod users;

use axum::Router;
use axum::routing::{get, post};

use crate::state::AppState;

/// The `/api/v1` routes.
pub fn v1_router() -> Router<AppState> {
    let router = Router::new()
        .route("/auth/config", get(auth::config))
        .route("/me", get(me::get_me).delete(me::delete_me))
        .route(
            "/me/preferences",
            get(me::get_preferences).put(me::put_preferences),
        )
        .route("/accounts", get(accounts::list))
        .route(
            "/accounts/credentials/{platform}",
            post(accounts::credentials),
        )
        .route(
            "/accounts/connect/{connect_id}/confirm",
            post(accounts::confirm),
        )
        .route(
            "/accounts/{id}",
            get(accounts::get)
                .patch(accounts::update)
                .delete(accounts::disconnect),
        )
        .route("/accounts/{id}/check", post(accounts::check))
        .route("/admin/audit", get(audit::list))
        .route("/admin/jobs", get(jobs::list))
        .route("/admin/jobs/stats", get(jobs::stats))
        .route("/admin/jobs/recurring", get(jobs::recurring))
        .route("/admin/jobs/recurring/{name}/trigger", post(jobs::trigger))
        .route("/admin/jobs/{id}", get(jobs::get).delete(jobs::delete))
        .route("/admin/jobs/{id}/retry", post(jobs::retry))
        .route("/admin/jobs/{id}/cancel", post(jobs::cancel))
        .route("/users", get(users::list))
        .route(
            "/users/{id}",
            get(users::get).patch(users::patch).delete(users::delete),
        );
    #[cfg(any(test, feature = "testkit"))]
    let router = router.nest("/_test", testing::router());
    router
}

/// Routes that carry their own body limit and timeout (see [`crate::router::with_limits`]):
/// the CSV account import now, and later uploads, public media and AI generation. Tests also
/// get `/_test/raised/*`, which allow 8x the configured limits.
pub fn v1_custom_limits_router(settings: &crate::settings::ApiSettings) -> Router<AppState> {
    // One byte over the configured size: the handler's own reader trips first and answers
    // with a problem; the layer still refuses a declared `Content-Length` far beyond it.
    let import = crate::router::with_limits(
        Router::new().route("/accounts/import/{platform}", post(accounts::import)),
        settings.csv_max_bytes.saturating_add(1),
        settings.csv_timeout,
    );
    let router = Router::new().merge(import);
    #[cfg(any(test, feature = "testkit"))]
    let router = router.nest("/_test/raised", testing::raised_router(settings));
    router
}
