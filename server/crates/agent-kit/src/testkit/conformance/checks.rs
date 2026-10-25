//! One `async fn` per conformance check. Each returns a [`Verdict`] listing every problem it
//! found; none stops at the first, and none panics on a broken agent.

use std::collections::BTreeSet;
use std::fmt::Debug;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use postit_core::{
    AgentClient, AgentError, AgentKind, AgentRegistry, ConnectMethod, DeliveryId, FieldKind,
    Message, OptionsSchema, PostKind, PublishMode, PublishOutcome, PublishProgress, PublishRequest,
    ReconcileOutcome, ReconcileRequest, ReplyTo, RetryClass, Variant, exposed_capabilities,
};
use postit_http::testkit::leak::{LogCapture, find_leak};
use uuid::Uuid;

use super::{ConformanceFixture, Fault};

/// What one check concluded.
pub(super) enum Verdict {
    Passed,
    Skipped,
    Failed(Vec<String>),
}

/// Collects a check's problems.
#[derive(Default)]
struct Problems(Vec<String>);

impl Problems {
    fn fail(&mut self, problem: impl Into<String>) {
        self.0.push(problem.into());
    }

    fn verdict(self) -> Verdict {
        if self.0.is_empty() {
            Verdict::Passed
        } else {
            Verdict::Failed(self.0)
        }
    }
}

/// Every rendering of an agent's output during the run, for the redaction check.
#[derive(Default)]
pub(super) struct Renderings(Mutex<Vec<(String, String)>>);

impl Renderings {
    fn push(&self, label: String, text: String) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((label, text));
    }

    /// Records `result`'s `Debug` and, for an error, its `Display`.
    fn result<T: Debug>(&self, label: &str, result: &Result<T, AgentError>) {
        if let Err(err) = result {
            self.push(format!("{label} error display"), err.to_string());
        }
        self.push(format!("{label} debug"), format!("{result:?}"));
    }

    fn snapshot(&self) -> Vec<(String, String)> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

/// A [`PublishProgress`] that records saved container refs.
struct Progress {
    cancel: bool,
    saved: Mutex<Vec<String>>,
}

impl Progress {
    fn new(cancel: bool) -> Self {
        Self {
            cancel,
            saved: Mutex::default(),
        }
    }

    fn saved(&self) -> usize {
        self.saved
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }
}

#[async_trait]
impl PublishProgress for Progress {
    async fn save_container_ref(&self, container_ref: &str) -> Result<(), AgentError> {
        self.saved
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(container_ref.to_owned());
        Ok(())
    }

    fn cancel_requested(&self) -> bool {
        self.cancel
    }
}

fn fresh_key() -> DeliveryId {
    DeliveryId::from_uuid(Uuid::now_v7())
}

/// A client from a fresh context, or the problem that prevented one.
fn client(fx: &ConformanceFixture, seen: &Renderings) -> Result<Box<dyn AgentClient>, String> {
    let created = fx.agent.create_client((fx.context)());
    match created {
        Ok(client) => Ok(client),
        Err(err) => {
            let detail = format!("create_client failed: {:?}", err.retry_class());
            seen.result::<()>("create_client", &Err(err));
            Err(detail)
        }
    }
}

/// A client and the first sample, or every problem that prevented either.
fn client_and_sample<'a>(
    fx: &'a ConformanceFixture,
    seen: &Renderings,
) -> Result<(Box<dyn AgentClient>, &'a Variant), Vec<String>> {
    let sample = fx.samples.first();
    match (client(fx, seen), sample) {
        (Ok(client), Some(sample)) => Ok((client, sample)),
        (client, sample) => Err(client
            .err()
            .into_iter()
            .chain(
                sample
                    .is_none()
                    .then(|| "the fixture has no sample variants".to_owned()),
            )
            .collect()),
    }
}

