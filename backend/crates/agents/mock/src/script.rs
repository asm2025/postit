//! [`MockScript`]: a Rust handle that overrides what the pasted values and post options would
//! do, counts calls, and (with the `testkit` feature) is the conformance suite's `Faults`.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use chrono::{DateTime, Utc};
use postit_core::{CredentialFields, DiscoveredAccount};

use crate::platform::{MockComment, MockPlatform, MockPost, MockSend};

/// Chooses a verify result and a delay before it from the pasted values. For tests that need
/// per-row behavior from one agent, such as CSV imports with slow rows.
pub type Decide = Arc<dyn Fn(&CredentialFields) -> (Duration, VerifyScript) + Send + Sync>;

/// What `verify_credentials` does, overriding the token keywords.
#[derive(Clone)]
pub enum VerifyScript {
    /// Succeed, reaching these accounts.
    Accounts(Vec<DiscoveredAccount>),
    /// The platform rejects the credentials, naming a field.
    Invalid(Option<String>),
    /// The platform asks the caller to slow down, with no delay given.
    RateLimited,
    /// A retryable platform failure (HTTP 500).
    Fail,
    /// Decide per call from the pasted values, after a delay.
    ByFields(Decide),
}

/// What `account_info` does, overriding the token keywords.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InfoScript {
    /// The account is active.
    Ok,
    /// The platform rejects the stored credential.
    AuthFailed,
    /// The call succeeds, but the platform reports the account as needing a reconnect.
    NeedsReconnect,
}

/// A platform failure forced on every live publish until cleared. The conformance suite's
/// `Fault` maps onto it one to one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(feature = "testkit"), allow(dead_code))] // only the testkit's `Faults` arms one
pub(crate) enum Forced {
    Unauthorized,
    RateLimited(Option<Duration>),
    ServerError,
    AcceptedThenLost,
    LostBeforeAccept,
    Malformed,
}

/// What `after_exchange` does for `mock_oauth`. The default (nothing scripted) names the user
/// `mock-user` and adds nothing else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExchangeScript {
    /// The remote user ID the exchange reports.
    pub remote_user_id: Option<String>,
    /// When the user must reconnect regardless of refreshing.
    pub reconnect_by: Option<DateTime<Utc>>,
    /// Remote account IDs that get the token `acct-token-<id>`.
    pub account_tokens: Vec<String>,
    /// Fail the exchange with an authentication error.
    pub fail: bool,
}

impl Default for ExchangeScript {
    fn default() -> Self {
        Self {
            remote_user_id: Some("mock-user".to_owned()),
            reconnect_by: None,
            account_tokens: Vec::new(),
            fail: false,
        }
    }
}

/// What OAuth discovery does for `mock_oauth` besides naming the accounts. The default answers
/// at once with the scripted accounts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiscoveryScript {
    /// First saves a refreshed token set with this access token through the credential store,
    /// as an agent that refreshed mid-discovery would.
    pub refresh_to: Option<String>,
    /// Then waits this long before answering.
    pub delay: Option<Duration>,
    /// Then fails with a transport error instead of answering.
    pub fail: bool,
}

#[derive(Default)]
struct Overrides {
    exchange: Option<ExchangeScript>,
    oauth_accounts: Option<usize>,
    discovery: DiscoveryScript,
    verify: Option<VerifyScript>,
    account_info: Option<InfoScript>,
    forced: Option<Forced>,
    fail_revoke: bool,
    publish_steps: usize,
    gate: Option<PublishGate>,
    chunk_gate: Option<(usize, PublishGate)>,
    keep_processing: bool,
    fail_reconcile: bool,
    retry_after: Option<Duration>,
}

/// The state one agent and all its clients share.
#[derive(Default)]
pub(crate) struct Shared {
    pub(crate) platform: MockPlatform,
    overrides: Mutex<Overrides>,
    seen: Mutex<Vec<String>>,
    revokes: AtomicUsize,
    exchanges: AtomicUsize,
    chunks: AtomicUsize,
    container_refs: Mutex<Vec<Option<String>>>,
}

