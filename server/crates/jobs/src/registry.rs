use std::collections::{BTreeSet, HashMap};
use std::future::Future;
use std::str::FromStr;
use std::sync::{Arc, PoisonError, RwLock};

use futures::future::BoxFuture;
use serde_json::Value;

use crate::console_spec::ConsoleCatalog;
use crate::error::{JobError, JobsError};
use crate::job::{Job, JobContext, Queue, RetryPolicy};

pub(crate) type BoxedHandler =
    Arc<dyn Fn(Value, JobContext) -> BoxFuture<'static, Result<(), JobError>> + Send + Sync>;

pub(crate) struct Registration {
    pub queue: Queue,
    pub retry: RetryPolicy,
    pub recurring: bool,
    pub handler: BoxedHandler,
}

/// The job types registered so far, shared with handlers that must know them at run time (the
/// history purge sweeps outbox rows of types no worker registers). A clone sees later
/// registrations, so a handler can be registered before the other job types.
#[derive(Clone, Default)]
pub struct RegisteredTypes(Arc<RwLock<BTreeSet<&'static str>>>);

impl RegisteredTypes {
    /// The registered job types, sorted.
    #[must_use]
    pub fn snapshot(&self) -> Vec<String> {
        self.0
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|t| (*t).to_string())
            .collect()
    }

    fn add(&self, job_type: &'static str) {
        self.0
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(job_type);
    }
}

/// Job handlers by job type. Built once at startup by each domain crate's `register`
/// function and handed to [`crate::Worker`].
#[derive(Default)]
pub struct JobRegistry {
    pub(crate) jobs: HashMap<&'static str, Registration>,
    pub(crate) recurring: Vec<Arc<crate::recurring::RecurringSpec>>,
    pub(crate) console: ConsoleCatalog,
    types: RegisteredTypes,
}

impl JobRegistry {
    /// # Errors
    ///
    /// Returns [`JobsError::DuplicateJobType`] if `J::JOB_TYPE` is already registered.
    pub fn register<J, H, Fut>(&mut self, retry: RetryPolicy, handler: H) -> Result<(), JobsError>
    where
        J: Job,
        H: Fn(J, JobContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), JobError>> + Send + 'static,
    {
        self.insert::<J>(retry, false, typed_handler(handler))
    }

    /// Registers `J` as a recurring job named `J::JOB_TYPE` on a seconds-first cron
    /// `schedule` (`sec min hour day-of-month month day-of-week`). The handler receives
    /// `J::default()`.
    ///
    /// # Errors
    ///
    /// Returns [`JobsError::InvalidSchedule`] if `schedule` doesn't parse, or
    /// [`JobsError::DuplicateJobType`].
    pub fn register_recurring<J, H, Fut>(
        &mut self,
        schedule: &str,
        retry: RetryPolicy,
        handler: H,
    ) -> Result<(), JobsError>
    where
        J: Job + Default,
        H: Fn(J, JobContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), JobError>> + Send + 'static,
    {
        let schedule =
            cron::Schedule::from_str(schedule).map_err(|err| JobsError::InvalidSchedule {
                name: J::JOB_TYPE,
                reason: err.to_string(),
            })?;
        let handler = Arc::new(handler);
        let boxed: BoxedHandler = Arc::new(move |_payload: Value, ctx: JobContext| {
            let handler = Arc::clone(&handler);
            Box::pin(async move { handler(J::default(), ctx).await })
        });
        self.insert::<J>(retry, true, boxed)?;
        self.recurring
            .push(Arc::new(crate::recurring::RecurringSpec {
                name: J::JOB_TYPE,
                queue: J::QUEUE,
                schedule,
            }));
        Ok(())
    }

    /// Attaches the console catalog (what the admin Jobs console may show and do).
    #[must_use]
    pub fn with_console(mut self, catalog: ConsoleCatalog) -> Self {
        self.console = catalog;
        self
    }

    pub(crate) fn console(&self) -> &ConsoleCatalog {
        &self.console
    }

    /// A handle on the registered job types that also sees types registered later.
    #[must_use]
    pub fn registered_types(&self) -> RegisteredTypes {
        self.types.clone()
    }

    /// The registered job type names, sorted.
    #[must_use]
    pub fn job_types(&self) -> Vec<String> {
        let mut types: Vec<String> = self.jobs.keys().map(|t| (*t).to_string()).collect();
        types.sort();
        types
    }

    pub(crate) fn get(&self, job_type: &str) -> Option<&Registration> {
        self.jobs.get(job_type)
    }

    pub(crate) fn insert<J: Job>(
        &mut self,
        retry: RetryPolicy,
        recurring: bool,
        handler: BoxedHandler,
    ) -> Result<(), JobsError> {
        if self.get(J::JOB_TYPE).is_some() {
            return Err(JobsError::DuplicateJobType(J::JOB_TYPE));
        }
        self.jobs.insert(
            J::JOB_TYPE,
            Registration {
                queue: J::QUEUE,
                retry,
                recurring,
                handler,
            },
        );
        self.types.add(J::JOB_TYPE);
        Ok(())
    }
}

fn typed_handler<J, H, Fut>(handler: H) -> BoxedHandler
where
    J: Job,
    H: Fn(J, JobContext) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<(), JobError>> + Send + 'static,
{
    let handler = Arc::new(handler);
    Arc::new(move |value: Value, ctx: JobContext| {
        let handler = Arc::clone(&handler);
        Box::pin(async move {
            let job: J = serde_json::from_value(value).map_err(|err| {
                JobError::Fatal(format!(
                    "payload for `{}` does not deserialize: {err}",
                    J::JOB_TYPE
                ))
            })?;
            handler(job, ctx).await
        })
    })
}