async fn publish(
    client: &dyn AgentClient,
    seen: &Renderings,
    label: &str,
    variant: &Variant,
    mode: PublishMode,
    key: DeliveryId,
    progress: &Progress,
) -> Result<PublishOutcome, AgentError> {
    let request = PublishRequest {
        variant,
        mode,
        native_publish_at: None,
        container_ref: None,
        idempotency_key: key,
    };
    let result = client.publish(request, progress).await;
    seen.result(label, &result);
    result
}

/// A live publish with no cancellation.
async fn live(
    client: &dyn AgentClient,
    seen: &Renderings,
    label: &str,
    variant: &Variant,
    key: DeliveryId,
) -> Result<PublishOutcome, AgentError> {
    let progress = Progress::new(false);
    publish(
        client,
        seen,
        label,
        variant,
        PublishMode::Live,
        key,
        &progress,
    )
    .await
}

async fn reconcile(
    fx: &ConformanceFixture,
    client: &dyn AgentClient,
    seen: &Renderings,
    label: &str,
    variant: &Variant,
    key: DeliveryId,
) -> Result<ReconcileOutcome, AgentError> {
    let request = ReconcileRequest {
        variant,
        container_ref: None,
        attempted_at: (fx.context)().clock.now(),
        idempotency_key: key,
    };
    let result = client.reconcile(request).await;
    seen.result(label, &result);
    result
}

/// Short text naming an outcome without its contents.
fn outcome_kind(result: &Result<PublishOutcome, AgentError>) -> String {
    match result {
        Ok(PublishOutcome::Published(_)) => "Published".to_owned(),
        Ok(PublishOutcome::Processing(_)) => "Processing".to_owned(),
        Ok(PublishOutcome::Simulated { .. }) => "Simulated".to_owned(),
        Err(err) => format!("an error of class {:?}", err.retry_class()),
    }
}

/// Every schema the agent declares, with where it came from. The per-account publish options
/// need a client; failing to get them is a problem.
async fn schemas(
    fx: &ConformanceFixture,
    seen: &Renderings,
    problems: &mut Problems,
) -> Vec<(&'static str, OptionsSchema)> {
    let descriptor = fx.agent.descriptor();
    let mut out = vec![("options", descriptor.options_schema.clone())];
    if let Some(config) = fx.agent.config_schema() {
        out.push(("config", config.clone()));
    }
    if let ConnectMethod::Credentials(schema) = &descriptor.connect {
        out.push(("credentials", schema.clone()));
    }
    match client(fx, seen) {
        Ok(client) => {
            let result = client.publish_options(&fx.account).await;
            seen.result("publish_options", &result);
            match result {
                Ok(schema) => out.push(("publish options", schema)),
                Err(err) => problems.fail(format!(
                    "publish_options failed with class {:?}",
                    err.retry_class()
                )),
            }
        }
        Err(problem) => problems.fail(problem),
    }
    out
}

fn schema_messages(schema: &OptionsSchema) -> Vec<Message> {
    let mut out = Vec::new();
    for field in schema.fields() {
        out.push(field.label.clone());
        out.extend(field.help.clone());
        out.extend(field.consent_text.clone());
        if let FieldKind::Enum { values, .. } = &field.kind {
            out.extend(values.iter().map(|v| v.label.clone()));
        }
    }
    out
}

/// 1a. The descriptor validates and registers.
#[allow(clippy::unused_async)] // every check is async so the runner treats them alike
pub(super) async fn descriptor(fx: &ConformanceFixture) -> Verdict {
    let mut problems = Problems::default();
    let descriptor = fx.agent.descriptor();
    if let Err(err) = descriptor.validate() {
        problems.fail(format!("the descriptor is invalid: {err}"));
    }
    if let Err(err) = AgentRegistry::new().register(fx.agent.clone()) {
        problems.fail(format!("the agent does not register: {err}"));
    }
    if fx.account.platform != descriptor.platform {
        problems.fail("the fixture's account is on another platform");
    }
    problems.verdict()
}