/// Holds live publishes just before they land, until the test releases them. Made by
/// [`MockScript::hold_publishes`]; clones share one gate.
#[derive(Clone)]
pub struct PublishGate(Arc<GateState>);

struct GateState {
    holding: AtomicBool,
    held: AtomicUsize,
    open: tokio::sync::watch::Sender<bool>,
}

impl PublishGate {
    fn new() -> Self {
        Self(Arc::new(GateState {
            holding: AtomicBool::new(true),
            held: AtomicUsize::new(0),
            open: tokio::sync::watch::Sender::new(false),
        }))
    }

    /// How many publishes reached the gate while it was holding.
    #[must_use]
    pub fn held(&self) -> usize {
        self.0.held.load(Ordering::SeqCst)
    }

    /// Waits until `n` publishes are held, or `timeout` passes. Returns whether they are.
    pub async fn wait_for_held(&self, n: usize, timeout: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + timeout;
        while self.held() < n {
            if tokio::time::Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        true
    }

    /// Lets publishes that arrive from now on pass; the ones already held keep waiting.
    pub fn stop_holding(&self) {
        self.0.holding.store(false, Ordering::SeqCst);
    }

    /// Lets every held publish land, and stops holding new ones.
    pub fn release(&self) {
        self.stop_holding();
        self.0.open.send_replace(true);
    }

    /// Waits here while the gate holds publishes.
    async fn pass(&self) {
        if !self.0.holding.load(Ordering::SeqCst) {
            return;
        }
        let mut open = self.0.open.subscribe();
        self.0.held.fetch_add(1, Ordering::SeqCst);
        // The sender lives as long as the gate, so this only ends once released.
        let _ = open.wait_for(|open| *open).await;
    }
}

/// A cloneable handle to one mock agent's platform and overrides. Every clone, and every
/// client the agent creates, sees the same state.
#[derive(Clone, Default)]
pub struct MockScript(pub(crate) Arc<Shared>);

impl MockScript {
    fn overrides(&self) -> MutexGuard<'_, Overrides> {
        self.0
            .overrides
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Makes `verify_credentials` follow `script` instead of the token keywords.
    pub fn set_verify(&self, script: VerifyScript) {
        self.overrides().verify = Some(script);
    }

    /// Makes `account_info` follow `script` instead of the token keywords.
    pub fn set_account_info(&self, script: InfoScript) {
        self.overrides().account_info = Some(script);
    }

    /// Makes `after_exchange` follow `script` instead of the default.
    pub fn set_exchange(&self, script: ExchangeScript) {
        self.overrides().exchange = Some(script);
    }

    /// How many times `after_exchange` ran.
    #[must_use]
    pub fn exchange_calls(&self) -> usize {
        self.0.exchanges.load(Ordering::SeqCst)
    }

    /// Makes OAuth discovery return `n` accounts, `oauth-1` to `oauth-n`.
    pub fn set_oauth_accounts(&self, n: usize) {
        self.overrides().oauth_accounts = Some(n);
    }

    /// Makes OAuth discovery follow `script` (a refresh, a delay, or a failure).
    pub fn set_oauth_discovery(&self, script: DiscoveryScript) {
        self.overrides().discovery = script;
    }

    /// Makes every live publish upload `steps` chunks before it creates the post. After each
    /// chunk it saves the container reference `chunks:<done>` through the progress, and between
    /// chunks (and before the final create) it returns `Cancelled` when the progress asks to
    /// stop. A publish given `chunks:<n>` resumes after chunk `n`. `0` (the default) publishes
    /// in one step.
    pub fn set_publish_steps(&self, steps: usize) {
        self.overrides().publish_steps = steps;
    }

    /// While `keep`, `status` answers `Processing` for a processing post instead of making it
    /// live.
    pub fn keep_processing(&self, keep: bool) {
        self.overrides().keep_processing = keep;
    }

