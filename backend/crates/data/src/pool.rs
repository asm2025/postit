use postit_config::DatabaseSettings;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgPoolOptions};
use sqlx::{Connection, PgPool};

use crate::advisory_lock::{self, MIGRATIONS_LOCK_KEY};
use crate::error::DataError;

/// The workspace's only Postgres pool factory. `database.url` (config, no credentials) and
/// `database.username`/`database.password` (secrets) are combined here so no crate outside
/// `postit-data` builds connection options directly.
#[derive(Clone)]
pub struct Db {
    pool: PgPool,
}

impl Db {
    /// Connects to `database.url`, creating the database first when the server reports it
    /// missing, so a fresh server (or one whose postit database was dropped) needs no manual
    /// step before the migrations run.
    ///
    /// # Errors
    ///
    /// Returns [`DataError::Sql`] when the pool can't be built (bad host, auth failure,
    /// `database.url` missing a host), or when the database is missing and the role may not
    /// create it.
    pub async fn connect(settings: &DatabaseSettings) -> Result<Self, DataError> {
        let host = settings.url.host_str().unwrap_or("localhost");
        let port = settings.url.port().unwrap_or(5432);
        let database = settings.url.path().trim_start_matches('/');

        let options = PgConnectOptions::new()
            .host(host)
            .port(port)
            .database(database)
            .username(&settings.username)
            .password(settings.password.expose());
        let pool_options = || PgPoolOptions::new().max_connections(settings.max_connections);

        let pool = match pool_options().connect_with(options.clone()).await {
            Err(err) if has_code(&err, MISSING_DATABASE) => {
                create_database(&options, database).await?;
                pool_options().connect_with(options).await?
            }
            other => other?,
        };

        Ok(Self { pool })
    }

    /// Wraps an existing pool (tests hand the composition root a `#[sqlx::test]` pool).
    #[must_use]
    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    /// The underlying connection pool.
    #[must_use]
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Runs every embedded migration under [`MIGRATIONS_LOCK_KEY`], so two processes
    /// starting at once apply them exactly once between them.
    ///
    /// # Errors
    ///
    /// Returns [`DataError::Migrate`] if a migration fails, or [`DataError::Sql`] if the
    /// advisory lock can't be taken.
    pub async fn run_migrations(&self) -> Result<(), DataError> {
        let pool = self.pool.clone();
        advisory_lock::with_session_lock(&self.pool, MIGRATIONS_LOCK_KEY, move || async move {
            sqlx::migrate!().run(&pool).await.map_err(DataError::from)
        })
        .await
    }
}

/// SQLSTATE `invalid_catalog_name`: the database named in the connection does not exist.
const MISSING_DATABASE: &str = "3D000";
/// SQLSTATE `duplicate_database`: another process created it first.
const DUPLICATE_DATABASE: &str = "42P04";
/// SQLSTATE `unique_violation`: what a `CREATE DATABASE` racing another one for the same
/// name gets instead of `42P04`, from `pg_database`'s unique name index.
const UNIQUE_VIOLATION: &str = "23505";

fn has_code(err: &sqlx::Error, code: &str) -> bool {
    err.as_database_error()
        .and_then(sqlx::error::DatabaseError::code)
        .is_some_and(|c| c == code)
}

/// Creates `database` through the server's `postgres` maintenance database, owned by the
/// connecting role. Two processes starting at once may both try; the loser's
/// `duplicate_database` or `unique_violation` counts as success.
async fn create_database(options: &PgConnectOptions, database: &str) -> Result<(), DataError> {
    let mut conn = PgConnection::connect_with(&options.clone().database("postgres")).await?;
    let quoted = database.replace('"', "\"\"");
    let created = sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE \"{quoted}\"")))
        .execute(&mut conn)
        .await;
    // Best effort: the connection is dropped either way.
    let _closed = conn.close().await;
    match created {
        Ok(_) => {
            tracing::info!(database, "created the missing database");
            Ok(())
        }
        Err(err) if has_code(&err, DUPLICATE_DATABASE) || has_code(&err, UNIQUE_VIOLATION) => {
            Ok(())
        }
        Err(err) => Err(err.into()),
    }
}