/// 1b. Every schema is inside the subset: serialize, re-parse, compare.
pub(super) async fn schema_subset(fx: &ConformanceFixture, seen: &Renderings) -> Verdict {
    let mut problems = Problems::default();
    for (name, schema) in schemas(fx, seen, &mut problems).await {
        let reparsed = serde_json::to_string(&schema)
            .map_err(|e| e.to_string())
            .and_then(|json| OptionsSchema::from_json_str(&json).map_err(|e| e.to_string()));
        match reparsed {
            Ok(back) if back == schema => {}
            Ok(_) => problems.fail(format!("the {name} schema changes when re-parsed")),
            Err(err) => problems.fail(format!("the {name} schema leaves the subset: {err}")),
        }
    }
    problems.verdict()
}

/// 2. Every reachable message has English text and a key prefixed by the platform ID.
pub(super) async fn messages(fx: &ConformanceFixture, seen: &Renderings) -> Verdict {
    let mut problems = Problems::default();
    let descriptor = fx.agent.descriptor();
    let prefix = format!("{}.", descriptor.platform);
    let mut all: Vec<Message> = descriptor
        .formats
        .iter()
        .flat_map(|f| f.media.safe_zones.iter().map(|z| z.label.clone()))
        .collect();
    // A publish_options failure is already reported by `schema_subset`; report it once.
    let mut reported_elsewhere = Problems::default();
    for (_, schema) in schemas(fx, seen, &mut reported_elsewhere).await {
        all.extend(schema_messages(&schema));
    }
    for message in all {
        if message.en.trim().is_empty() {
            problems.fail(format!("message {:?} has no English text", message.key));
        }
        if !message.key.starts_with(&prefix) || message.key.len() == prefix.len() {
            problems.fail(format!(
                "message key {:?} is not prefixed {prefix:?}",
                message.key
            ));
        }
    }
    problems.verdict()
}

/// 3. Declared capabilities equal the exposed ones.
#[allow(clippy::unused_async)] // every check is async so the runner treats them alike
pub(super) async fn capabilities(fx: &ConformanceFixture, seen: &Renderings) -> Verdict {
    let mut problems = Problems::default();
    match client(fx, seen) {
        Ok(client) => {
            let declared = &fx.agent.descriptor().capabilities;
            let exposed = exposed_capabilities(client.as_ref());
            if *declared != exposed {
                problems.fail(format!("declared {declared:?}, exposed {exposed:?}"));
            }
        }
        Err(problem) => problems.fail(problem),
    }
    problems.verdict()
}

/// 4. A dry run returns `Simulated`, calls nothing, and saves no container ref.
pub(super) async fn dry_run(fx: &ConformanceFixture, seen: &Renderings) -> Verdict {
    let mut problems = Problems::default();
    if fx.samples.is_empty() {
        problems.fail("the fixture has no sample variants");
    }
    let client = match client(fx, seen) {
        Ok(client) => client,
        Err(problem) => {
            problems.fail(problem);
            return problems.verdict();
        }
    };
    for (i, sample) in fx.samples.iter().enumerate() {
        let progress = Progress::new(false);
        let before = fx.faults.remote_calls();
        let result = publish(
            client.as_ref(),
            seen,
            "dry-run publish",
            sample,
            PublishMode::DryRun,
            fresh_key(),
            &progress,
        )
        .await;
        judge_dry_run(
            fx,
            &mut problems,
            &format!("publish of sample {i}"),
            &result,
            before,
            &progress,
        );

        if let Some(commenter) = client.commenter() {
            let progress = Progress::new(false);
            let before = fx.faults.remote_calls();
            // A comment answers a post, so the dry run gets a comment-kind variant with a
            // reply target; the post need not exist because a dry run calls nothing.
            let reply = Variant {
                kind: PostKind::Comment,
                reply_to: Some(ReplyTo {
                    remote_post_id: "conformance-post".to_owned(),
                    remote_comment_id: None,
                    url: None,
                }),
                ..sample.clone()
            };
            let request = PublishRequest {
                variant: &reply,
                mode: PublishMode::DryRun,
                native_publish_at: None,
                container_ref: None,
                idempotency_key: fresh_key(),
            };
            let result = commenter.comment(request, &progress).await;
            seen.result("dry-run comment", &result);
            judge_dry_run(
                fx,
                &mut problems,
                &format!("comment of sample {i}"),
                &result,
                before,
                &progress,
            );
        }
    }
    problems.verdict()
}