    /// While `fail`, `reconcile` and `reconcile_comment` fail with a transport error (after
    /// the platform counts the call), unless the `reconcile` option is `unsupported`.
    pub fn fail_reconcile(&self, fail: bool) {
        self.overrides().fail_reconcile = fail;
    }

    /// While set, `status`, `reconcile`, and `reconcile_comment` answer `RateLimited` with this
    /// delay (after the platform counts the call), unless the `reconcile` option is
    /// `unsupported`.
    pub fn set_retry_after(&self, after: Option<Duration>) {
        self.overrides().retry_after = after;
    }

    /// Holds every live publish just before it lands, and every direct batch before it sends,
    /// until the returned gate lets it go.
    #[must_use]
    pub fn hold_publishes(&self) -> PublishGate {
        let gate = PublishGate::new();
        self.overrides().gate = Some(gate.clone());
        gate
    }

    /// Holds every live multi-step publish right after it uploads chunk number `after` and
    /// saves its container reference, before it checks whether to stop, until the returned
    /// gate lets it go. Chunks count from one. Tests use it to stop a publish between chunks.
    #[must_use]
    pub fn hold_after_chunk(&self, after: usize) -> PublishGate {
        let gate = PublishGate::new();
        self.overrides().chunk_gate = Some((after, gate.clone()));
        gate
    }

    /// How many upload chunks live publishes sent.
    #[must_use]
    pub fn chunk_uploads(&self) -> usize {
        self.0.chunks.load(Ordering::SeqCst)
    }

