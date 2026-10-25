//! [`MockScript`]: a Rust handle that overrides what the pasted values and post options would
//! do, counts calls, and (with the `testkit` feature) is the conformance suite's `Faults`.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

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

#[derive(Default)]
struct Overrides {
    verify: Option<VerifyScript>,
    account_info: Option<InfoScript>,
    forced: Option<Forced>,
    fail_revoke: bool,
}

/// The state one agent and all its clients share.
#[derive(Default)]
pub(crate) struct Shared {
    pub(crate) platform: MockPlatform,
    overrides: Mutex<Overrides>,
    seen: Mutex<Vec<String>>,
    revokes: AtomicUsize,
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