fn judge_dry_run(
    fx: &ConformanceFixture,
    problems: &mut Problems,
    what: &str,
    result: &Result<PublishOutcome, AgentError>,
    calls_before: usize,
    progress: &Progress,
) {
    if !matches!(result, Ok(PublishOutcome::Simulated { .. })) {
        problems.fail(format!("dry-run {what} returned {}", outcome_kind(result)));
    }
    let calls = fx.faults.remote_calls().saturating_sub(calls_before);
    if calls > 0 {
        problems.fail(format!("dry-run {what} made {calls} remote call(s)"));
    }
    if progress.saved() > 0 {
        problems.fail(format!("dry-run {what} saved a container ref"));
    }
}

/// 5. Every sample passes `validate` with no violations.
pub(super) async fn validate(fx: &ConformanceFixture, seen: &Renderings) -> Verdict {
    let mut problems = Problems::default();
    if fx.samples.is_empty() {
        problems.fail("the fixture has no sample variants");
    }
    let client = match client(fx, seen) {
        Ok(client) => client,
        Err(problem) => {
            problems.fail(problem);
            return problems.verdict();
        }
    };
    for (i, sample) in fx.samples.iter().enumerate() {
        let result = client.validate(sample, &[]).await;
        seen.result("validate", &result);
        match result {
            Ok(violations) if violations.is_empty() => {}
            Ok(violations) => problems.fail(format!(
                "sample {i} has {} violation(s): {:?}",
                violations.len(),
                violations.iter().map(|v| v.code).collect::<Vec<_>>()
            )),
            Err(err) => problems.fail(format!(
                "validate of sample {i} failed with class {:?}",
                err.retry_class()
            )),
        }
    }
    problems.verdict()
}

/// The retry class a fault on a live publish must map to.
#[derive(Clone, Copy)]
enum Want {
    /// Exactly this class.
    Class(RetryClass),
    /// `Ambiguous` once a mutating request was sent; `Retryable` or `Ambiguous` before.
    SafeRetry,
    /// `Ambiguous` once a mutating request was sent; anything before.
    AmbiguousIfSent,
}

/// 6. Each fault on a live publish maps to the retry class the engine needs.
pub(super) async fn error_mapping(fx: &ConformanceFixture, seen: &Renderings) -> Verdict {
    const DELAY: Duration = Duration::from_secs(7);
    let mut problems = Problems::default();
    let (client, sample) = match client_and_sample(fx, seen) {
        Ok(pair) => pair,
        Err(setup) => {
            problems.0.extend(setup);
            return problems.verdict();
        }
    };
    let cases = [
        (Fault::Unauthorized, Want::Class(RetryClass::NeedsReconnect)),
        (
            Fault::RateLimited(Some(DELAY)),
            Want::Class(RetryClass::RetryAfter(DELAY)),
        ),
        (Fault::RateLimited(None), Want::Class(RetryClass::Retryable)),
        (Fault::ServerError, Want::SafeRetry),
        (Fault::AcceptedThenLost, Want::Class(RetryClass::Ambiguous)),
        (Fault::LostBeforeAccept, Want::SafeRetry),
        (Fault::Malformed, Want::AmbiguousIfSent),
    ];
    for (fault, want) in cases {
        fx.faults.clear().await;
        fx.faults.arm(fault).await;
        let mutating_before = fx.faults.mutating_calls();
        let result = publish(
            client.as_ref(),
            seen,
            &format!("{fault:?} publish"),
            sample,
            PublishMode::Live,
            fresh_key(),
            &Progress::new(false),
        )
        .await;
        let sent = fx.faults.mutating_calls() > mutating_before;
        fx.faults.clear().await;
        if fault == Fault::AcceptedThenLost && !sent {
            // By definition the platform acted, so a mutating request reached it.
            problems
                .fail("AcceptedThenLost: the fixture's Faults did not count the faulted request");
        }
        let class = match &result {
            Ok(_) => {
                problems.fail(format!(
                    "{fault:?}: the publish returned {}",
                    outcome_kind(&result)
                ));
                continue;
            }
            Err(err) => err.retry_class(),
        };
        match want {
            Want::Class(want) if class != want => {
                problems.fail(format!("{fault:?}: got {class:?}, want {want:?}"));
            }
            Want::SafeRetry | Want::AmbiguousIfSent if sent && class != RetryClass::Ambiguous => {
                problems.fail(format!(
                    "{fault:?} after a mutating call: got {class:?}, want Ambiguous"
                ));
            }
            Want::SafeRetry
                if !sent && !matches!(class, RetryClass::Retryable | RetryClass::Ambiguous) =>
            {
                problems.fail(format!(
                    "{fault:?} with nothing sent: got {class:?}, want Retryable or Ambiguous"
                ));
            }
            _ => {}
        }
    }
    problems.verdict()
}