    /// The container reference each live publish was given, in order.
    #[must_use]
    pub fn seen_container_refs(&self) -> Vec<Option<String>> {
        self.0
            .container_refs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Makes `revoke` fail (after counting the call) while `fail` is true.
    pub fn fail_revoke(&self, fail: bool) {
        self.overrides().fail_revoke = fail;
    }

    /// How many times a client's `revoke` ran.
    #[must_use]
    pub fn revoke_calls(&self) -> usize {
        self.0.revokes.load(Ordering::SeqCst)
    }

    /// The `remote_id` of every account `account_info` was asked about, in order.
    #[must_use]
    pub fn seen_remote_ids(&self) -> Vec<String> {
        self.0
            .seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Requests the mock platform has received.
    #[must_use]
    pub fn remote_calls(&self) -> usize {
        self.0.platform.remote_calls()
    }

    /// Received requests that could change the mock platform.
    #[must_use]
    pub fn mutating_calls(&self) -> usize {
        self.0.platform.mutating_calls()
    }

    /// Every post on the mock platform, in creation order.
    #[must_use]
    pub fn posts(&self) -> Vec<MockPost> {
        self.0.platform.posts()
    }

    /// Every comment on the mock platform, in creation order.
    #[must_use]
    pub fn comments(&self) -> Vec<MockComment> {
        self.0.platform.comments()
    }

    /// Every direct send the mock platform accepted, in order.
    #[must_use]
    pub fn sends(&self) -> Vec<MockSend> {
        self.0.platform.sends()
    }

    /// Adds a follower's comment to the post `remote_post_id`, as if it had just arrived, so
    /// the next inbox poll returns it. Its author is `@fan{i}`, `i` counting the post's comments.
    /// Returns the new comment's ID, or `None` when the platform has no such post.
    #[must_use = "None means the post does not exist"]
    pub fn push_comment(&self, remote_post_id: &str, text: &str) -> Option<String> {
        self.0.platform.push_comment(remote_post_id, text)
    }

    /// Counts an exchange and returns the script it follows.
    pub(crate) fn record_exchange(&self) -> ExchangeScript {
        self.0.exchanges.fetch_add(1, Ordering::SeqCst);
        self.overrides().exchange.clone().unwrap_or_default()
    }

    /// The accounts OAuth discovery reaches.
    pub(crate) fn oauth_accounts(&self) -> Vec<DiscoveredAccount> {
        match self.overrides().oauth_accounts {
            None => vec![crate::credentials::account("oauth")],
            Some(n) => (1..=n)
                .map(|i| crate::credentials::account(&format!("oauth-{i}")))
                .collect(),
        }
    }

    /// What OAuth discovery does besides naming the accounts.
    pub(crate) fn oauth_discovery(&self) -> DiscoveryScript {
        self.overrides().discovery.clone()
    }

    pub(crate) fn platform(&self) -> &MockPlatform {
        &self.0.platform
    }

    pub(crate) fn verify(&self) -> Option<VerifyScript> {
        self.overrides().verify.clone()
    }

    pub(crate) fn account_info(&self) -> Option<InfoScript> {
        self.overrides().account_info
    }

    pub(crate) fn forced(&self) -> Option<Forced> {
        self.overrides().forced
    }

    #[cfg(feature = "testkit")]
    fn force(&self, forced: Option<Forced>) {
        self.overrides().forced = forced;
    }

    pub(crate) fn keeps_processing(&self) -> bool {
        self.overrides().keep_processing
    }

    pub(crate) fn retry_after(&self) -> Option<Duration> {
        self.overrides().retry_after
    }

    pub(crate) fn fails_reconcile(&self) -> bool {
        self.overrides().fail_reconcile
    }

    pub(crate) fn publish_steps(&self) -> usize {
        self.overrides().publish_steps
    }

    /// Waits at the publish gate, if one is holding.
    pub(crate) async fn pass_gate(&self) {
        let gate = self.overrides().gate.clone();
        if let Some(gate) = gate {
            gate.pass().await;
        }
    }

    /// Waits at the chunk gate when `uploaded` chunks are done and it holds after that many.
    pub(crate) async fn pass_chunk_gate(&self, uploaded: usize) {
        let gate = self
            .overrides()
            .chunk_gate
            .as_ref()
            .filter(|(after, _)| *after == uploaded)
            .map(|(_, gate)| gate.clone());
        if let Some(gate) = gate {
            gate.pass().await;
        }
    }

    pub(crate) fn record_chunk(&self) {
        self.0.chunks.fetch_add(1, Ordering::SeqCst);
    }

    pub(crate) fn record_container_ref(&self, container_ref: Option<&str>) {
        self.0
            .container_refs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(container_ref.map(str::to_owned));
    }

    pub(crate) fn record_seen(&self, remote_id: &str) {
        self.0
            .seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(remote_id.to_owned());
    }

    /// Counts a revoke and says whether it should fail.
    pub(crate) fn record_revoke(&self) -> bool {
        self.0.revokes.fetch_add(1, Ordering::SeqCst);
        self.overrides().fail_revoke
    }
}

#[cfg(feature = "testkit")]
mod faults {
    use async_trait::async_trait;
    use postit_agent_kit::testkit::conformance::{Fault, Faults};

    use super::{Forced, MockScript};

    impl From<Fault> for Forced {
        fn from(fault: Fault) -> Self {
            match fault {
                Fault::Unauthorized => Self::Unauthorized,
                Fault::RateLimited(after) => Self::RateLimited(after),
                Fault::ServerError => Self::ServerError,
                Fault::AcceptedThenLost => Self::AcceptedThenLost,
                Fault::LostBeforeAccept => Self::LostBeforeAccept,
                Fault::Malformed => Self::Malformed,
            }
        }
    }

    /// Faults apply to every live publish until cleared; they win over the token keywords and
    /// the `outcome` option.
    #[async_trait]
    impl Faults for MockScript {
        async fn arm(&self, fault: Fault) {
            self.force(Some(fault.into()));
        }

        async fn clear(&self) {
            self.force(None);
        }

        fn remote_calls(&self) -> usize {
            MockScript::remote_calls(self)
        }

        fn mutating_calls(&self) -> usize {
            MockScript::mutating_calls(self)
        }
    }
}
