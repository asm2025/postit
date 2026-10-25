//! The one conformance suite every agent passes (plan 01, "One conformance suite").
//!
//! An agent crate builds a [`ConformanceFixture`] (the agent, a context factory, a connected
//! account, sample variants, credential and recipient samples, the secrets that must never
//! show, and a [`Faults`] implementation) and calls [`run_conformance`]. The suite talks to the
//! agent only through the `postit-core` contract and to the fake platform only through
//! [`Faults`], so an HTTP agent faked with wiremock passes it the same way the in-memory mock
//! agent does.
//!
//! Every check runs even when an earlier one failed, and [`Faults::clear`] runs before and
//! after each. The checks, by the name a failure carries:
//!
//! - `descriptor`: the descriptor validates and registers.
//! - `schema_subset`: the config, options, credentials, and per-account publish-options
//!   schemas survive serialize, re-parse, compare.
//! - `messages`: every message reachable from the descriptor and schemas has English text and a
//!   key prefixed `<platform>.`.
//! - `capabilities`: declared capabilities equal [`postit_core::exposed_capabilities`].
//! - `dry_run`: `publish` (and `comment`, when offered) in dry-run mode returns `Simulated`,
//!   makes no remote call, and saves no container ref.
//! - `validate`: each sample passes `validate` with no violations.
//! - `error_mapping`: each [`Fault`] on a live publish maps to the right retry class.
//! - `idempotency`: reconcile after a lost response, and no second post for a reused key. A
//!   repeat proves no second post by making no mutating call, by returning the first post's
//!   remote ID, or by an `Ambiguous` error; a receipt without a remote ID proves nothing alone.
//!   For a platform that cannot deduplicate ([`ConformanceFixture::deduplicates`] false) the
//!   repeat-key part does not run, and the check reports skipped when the rest passed.
//! - `cancellation`: a publish cancelled from the start returns `Cancelled` and mutates nothing.
//! - `credentials`: valid fields discover an account and invalid ones are rejected (skipped
//!   for an OAuth agent).
//! - `direct`: `send_batch` answers each recipient once (skipped for a broadcast agent).
//! - `redaction`: no secret shows in any error, outcome, receipt, or log line of the run.
//!
//! The runner installs a thread-local log capture, so run it on a current-thread runtime
//! (`#[tokio::test]`'s default) and in its own test binary.
//!
//! Limits:
//!
//! - `redaction` checks only the log lines the capture saw. Events logged on another thread
//!   (a task the agent spawns onto a multi-thread runtime) are not captured, so that part of the
//!   check can pass without proving anything.
//! - Zero-count assertions (`dry_run`, `cancellation`) trust the fixture's counters;
//!   `idempotency` fails when a clean live publish does not move them, and `error_mapping`
//!   fails when an `AcceptedThenLost` request went uncounted.
//! - A panic in the agent is not caught: it aborts the run and the test, with no report.

mod checks;

use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use postit_core::{
    AccountRef, Agent, AgentContext, CredentialCheckContext, CredentialFields, DirectRecipient,
    Variant,
};
use postit_http::testkit::leak::LogCapture;

use checks::{Renderings, Verdict};

/// A platform failure the suite injects into the next live publish.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// The platform rejects the credentials (HTTP 401).
    Unauthorized,
    /// The platform asks the caller to slow down, with a delay when given.
    RateLimited(Option<Duration>),
    /// The platform fails with a server error (HTTP 500).
    ServerError,
    /// The platform acts on the request, then the response is lost.
    AcceptedThenLost,
    /// The request reaches the platform, which does not act on it, and no response arrives.
    LostBeforeAccept,
    /// The platform answers success with a body that cannot be read.
    Malformed,
}

/// Breaks the fake platform on request and counts the calls it receives.
#[async_trait]
pub trait Faults: Send + Sync {
    /// Makes the platform fail with `fault` until [`Faults::clear`].
    async fn arm(&self, fault: Fault);
    /// Removes every armed fault.
    async fn clear(&self);
    /// Requests the platform has received so far.
    fn remote_calls(&self) -> usize;
    /// Received requests that could change the platform (anything but a read).
    fn mutating_calls(&self) -> usize;
}

/// Credentials for a credentials agent: fields that connect and fields the platform rejects.
pub struct CredentialSamples {
    /// Fields that discover at least one account.
    pub valid: CredentialFields,
    /// Fields that fail with `InvalidCredentials`.
    pub invalid: CredentialFields,
}