/// 7. Reconcile finds what a lost response hid, and a reused key never posts twice.
///
/// When the fixture says the platform does not deduplicate, the repeat-key part does not run
/// and a check that otherwise passed reports `Skipped`.
pub(super) async fn idempotency(fx: &ConformanceFixture, seen: &Renderings) -> Verdict {
    let mut problems = Problems::default();
    let (client, sample) = match client_and_sample(fx, seen) {
        Ok(pair) => pair,
        Err(setup) => {
            problems.0.extend(setup);
            return problems.verdict();
        }
    };
    let client = client.as_ref();

    // The platform acted, the response was lost: reconcile finds the post, and publishing
    // again with the same key returns it instead of posting twice.
    let landed = fresh_key();
    fx.faults.arm(Fault::AcceptedThenLost).await;
    let _ = live(client, seen, "lost publish", sample, landed).await;
    fx.faults.clear().await;
    match reconcile(fx, client, seen, "reconcile after accept", sample, landed).await {
        Ok(ReconcileOutcome::Found(found)) if fx.deduplicates => {
            let repeat = Repeat {
                label: "republish after accept",
                what: "republishing a reconciled key",
                key: landed,
                original: found.remote_id.as_ref(),
            };
            repeat.check(fx, client, seen, sample, &mut problems).await;
        }
        Ok(ReconcileOutcome::Found(_)) | Err(AgentError::Unsupported { .. }) => {}
        other => problems.fail(format!(
            "reconcile after AcceptedThenLost: want Found or Unsupported, got {}",
            reconcile_kind(&other)
        )),
    }

    // The platform never acted: reconcile must not claim the post exists.
    let lost = fresh_key();
    fx.faults.arm(Fault::LostBeforeAccept).await;
    let _ = live(client, seen, "unaccepted publish", sample, lost).await;
    fx.faults.clear().await;
    match reconcile(fx, client, seen, "reconcile after loss", sample, lost).await {
        Ok(ReconcileOutcome::Absent | ReconcileOutcome::Unresolvable)
        | Err(AgentError::Unsupported { .. }) => {}
        other => problems.fail(format!(
            "reconcile after LostBeforeAccept: want Absent, Unresolvable, or Unsupported, got {}",
            reconcile_kind(&other)
        )),
    }

    // A clean publish, then the same key again. The publish must move both counters, so
    // the zero-count assertions elsewhere (dry run, cancellation) are not vacuous.
    let key = fresh_key();
    let (remote_before, mutating_before) = (fx.faults.remote_calls(), fx.faults.mutating_calls());
    let first = live(client, seen, "first publish", sample, key).await;
    if fx.faults.remote_calls() <= remote_before || fx.faults.mutating_calls() <= mutating_before {
        problems.fail(
            "the fixture's Faults counters do not move: a clean live publish must raise both remote_calls and mutating_calls",
        );
    }
    match &first {
        Ok(PublishOutcome::Published(receipt) | PublishOutcome::Processing(receipt)) => {
            if fx.deduplicates {
                let repeat = Repeat {
                    label: "second publish",
                    what: "publishing a key twice",
                    key,
                    original: receipt.remote_id.as_ref(),
                };
                repeat.check(fx, client, seen, sample, &mut problems).await;
            }
        }
        other => problems.fail(format!(
            "a live publish without faults returned {}",
            outcome_kind(other)
        )),
    }
    if !fx.deduplicates && problems.0.is_empty() {
        return Verdict::Skipped;
    }
    problems.verdict()
}

