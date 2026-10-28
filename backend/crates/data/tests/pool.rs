//! `Db::connect` and the migrations against a fresh, empty database, created and dropped by
//! `#[sqlx::test]` on the server `DATABASE_URL` names (credentials come from there too).

use postit_config::{DatabaseSettings, RedactedSecret};
use postit_data::Db;
use sqlx::PgPool;
use url::Url;

/// Settings that reach the `#[sqlx::test]` database `pool` is connected to, with the host,
/// port, and credentials of `DATABASE_URL`.
fn settings(pool: &PgPool) -> DatabaseSettings {
    let raw = std::env::var("DATABASE_URL")
        .unwrap_or_else(|err| unreachable!("DATABASE_URL must name the test server: {err}"));
    let server =
        Url::parse(&raw).unwrap_or_else(|err| unreachable!("DATABASE_URL is not a URL: {err}"));
    let database = pool
        .connect_options()
        .get_database()
        .unwrap_or_else(|| unreachable!("the test pool names its database"))
        .to_owned();
    let mut url = server.clone();
    url.set_path(&database);
    // The settings carry credentials separately; `database.url` must not.
    url.set_username("")
        .and_then(|()| url.set_password(None))
        .unwrap_or_else(|()| unreachable!("a postgres URL can drop its credentials"));
    let decode = |raw: &str| {
        urlencoding::decode(raw)
            .unwrap_or_else(|err| unreachable!("credentials are not valid UTF-8: {err}"))
            .into_owned()
    };
    DatabaseSettings {
        url,
        username: decode(server.username()),
        password: RedactedSecret::from(decode(server.password().unwrap_or_default())),
        max_connections: 5,
    }
}

#[sqlx::test(migrations = false)]
async fn connects_and_runs_migrations_from_empty(pool: PgPool) {
    let tables: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema = 'public'",
    )
    .fetch_one(&pool)
    .await
    .unwrap_or_else(|err| unreachable!("counting tables: {err}"));
    assert_eq!(tables, 0, "the test database starts empty");

    let db = Db::connect(&settings(&pool))
        .await
        .unwrap_or_else(|err| unreachable!("connecting to the test database: {err}"));

    db.run_migrations()
        .await
        .unwrap_or_else(|err| unreachable!("running migrations: {err}"));

    let citext_installed: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_extension WHERE extname = 'citext')")
            .fetch_one(db.pool())
            .await
            .unwrap_or_else(|err| unreachable!("checking citext extension: {err}"));

    assert!(citext_installed);
}

#[sqlx::test(migrations = false)]
async fn running_migrations_twice_is_a_no_op(pool: PgPool) {
    let db = Db::connect(&settings(&pool))
        .await
        .unwrap_or_else(|err| unreachable!("connecting to the test database: {err}"));

    db.run_migrations()
        .await
        .unwrap_or_else(|err| unreachable!("first migration run: {err}"));
    db.run_migrations()
        .await
        .unwrap_or_else(|err| unreachable!("second migration run: {err}"));
}