/// Everything the suite needs to exercise one agent.
pub struct ConformanceFixture {
    /// The agent under test.
    pub agent: Arc<dyn Agent>,
    /// Builds a fresh context for a client bound to [`ConformanceFixture::account`]'s grant.
    pub context: Arc<dyn Fn() -> AgentContext + Send + Sync>,
    /// The context for `verify_credentials`.
    pub check_context: CredentialCheckContext,
    /// One connected account.
    pub account: AccountRef,
    /// A valid sample per declared format. Live checks use the first one, so put a media
    /// sample first when the agent uploads.
    pub samples: Vec<Variant>,
    /// For a credentials agent, valid and invalid fields.
    pub credentials: Option<CredentialSamples>,
    /// For a direct agent, the recipients of a sample batch.
    pub recipients: Vec<DirectRecipient>,
    /// Strings no output of the run may contain (tokens, passwords).
    pub secrets: Vec<String>,
    /// The fake platform's fault switch and call counters.
    pub faults: Arc<dyn Faults>,
    /// Whether a publish repeated with the same idempotency key returns the first post (the
    /// platform or the agent deduplicates). `false` for a platform that cannot, such as a
    /// webhook with no lookup: `idempotency` then skips its repeat-key part.
    pub deduplicates: bool,
}

/// One failed check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckFailure {
    /// The check's name, such as `dry_run`.
    pub check: &'static str,
    /// What went wrong. Never carries a secret from the fixture.
    pub detail: String,
}

/// The result of a run: every failed check, and the checks that did not apply.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConformanceReport {
    /// Failed checks, in run order.
    pub failures: Vec<CheckFailure>,
    /// Checks that do not apply to the agent (`direct` for a broadcast agent, `credentials`
    /// for an OAuth agent, `idempotency` for a platform that does not deduplicate).
    pub skipped: Vec<&'static str>,
}

impl ConformanceReport {
    /// Whether every check passed or was skipped.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.failures.is_empty()
    }

    /// Panics listing every failure.
    ///
    /// # Panics
    ///
    /// When any check failed.
    pub fn assert_passed(&self) {
        let mut text = String::new();
        for failure in &self.failures {
            let _ = writeln!(text, "- {}: {}", failure.check, failure.detail);
        }
        assert!(
            self.passed(),
            "{} conformance check(s) failed:\n{text}",
            self.failures.len()
        );
    }
}

async fn step(
    report: &mut ConformanceReport,
    faults: &dyn Faults,
    name: &'static str,
    check: impl Future<Output = Verdict>,
) {
    faults.clear().await;
    let verdict = check.await;
    faults.clear().await;
    match verdict {
        Verdict::Passed => {}
        Verdict::Skipped => report.skipped.push(name),
        Verdict::Failed(problems) => report.failures.push(CheckFailure {
            check: name,
            detail: problems.join("; "),
        }),
    }
}

/// Runs every check against `fixture` and reports all failures.
pub async fn run_conformance(fixture: ConformanceFixture) -> ConformanceReport {
    let (capture, guard) = LogCapture::install();
    let fx = &fixture;
    let faults = fx.faults.as_ref();
    let seen = Renderings::default();
    let seen = &seen;
    let mut report = ConformanceReport::default();
    let r = &mut report;
    step(r, faults, "descriptor", checks::descriptor(fx)).await;
    step(r, faults, "schema_subset", checks::schema_subset(fx, seen)).await;
    step(r, faults, "messages", checks::messages(fx, seen)).await;
    step(r, faults, "capabilities", checks::capabilities(fx, seen)).await;
    step(r, faults, "dry_run", checks::dry_run(fx, seen)).await;
    step(r, faults, "validate", checks::validate(fx, seen)).await;
    step(r, faults, "error_mapping", checks::error_mapping(fx, seen)).await;
    step(r, faults, "idempotency", checks::idempotency(fx, seen)).await;
    step(r, faults, "cancellation", checks::cancellation(fx, seen)).await;
    step(r, faults, "credentials", checks::credentials(fx, seen)).await;
    step(r, faults, "direct", checks::direct(fx, seen)).await;
    step(
        r,
        faults,
        "redaction",
        checks::redaction(fx, seen, &capture),
    )
    .await;
    drop(guard);
    report
}
