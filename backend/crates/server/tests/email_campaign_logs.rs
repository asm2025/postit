//! No recipient address reaches a log line during an email campaign, whatever the relay
//! answers. The log capture is thread-local and a callsite's interest is cached process-wide, so
//! this test lives in its own binary, on `sqlx::test`'s current-thread runtime.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod campaign;

use campaign::*;
use postit_http::testkit::leak::{LogCapture, find_leak};
use sqlx::PgPool;

#[sqlx::test(migrations = "../data/migrations")]
async fn no_address_in_logs(pool: PgPool) {
    let (capture, guard) = LogCapture::install();
    let fx = fixture(
        &pool,
        Options {
            script: echoing(),
            ..Options::default()
        },
    )
    .await;
    leaky_campaigns(&fx).await;
    drop(guard);

    assert_eq!(
        fx.sink.received().len(),
        1,
        "the one clean address got its mail"
    );
    let log = capture.text();
    assert!(!log.is_empty(), "the capture saw the run");
    let secrets = leaky_secrets();
    let secrets: Vec<&str> = secrets.iter().map(String::as_str).collect();
    if let Some(leak) = find_leak(&secrets, &[("the captured log", log.as_str())]) {
        panic!("{leak}");
    }
}
