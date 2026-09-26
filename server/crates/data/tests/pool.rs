use postit_config::{DatabaseSettings, RedactedSecret};
use postit_data::Db;
use url::Url;

fn settings() -> DatabaseSettings {
    DatabaseSettings {
        url: Url::parse("postgres://localhost:5432/postit")
            .unwrap_or_else(|err| unreachable!("hardcoded valid url: {err}")),
        username: "postgres".to_string(),
        password: RedactedSecret::from("P@$$w0rd".to_string()),
        max_connections: 5,
    }
}

#[tokio::test]
async fn connects_and_runs_migrations_from_empty() {
    let db = Db::connect(&settings())
        .await
        .unwrap_or_else(|err| unreachable!("connecting to local test database: {err}"));

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

#[tokio::test]
async fn running_migrations_twice_is_a_no_op() {
    let db = Db::connect(&settings())
        .await
        .unwrap_or_else(|err| unreachable!("connecting to local test database: {err}"));

    db.run_migrations()
        .await
        .unwrap_or_else(|err| unreachable!("first migration run: {err}"));
    db.run_migrations()
        .await
        .unwrap_or_else(|err| unreachable!("second migration run: {err}"));
}
