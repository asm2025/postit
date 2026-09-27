use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;

use futures::future::BoxFuture;
use serde_json::Value;

use crate::error::{JobError, JobsError};
use crate::job::{Job, JobContext, Queue, RetryPolicy};

pub(crate) type BoxedHandler =
    Arc<dyn Fn(Value, JobContext) -> BoxFuture<'static, Result<(), JobError>> + Send + Sync>;

#[expect(
    dead_code,
    reason = "queue/retry/recurring/handler consumed by the worker/dispatcher in Task 6"
)]
pub(crate) struct Registration {
    pub queue: Queue,
    pub retry: RetryPolicy,
    pub recurring: bool,
    pub handler: BoxedHandler,
}

/// Job handlers by job type. Built once at startup by each domain crate's `register`
/// function and handed to [`crate::Worker`].
#[derive(Default)]
pub struct JobRegistry {
    pub(crate) jobs: HashMap<&'static str, Registration>,
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
