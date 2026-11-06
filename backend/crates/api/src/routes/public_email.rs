//! The public routes behind the links in sent email: the unsubscribe pages, tracked clicks,
//! and the open pixel. No bearer token: the signed token in the path is the authority, so
//! `POST` needs no CSRF protection. Tokens never expire and need no row, so every link keeps
//! working after its send is purged or its post deleted.
//!
//! Rate limits never break a link. A good token marks the request [`Authenticated`], so it
//! is never charged to its IP; a bad one is. The per-IP bucket only decides whether a click or
//! an open records its event, which bounds `email_events` rows.

use std::time::Duration;

use askama::Template;
use axum::Extension;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use postit_core::{EmailEventKind, EmailLanguage, TEST_SEND_ID};
use postit_data::contacts::{SuppressionReason, SuppressionRepo};
use postit_data::publishing::direct::DirectRepo;
use postit_data::{DataError, OwnerScope};
use postit_i18n::{Args, Locale, t};
use sqlx::Connection as _;

use crate::client_ip::ClientIp;
use crate::email_links::Unsubscribe;
use crate::error::ApiError;
use crate::json::ApiPath;
use crate::middleware::Authenticated;
use crate::state::AppState;

/// The policy of every page here: no script, no image, no frame, no form to another origin.
pub const PAGE_CSP: &str =
    "default-src 'none'; style-src 'unsafe-inline'; form-action 'self'; frame-ancestors 'none'";

/// How long a click or an open may wait on the database. A stalled pool loses the event, never
/// the recipient's link.
const RECORD_TIMEOUT: Duration = Duration::from_secs(2);

/// A transparent 1x1 GIF.
const PIXEL: &[u8] = &[
    0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x01, 0x00, 0x01, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00,
    0xff, 0xff, 0xff, 0x21, 0xf9, 0x04, 0x01, 0x00, 0x00, 0x00, 0x00, 0x2c, 0x00, 0x00, 0x00, 0x00,
    0x01, 0x00, 0x01, 0x00, 0x00, 0x02, 0x02, 0x44, 0x01, 0x00, 0x3b,
];

#[derive(Template)]
#[template(path = "unsubscribe.html")]
struct ConfirmPage<'a> {
    lang: &'a str,
    dir: &'a str,
    title: String,
    question: String,
    explain: String,
    button: String,
}

/// Declares the pages that show a title and one paragraph. Their templates are separate files
/// (the plan names each), so each gets its own type; this keeps their fields in one place.
macro_rules! message_pages {
    ($($(#[$doc:meta])* $name:ident => $path:literal),* $(,)?) => {$(
        $(#[$doc])*
        #[derive(Template)]
        #[template(path = $path)]
        struct $name<'a> {
            lang: &'a str,
            dir: &'a str,
            title: String,
            body: String,
        }

        impl $name<'_> {
            /// Renders the page in `l` from two message IDs.
            fn render_in(l: Locale, title: &str, body: &str) -> Result<String, askama::Error> {
                $name {
                    lang: l.tag(),
                    dir: l.dir(),
                    title: msg(l, title),
                    body: msg(l, body),
                }
                .render()
            }
        }
    )*};
}

message_pages! {
    /// The address left the list.
    DonePage => "unsubscribed.html",
    /// The link came from a test send.
    TestPage => "unsubscribe_test.html",
    /// The token did not verify.
    InvalidPage => "link_invalid.html",
}

fn locale(language: EmailLanguage) -> Locale {
    match language {
        EmailLanguage::EnUs => Locale::EnUs,
    }
}

fn msg(locale: Locale, id: &str) -> String {
    t(locale, id, &Args::new())
}

/// A page with the headers every page here carries.
fn page(status: StatusCode, rendered: Result<String, askama::Error>) -> Response {
    let html = match rendered {
        Ok(html) => html,
        Err(err) => return ApiError::internal(&err).into_response(),
    };
    let mut response = (status, html).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(PAGE_CSP),
    );
    no_store(headers);
    response
}

fn no_store(headers: &mut HeaderMap) {
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
}

/// The `404` page for a forged, damaged, or wrong-kind token. It has no language of its own,
/// so the browser's is used.
fn invalid(headers: &HeaderMap) -> Response {
    let accept = headers
        .get(header::ACCEPT_LANGUAGE)
        .and_then(|v| v.to_str().ok());
    let l = postit_i18n::negotiate(accept);
    page(
        StatusCode::NOT_FOUND,
        InvalidPage::render_in(l, "link-invalid-title", "link-invalid-body"),
    )
}

fn test_page(l: Locale) -> Response {
    page(
        StatusCode::OK,
        TestPage::render_in(l, "unsubscribe-test-title", "unsubscribe-test-body"),
    )
}

/// Marks a request whose token verified, so its IP is not charged for it.
fn mark(auth: Option<&Extension<Authenticated>>) {
    if let Some(Extension(auth)) = auth {
        auth.mark();
    }
}

/// The verified unsubscribe token, with the request marked.
fn verify_unsubscribe(
    state: &AppState,
    auth: Option<&Extension<Authenticated>>,
    token: &str,
) -> Option<Unsubscribe> {
    let found = state.email_links.verify_unsubscribe(token)?;
    mark(auth);
    Some(found)
}

