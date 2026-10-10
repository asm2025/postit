use std::collections::{BTreeMap, HashMap};

use serde_json::Value;
use uuid::Uuid;

use crate::job::{Job, Queue};

/// ID-and-enum fields of one job, safe to show an admin.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JobSummary(pub BTreeMap<String, String>);

/// The console actions a job may offer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Actions {
    /// Re-run the job as a new job.
    pub retry: bool,
    /// Cancel the job before it finishes.
    pub cancel: bool,
    /// Delete the job and its history.
    pub delete: bool,
}

impl Actions {
    /// Every action enabled.
    pub const ALL: Self = Self {
        retry: true,
        cancel: true,
        delete: true,
    };
    /// Every action disabled.
    pub const NONE: Self = Self {
        retry: false,
        cancel: false,
        delete: false,
    };
}

/// How the console presents and treats one job type.
#[derive(Debug, Clone)]
pub struct ConsoleSpec {
    id_fields: Vec<(&'static str, &'static str)>,
    enum_fields: Vec<(&'static str, &'static str)>,
    actions: Actions,
    disabled_reason: Option<&'static str>,
    show_error_message: bool,
    schedule: Option<String>,
    /// The job type's queue name; `ConsoleCatalog::add` fills it from `J::QUEUE`, so the
    /// console can enqueue a manual run without a `JobRegistry` (the `api` role has none).
    queue: &'static str,
}

impl Default for ConsoleSpec {
    fn default() -> Self {
        Self::new()
    }
}

impl ConsoleSpec {
    /// A spec with every action enabled and no payload fields shown.
    #[must_use]
    pub fn new() -> Self {
        Self {
            id_fields: Vec::new(),
            enum_fields: Vec::new(),
            actions: Actions::ALL,
            disabled_reason: None,
            show_error_message: false,
            schedule: None,
            queue: Queue::Default.as_str(),
        }
    }

    /// A payload value that must parse as a UUID to be shown.
    #[must_use]
    pub fn id_field(mut self, payload_key: &'static str, label: &'static str) -> Self {
        self.id_fields.push((payload_key, label));
        self
    }

    /// A payload value that must match `[a-z0-9_]{1,32}` to be shown.
    #[must_use]
    pub fn enum_field(mut self, payload_key: &'static str, label: &'static str) -> Self {
        self.enum_fields.push((payload_key, label));
        self
    }

    /// Sets which actions the console offers.
    #[must_use]
    pub fn actions(mut self, actions: Actions) -> Self {
        self.actions = actions;
        self
    }

    /// Turns every action off and records why (domain state decides whether work happens).
    #[must_use]
    pub fn disabled(mut self, reason: &'static str) -> Self {
        self.actions = Actions::NONE;
        self.disabled_reason = Some(reason);
        self
    }

    /// Opt in to storing and showing the handler's error message. Only for job types whose
    /// errors cannot contain user content.
    #[must_use]
    pub fn show_error_message(mut self) -> Self {
        self.show_error_message = true;
        self
    }

    /// Cron text shown on the recurring page.
    #[must_use]
    pub fn recurring(mut self, schedule: &str) -> Self {
        self.schedule = Some(schedule.to_owned());
        self
    }

    /// The actions the console offers.
    #[must_use]
    pub fn allowed(&self) -> Actions {
        self.actions
    }

    /// Why the actions are disabled, if they are.
    #[must_use]
    pub fn disabled_reason(&self) -> Option<&'static str> {
        self.disabled_reason
    }

    /// Whether the handler's error message may be stored and shown.
    #[must_use]
    pub fn shows_error_message(&self) -> bool {
        self.show_error_message
    }

    /// The cron text of the recurring schedule, if any.
    #[must_use]
    pub fn schedule(&self) -> Option<&str> {
        self.schedule.as_deref()
    }

    /// The queue name the job type runs on.
    #[must_use]
    pub fn queue(&self) -> &'static str {
        self.queue
    }

    /// The allow-listed payload fields to show for `payload`.
    #[must_use]
    pub fn summarize(&self, payload: &Value) -> JobSummary {
        let mut fields = BTreeMap::new();
        for (key, label) in &self.id_fields {
            if let Some(text) = payload.get(*key).and_then(Value::as_str)
                && Uuid::parse_str(text).is_ok()
            {
                fields.insert((*label).to_owned(), text.to_owned());
            }
        }
        for (key, label) in &self.enum_fields {
            if let Some(text) = payload.get(*key).and_then(Value::as_str)
                && is_token(text)
            {
                fields.insert((*label).to_owned(), text.to_owned());
            }
        }
        JobSummary(fields)
    }
}

fn is_token(text: &str) -> bool {
    (1..=32).contains(&text.len())
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// The console spec of every job type, keyed by type name.
#[derive(Debug, Clone, Default)]
pub struct ConsoleCatalog {
    specs: HashMap<&'static str, ConsoleSpec>,
}

impl ConsoleCatalog {
    /// Registers `spec` for job type `J`, taking its queue from `J`.
    pub fn add<J: Job>(&mut self, mut spec: ConsoleSpec) {
        spec.queue = J::QUEUE.as_str();
        self.specs.insert(J::JOB_TYPE, spec);
    }

    /// The spec of `job_type`, if registered.
    #[must_use]
    pub fn get(&self, job_type: &str) -> Option<&ConsoleSpec> {
        self.specs.get(job_type)
    }

    /// The registered job types, sorted.
    #[must_use]
    pub fn job_types(&self) -> Vec<&'static str> {
        let mut types: Vec<_> = self.specs.keys().copied().collect();
        types.sort_unstable();
        types
    }

    /// `(name, cron text)` of every recurring job, sorted by name.
    #[must_use]
    pub fn recurring(&self) -> Vec<(&'static str, String)> {
        let mut rows: Vec<_> = self
            .specs
            .iter()
            .filter_map(|(name, spec)| spec.schedule().map(|s| (*name, s.to_owned())))
            .collect();
        rows.sort_unstable_by_key(|(name, _)| *name);
        rows
    }
}
