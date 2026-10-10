use anyhow::bail;
use postit_config::Environment;

/// Which parts of postit a process runs, from `POSTIT_ROLE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// API and worker together.
    All,
    /// The API only.
    Api,
    /// The job worker only.
    Worker,
}

impl Role {
    /// `POSTIT_ROLE`: `all`, `api`, or `worker`. Defaults to `all` in development only.
    ///
    /// # Errors
    ///
    /// Fails on an unknown value, or when unset outside development.
    pub fn resolve(raw: Option<&str>, env: Environment) -> anyhow::Result<Self> {
        match raw {
            Some("all") => Ok(Self::All),
            Some("api") => Ok(Self::Api),
            Some("worker") => Ok(Self::Worker),
            Some(other) => bail!("POSTIT_ROLE must be all, api, or worker (got {other})"),
            None if env == Environment::Development => Ok(Self::All),
            None => bail!("POSTIT_ROLE is required outside development (all, api, or worker)"),
        }
    }

    /// Whether this role serves the API.
    #[must_use]
    pub fn runs_api(self) -> bool {
        matches!(self, Self::All | Self::Api)
    }

    /// Whether this role runs the job worker.
    #[must_use]
    pub fn runs_worker(self) -> bool {
        matches!(self, Self::All | Self::Worker)
    }
}