fn reconcile_kind(result: &Result<ReconcileOutcome, AgentError>) -> String {
    match result {
        Ok(ReconcileOutcome::Found(_)) => "Found".to_owned(),
        Ok(ReconcileOutcome::Absent) => "Absent".to_owned(),
        Ok(ReconcileOutcome::Unresolvable) => "Unresolvable".to_owned(),
        Err(err) => format!("an error of class {:?}", err.retry_class()),
    }
}

/// A live publish repeated with a key that already has a post.
struct Repeat<'a> {
    label: &'static str,
    what: &'static str,
    key: DeliveryId,
    /// The first post's remote ID, when the platform gave one.
    original: Option<&'a String>,
}

impl Repeat<'_> {
    /// The repeat must return the first post or an `Ambiguous` error, never a new post. Only
    /// two things prove no second post: no mutating call reached the platform, or the receipt
    /// names the first post's remote ID. A receipt without a remote ID proves nothing alone.
    async fn check(
        &self,
        fx: &ConformanceFixture,
        client: &dyn AgentClient,
        seen: &Renderings,
        sample: &Variant,
        problems: &mut Problems,
    ) {
        let what = self.what;
        let before = fx.faults.mutating_calls();
        let again = live(client, seen, self.label, sample, self.key).await;
        let mutated = fx.faults.mutating_calls() > before;
        match &again {
            Err(err) if err.retry_class() == RetryClass::Ambiguous => {}
            Ok(PublishOutcome::Published(r) | PublishOutcome::Processing(r)) => {
                match (self.original, r.remote_id.as_ref()) {
                    (Some(first), Some(now)) if first == now => {}
                    (Some(_), Some(_)) if mutated => {
                        problems.fail(format!("{what} created a second post"));
                    }
                    (Some(_), Some(_)) => {
                        problems.fail(format!("{what} returned a different post"));
                    }
                    _ if !mutated => {}
                    _ => problems.fail(format!(
                        "{what} made a mutating call and a receipt has no remote ID, so \
                         nothing proves it did not post twice"
                    )),
                }
            }
            other => problems.fail(format!(
                "{what}: want the same receipt or an Ambiguous error, got {}",
                outcome_kind(other)
            )),
        }
    }
}

/// 8. A publish cancelled from the start returns `Cancelled` and mutates nothing.
pub(super) async fn cancellation(fx: &ConformanceFixture, seen: &Renderings) -> Verdict {
    let mut problems = Problems::default();
    if fx.samples.is_empty() {
        problems.fail("the fixture has no sample variants");
    }
    let client = match client(fx, seen) {
        Ok(client) => client,
        Err(problem) => {
            problems.fail(problem);
            return problems.verdict();
        }
    };
    for (i, sample) in fx.samples.iter().enumerate() {
        let before = fx.faults.mutating_calls();
        let result = publish(
            client.as_ref(),
            seen,
            "cancelled publish",
            sample,
            PublishMode::Live,
            fresh_key(),
            &Progress::new(true),
        )
        .await;
        if !matches!(result, Err(AgentError::Cancelled)) {
            problems.fail(format!(
                "cancelled publish of sample {i} returned {}",
                outcome_kind(&result)
            ));
        }
        let mutated = fx.faults.mutating_calls().saturating_sub(before);
        if mutated > 0 {
            problems.fail(format!(
                "cancelled publish of sample {i} made {mutated} mutating call(s)"
            ));
        }
    }
    problems.verdict()
}

