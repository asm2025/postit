//! HTTP route handlers.

pub mod accounts;
pub mod audit;
pub mod auth;
pub mod jobs;
pub mod me;
pub mod media;
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
        .route("/media", get(media::list))
        .route("/media/usage", get(media::usage))
        .route("/media/uploads", post(media::start_session))
        .route("/media/{id}", get(media::get).delete(media::delete))
        .route("/media/{id}/view-url", get(media::view_url))
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
    let router = Router::new()
        .merge(import)
        .merge(media_streaming_router(settings));
    #[cfg(any(test, feature = "testkit"))]
    let router = router.nest("/_test/raised", testing::raised_router(settings));
    router
}

/// The signed public media routes, outside `/api/v1`: no bearer token, the signature in the
/// query is the authorization. They stream, so they carry idle timeouts instead of an overall one.
pub fn public_media_router(settings: &crate::settings::ApiSettings) -> Router<AppState> {
    crate::router::with_streaming_limits(
        Router::new()
            .route("/public/media/{id}", get(media::public_content))
            .route("/public/media/{id}/thumbnail", get(media::public_thumbnail)),
        0,
        settings.media_idle_timeout,
    )
}

/// The media routes that stream bodies or run long, each group under its own limits: the
/// single-request upload, chunks, downloads, and the completion request. The small JSON media
/// routes live in [`v1_router`].
fn media_streaming_router(settings: &crate::settings::ApiSettings) -> Router<AppState> {
    use crate::router::{with_limits, with_streaming_limits};
    use axum::routing::put;

    let idle = settings.media_idle_timeout;
    let upload = with_streaming_limits(
        Router::new().route("/media", post(media::upload)),
        settings.media_single_request_max,
        idle,
    );
    let chunks = with_streaming_limits(
        Router::new().route(
            "/media/uploads/{id}",
            put(media::append).head(media::session_head),
        ),
        settings.media_chunk_max,
        idle,
    );
    let downloads = with_streaming_limits(
        Router::new()
            .route("/media/{id}/content", get(media::content))
            .route("/media/{id}/thumbnail", get(media::thumbnail)),
        0,
        idle,
    );
    let complete = with_limits(
        Router::new().route("/media/uploads/{id}/complete", post(media::complete)),
        settings.body_limit,
        settings.media_complete_timeout,
    );
    Router::new()
        .merge(upload)
        .merge(chunks)
        .merge(downloads)
        .merge(complete)
}
