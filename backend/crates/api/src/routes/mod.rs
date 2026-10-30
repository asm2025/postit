//! HTTP route handlers.

pub mod accounts;
pub mod audit;
pub mod auth;
pub mod delegations;
pub mod jobs;
pub mod me;
pub mod media;
pub mod notifications;
pub mod platforms;
pub mod probes;
pub mod push;
#[cfg(any(test, feature = "testkit"))]
pub mod testing;
pub mod users;

use axum::Router;
use axum::routing::{delete, get, patch, post};

use crate::middleware::reject_act_as;
use crate::state::AppState;

/// The `/api/v1` routes.
pub fn v1_router() -> Router<AppState> {
    let router = Router::new()
        .route(
            "/auth/config",
            get(auth::config).route_layer(axum::middleware::from_fn(reject_act_as)),
        )
        .route("/me", get(me::get_me).delete(me::delete_me))
        .route(
            "/me/preferences",
            get(me::get_preferences).put(me::put_preferences),
        )
        .route("/me/workspaces", get(me::workspaces))
        .route("/me/push-endpoints", post(push::register))
        .route("/me/push-endpoints/{id}", delete(push::delete))
        .route("/push/vapid-public-key", get(push::vapid_public_key))
        .route("/notifications", get(notifications::list))
        .route("/notifications/read", post(notifications::mark_read))
        .route(
            "/notifications/unread-count",
            get(notifications::unread_count),
        )
        .route(
            "/delegations",
            get(delegations::list).post(delegations::grant),
        )
        .route(
            "/delegations/{id}",
            patch(delegations::change).delete(delegations::revoke),
        )
        .route("/delegations/received", get(delegations::list_received))
        .route("/delegations/received/{id}", delete(delegations::leave))
        .route("/users/lookup", get(users::lookup))
        .route("/accounts", get(accounts::list))
        .route(
            "/accounts/credentials/{platform}",
            post(accounts::credentials),
        )
        // One route for both: matchit refuses two routes of the same shape with different
        // parameter names. POST takes a platform ID, GET a connect ID.
        .route(
            "/accounts/connect/{id}",
            get(accounts::pending).post(accounts::start_oauth),
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
/// the CSV account import, the media uploads and downloads, the OAuth callback (which outlives
/// the default timeout while the provider answers), and platform events (a fixed body limit),
/// and later AI generation. Tests also get `/_test/raised/*`, which allow 8x the configured
/// limits.
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
        .merge(media_streaming_router(settings))
        .merge(platform_router(settings));
    #[cfg(any(test, feature = "testkit"))]
    let router = router.nest("/_test/raised", testing::raised_router(settings));
    router
}

/// The routes a platform calls, without a bearer token. The callback ignores
/// `X-Postit-Act-As`: a browser following a redirect never sends it, the connection belongs to
/// whoever started it, and the callback always answers with a redirect. Events refuse it like
/// every other route without an auth extractor.
fn platform_router(settings: &crate::settings::ApiSettings) -> Router<AppState> {
    use crate::router::with_limits;

    let callback = with_limits(
        Router::new().route(
            "/platforms/{id}/oauth/callback",
            get(platforms::oauth_callback),
        ),
        settings.body_limit,
        platforms::CALLBACK_TIMEOUT,
    );
    let events = with_limits(
        Router::new()
            .route(
                "/platforms/{id}/events/{kind}",
                post(platforms::platform_event),
            )
            .route_layer(axum::middleware::from_fn(reject_act_as)),
        platforms::EVENT_BODY_LIMIT,
        settings.request_timeout,
    );
    Router::new().merge(callback).merge(events)
}

/// The signed public media routes, outside `/api/v1`: no bearer token, the signature in the
/// query is the authorization. They stream, so they carry idle timeouts instead of an overall one.
pub fn public_media_router(settings: &crate::settings::ApiSettings) -> Router<AppState> {
    crate::router::with_streaming_limits(
        Router::new()
            .route("/public/media/{id}", get(media::public_content))
            .route("/public/media/{id}/thumbnail", get(media::public_thumbnail))
            // No auth extractor here: the header is refused before the signature is checked.
            .route_layer(axum::middleware::from_fn(reject_act_as)),
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