/// 9. Valid fields discover an account; invalid fields are `InvalidCredentials`.
pub(super) async fn credentials(fx: &ConformanceFixture, seen: &Renderings) -> Verdict {
    if matches!(fx.agent.descriptor().connect, ConnectMethod::OAuth(_)) {
        return Verdict::Skipped;
    }
    let mut problems = Problems::default();
    let Some(samples) = &fx.credentials else {
        problems.fail("a credentials agent's fixture has no credential samples");
        return problems.verdict();
    };
    let valid = fx
        .agent
        .verify_credentials(&fx.check_context, &samples.valid)
        .await;
    seen.result("verify valid credentials", &valid);
    match valid {
        Ok(found) if !found.accounts.is_empty() => {}
        Ok(_) => problems.fail("valid credentials discovered no account"),
        Err(AgentError::Unsupported { .. }) => {
            problems.fail("a credentials agent does not verify credentials");
        }
        Err(err) => problems.fail(format!(
            "valid credentials failed with class {:?}",
            err.retry_class()
        )),
    }
    let invalid = fx
        .agent
        .verify_credentials(&fx.check_context, &samples.invalid)
        .await;
    seen.result("verify invalid credentials", &invalid);
    match invalid {
        Err(AgentError::InvalidCredentials { .. }) => {}
        Ok(_) => problems.fail("invalid credentials were accepted"),
        Err(err) => problems.fail(format!(
            "invalid credentials: want InvalidCredentials, got class {:?}",
            err.retry_class()
        )),
    }
    problems.verdict()
}

/// 10. `send_batch` answers every recipient exactly once.
pub(super) async fn direct(fx: &ConformanceFixture, seen: &Renderings) -> Verdict {
    if fx.agent.descriptor().kind != AgentKind::Direct {
        return Verdict::Skipped;
    }
    let mut problems = Problems::default();
    if fx.recipients.is_empty() {
        problems.fail("a direct agent's fixture has no recipients");
    }
    let (client, sample) = match client_and_sample(fx, seen) {
        Ok(pair) => pair,
        Err(setup) => {
            problems.0.extend(setup);
            return problems.verdict();
        }
    };
    let Some(sender) = client.direct() else {
        problems.fail("a direct agent exposes no direct sender");
        return problems.verdict();
    };
    let result = sender.send_batch(&fx.account, &fx.recipients, sample).await;
    seen.result("send_batch", &result);
    match result {
        Ok(results) => {
            if results.len() != fx.recipients.len() {
                problems.fail(format!(
                    "{} result(s) for {} recipient(s)",
                    results.len(),
                    fx.recipients.len()
                ));
            }
            let mut got: Vec<_> = results.iter().map(|r| r.send_id).collect();
            let mut want: Vec<_> = fx.recipients.iter().map(|r| r.send_id).collect();
            got.sort_unstable();
            want.sort_unstable();
            if got != want {
                problems.fail("the results' send IDs do not match the recipients'");
            }
        }
        Err(err) => problems.fail(format!(
            "send_batch failed with class {:?}",
            err.retry_class()
        )),
    }
    problems.verdict()
}

/// 11. No secret shows in any rendering recorded during the run or in its log.
#[allow(clippy::unused_async)] // every check is async so the runner treats them alike
pub(super) async fn redaction(
    fx: &ConformanceFixture,
    seen: &Renderings,
    capture: &LogCapture,
) -> Verdict {
    let mut problems = Problems::default();
    let secrets: BTreeSet<&str> = fx
        .secrets
        .iter()
        .map(String::as_str)
        .filter(|s| !s.is_empty())
        .collect();
    if secrets.is_empty() {
        problems.fail("the fixture names no secrets");
    }
    let secrets: Vec<&str> = secrets.into_iter().collect();
    let recorded = seen.snapshot();
    let log = capture.text();
    let mut outputs: Vec<(&str, &str)> = recorded
        .iter()
        .map(|(label, text)| (label.as_str(), text.as_str()))
        .collect();
    outputs.push(("the captured log", log.as_str()));
    if let Some(leak) = find_leak(&secrets, &outputs) {
        problems.fail(leak);
    }
    problems.verdict()
}
