use postit_config::DatabaseSettings;
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

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
    /// # Errors
    ///
    /// Returns [`DataError::Sql`] when the pool can't be built (bad host, auth failure,
    /// `database.url` missing a host).
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

        let pool = PgPoolOptions::new()
            .max_connections(settings.max_connections)
            .connect_with(options)
            .await?;

        Ok(Self { pool })
    }

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