/// `GET /public/u/{token}`: the confirm page in the token's language. It changes nothing,
/// because mail scanners fetch links nobody clicked.
pub async fn unsubscribe_page(
    State(state): State<AppState>,
    auth: Option<Extension<Authenticated>>,
    headers: HeaderMap,
    ApiPath(token): ApiPath<String>,
) -> Response {
    let Some(found) = verify_unsubscribe(&state, auth.as_ref(), &token) else {
        return invalid(&headers);
    };
    let l = locale(found.language);
    if found.send == TEST_SEND_ID {
        return test_page(l);
    }
    page(
        StatusCode::OK,
        ConfirmPage {
            lang: l.tag(),
            dir: l.dir(),
            title: msg(l, "unsubscribe-title"),
            question: msg(l, "unsubscribe-question"),
            explain: msg(l, "unsubscribe-explain"),
            button: msg(l, "unsubscribe-button"),
        }
        .render(),
    )
}

/// `POST /public/u/{token}`: unsubscribes. The confirm page's button and RFC 8058 one-click
/// both post here; the body is never read. A test send's token suppresses nothing.
///
/// # Errors
///
/// `500 internal` on a database failure other than the owner being gone.
pub async fn unsubscribe(
    State(state): State<AppState>,
    auth: Option<Extension<Authenticated>>,
    headers: HeaderMap,
    ApiPath(token): ApiPath<String>,
) -> Result<Response, ApiError> {
    let Some(found) = verify_unsubscribe(&state, auth.as_ref(), &token) else {
        return Ok(invalid(&headers));
    };
    let l = locale(found.language);
    if found.send == TEST_SEND_ID {
        return Ok(test_page(l));
    }
    match suppress(&state, &found).await {
        Ok(()) => {}
        // The owner's account is gone, and with it the list the address left.
        Err(DataError::Sql(sqlx::Error::Database(db))) if db.is_foreign_key_violation() => {}
        Err(err) => return Err(err.into()),
    }
    Ok(page(
        StatusCode::OK,
        DonePage::render_in(l, "unsubscribed-title", "unsubscribed-body"),
    ))
}

/// Records the unsubscribe on its send, when the send still exists, and suppresses the address
/// whether or not it does. Both commit together; repeating either changes nothing.
async fn suppress(state: &AppState, found: &Unsubscribe) -> Result<(), DataError> {
    let mut conn = state.pool.acquire().await?;
    let mut tx = conn.begin().await?;
    DirectRepo::record_event(
        &mut tx,
        found.owner,
        found.send,
        EmailEventKind::Unsubscribed,
        None,
        state.clock.now(),
    )
    .await?;
    SuppressionRepo::add_hash(
        &mut tx,
        &OwnerScope::own(found.owner),
        found.hash,
        SuppressionReason::Unsubscribed,
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Records a click or an open while the send exists. A failure, or a database too slow to
/// answer within [`RECORD_TIMEOUT`], is logged and the link still answers.
async fn record(state: &AppState, send: postit_core::DirectSendId, kind: EmailEventKind) {
    let recording = async {
        let mut conn = state.pool.acquire().await?;
        if let Some(owner) = DirectRepo::send_owner(&mut conn, send).await? {
            DirectRepo::record_event(&mut conn, owner, send, kind, None, state.clock.now()).await?;
        }
        Ok::<(), DataError>(())
    };
    match tokio::time::timeout(RECORD_TIMEOUT, recording).await {
        Ok(Ok(())) => {}
        Ok(Err(err)) => {
            tracing::warn!(error = %err, kind = kind.as_str(), "an email event was not recorded");
        }
        Err(_) => tracing::warn!(kind = kind.as_str(), "an email event timed out unrecorded"),
    }
}

/// Whether this request may record an event: the per-IP bucket bounds event rows.
fn may_record(state: &AppState, ip: Option<&Extension<ClientIp>>) -> bool {
    ip.is_some_and(|Extension(ClientIp(ip))| state.limits.ip.check(ip).is_ok())
}

/// `GET /public/t/c/{token}`: a `303` to the destination signed into the token, and nowhere
/// else, so the route is no open redirect.
pub async fn click(
    State(state): State<AppState>,
    auth: Option<Extension<Authenticated>>,
    ip: Option<Extension<ClientIp>>,
    headers: HeaderMap,
    ApiPath(token): ApiPath<String>,
) -> Response {
    let Some(found) = state.email_links.verify_click(&token) else {
        return invalid(&headers);
    };
    mark(auth.as_ref());
    if may_record(&state, ip.as_ref()) {
        record(&state, found.send, EmailEventKind::Clicked).await;
    }
    let Ok(location) = HeaderValue::from_str(found.destination.as_str()) else {
        // A parsed `Url` is always ASCII, so this does not happen.
        return invalid(&headers);
    };
    let mut response = StatusCode::SEE_OTHER.into_response();
    response.headers_mut().insert(header::LOCATION, location);
    no_store(response.headers_mut());
    response
}

/// `GET /public/t/o/{token}`: the open pixel, a fixed 1x1 GIF.
pub async fn open(
    State(state): State<AppState>,
    auth: Option<Extension<Authenticated>>,
    ip: Option<Extension<ClientIp>>,
    headers: HeaderMap,
    ApiPath(token): ApiPath<String>,
) -> Response {
    let Some(found) = state.email_links.verify_open(&token) else {
        return invalid(&headers);
    };
    mark(auth.as_ref());
    if may_record(&state, ip.as_ref()) {
        record(&state, found.send, EmailEventKind::Opened).await;
    }
    let mut response = PIXEL.into_response();
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static("image/gif"));
    no_store(response.headers_mut());
    response
}
