//! Static hosting for the React web build (plan 02, "Web client hosting"; P7 spec).

use std::net::SocketAddr;
use std::path::Path;

use anyhow::bail;
use axum::Router;
use axum::extract::{Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::{Next, from_fn};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use postit_config::{Environment, Settings};
use serde_json::json;
use tower::ServiceExt;
use tower_http::services::{ServeDir, ServeFile};

/// What `GET /config.json` returns; the web build reads it before rendering.
#[derive(Debug, Clone)]
pub struct WebConfig {
    pub api_base_url: String,
    pub env: &'static str,
}

/// The web router plus its dedicated address, when `server.web.bind` is set.
pub struct WebHosting {
    pub router: Router,
    pub bind: Option<SocketAddr>,
}

/// Paths the API owns: never answered with the SPA fallback. Keep in step with
/// `postit_api::api_router` (add a prefix here if the API gains a top-level path).
const RESERVED: [&str; 4] = ["/api", "/docs", "/health", "/ready"];

const IMMUTABLE: &str = "public, max-age=31536000, immutable";

fn is_reserved(path: &str) -> bool {
    RESERVED.iter().any(|p| {
        path == *p
            || path
                .strip_prefix(p)
                .is_some_and(|rest| rest.starts_with('/'))
    })
}

async fn config_json(State(cfg): State<WebConfig>) -> Response {
    axum::Json(json!({ "API_BASE_URL": cfg.api_base_url, "ENV": cfg.env })).into_response()
}

async fn reserved_not_found(request: Request, next: Next) -> Response {
    if is_reserved(request.uri().path()) {
        return StatusCode::NOT_FOUND.into_response();
    }
    next.run(request).await
}

async fn cache_control(request: Request, next: Next) -> Response {
    let asset = request.uri().path().starts_with("/assets/");
    let mut response = next.run(request).await;
    let value = if asset && response.status().is_success() {
        IMMUTABLE
    } else {
        "no-cache"
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static(value));
    response
}

/// `/config.json` (generated), `/assets/*` (hashed files, 404 when missing), any other
/// file under `root`, and `index.html` for every other path.
pub fn router(root: &Path, config: WebConfig) -> Router {
    let pages = ServeDir::new(root).fallback(ServeFile::new(root.join("index.html")));
    let app = Router::new()
        .route("/config.json", get(config_json))
        .with_state(config)
        .nest_service("/assets", ServeDir::new(root.join("assets")))
        .fallback_service(pages)
        .layer(from_fn(cache_control))
        .layer(from_fn(reserved_not_found));
    postit_api::router::observed(app)
}

/// The shared API port: reserved prefixes go to the API router untouched (its middleware
/// and problem+json 404s), every other path to the web router, outside the API middleware.
pub fn dispatch(api: Router, web: Router) -> Router {
    Router::new().fallback_service(tower::service_fn(move |request: Request| {
        let target = if is_reserved(request.uri().path()) {
            api.clone()
        } else {
            web.clone()
        };
        async move { target.oneshot(request).await }
    }))
}

/// Builds the web hosting when `server.web.enabled` (roles `all` and `api` only).
///
/// # Errors
///
/// Fails when `server.web.root` is missing (config validation normally catches that first)
/// or has no `index.html`.
pub fn from_settings(env: Environment, settings: &Settings) -> anyhow::Result<Option<WebHosting>> {
    let web = &settings.server.web;
    if !web.enabled {
        return Ok(None);
    }
    let Some(root) = web.root.as_ref() else {
        bail!("server.web.enabled requires server.web.root");
    };
    if !root.join("index.html").is_file() {
        bail!(
            "server.web.root {} has no index.html; build the web app first (pnpm build in web/)",
            root.display()
        );
    }
    let bind = match &web.bind {
        Some(b) => Some(
            b.parse::<SocketAddr>()
                .map_err(|_| anyhow::anyhow!("server.web.bind must be an IP socket address"))?,
        ),
        None => None,
    };
    let api_base_url = web
        .api_base_url
        .as_ref()
        .unwrap_or(&settings.server.public_url)
        .as_str()
        .trim_end_matches('/')
        .to_string();
    let config = WebConfig {
        api_base_url,
        env: env.as_str(),
    };
    Ok(Some(WebHosting {
        router: router(root, config),
        bind,
    }))
}
