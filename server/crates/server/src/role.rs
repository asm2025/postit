use anyhow::bail;
use postit_config::Environment;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    All,
    Api,
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

    #[must_use]
    pub fn runs_api(self) -> bool {
        matches!(self, Self::All | Self::Api)
    }

    #[must_use]
    pub fn runs_worker(self) -> bool {
        matches!(self, Self::All | Self::Worker)
    }
}
