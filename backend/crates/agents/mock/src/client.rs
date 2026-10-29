//! [`MockClient`]: the mock bound to one grant. It implements the required `AgentClient`
//! calls; each optional capability lives in its own module and is exposed through the
//! accessor of the same name.
//!
//! Every call that would reach a real platform goes through `MockPlatform::call` first, so
//! the call counters see exactly what a wiremock server would.

use std::collections::BTreeSet;

use async_trait::async_trait;
use chrono::{DateTime, TimeDelta, Utc};
use postit_agent_kit::message;
use postit_core::{
    AccountInfo, AccountRef, AccountStatus, AgentClient, AgentContext, AgentError, Capability,
    Commenter, Credentials, DirectSender, Discovered, InboxReader, MediaAsset, MetricsReader,
    NativeScheduler, OAuthTokens, OptionsSchema, PlatformId, PublishMode, PublishOutcome,
    PublishProgress, PublishReceipt, PublishRequest, ReconcileOutcome, ReconcileRequest, Variant,
    Violation,
};

use secrecy::SecretString;

use crate::credentials::{self, ACCOUNT_PREFIX, RATE_LIMIT_DELAY, Token};
use crate::descriptor::{MAX_SCHEDULE_LEAD, MIN_SCHEDULE_LEAD, POSTS_BUCKET};
use crate::options::{Outcome, PostOptions, ReconcileAnswer};
use crate::platform::{Call, MockPlatform, NewPost, State};
use crate::script::{Forced, InfoScript, MockScript};
use postit_core::DeliveryId;

/// The mock agent bound to one grant.
pub struct MockClient {
    pub(crate) platform_id: PlatformId,
    pub(crate) options: OptionsSchema,
    pub(crate) script: MockScript,
    pub(crate) ctx: AgentContext,
    /// The descriptor's capabilities: the accessors expose exactly these.
    pub(crate) capabilities: BTreeSet<Capability>,
}

/// What the mock platform does with one live publish or comment.
pub(crate) enum Reaction {
    /// Refused before acting: nothing is stored, and a repeated key does not matter.
    Refuse(AgentError),
    /// Accepted and stored; `live` says whether it is live yet.
    Store { live: bool },
    /// Stored, then the response is lost.
    StoreThenLose(&'static str),
    /// Lost before the platform acted: nothing is stored.
    Lose(AgentError),
}

impl MockClient {
    pub(crate) fn platform(&self) -> &MockPlatform {
        self.script.platform()
    }

    pub(crate) fn now(&self) -> DateTime<Utc> {
        self.ctx.clock.now()
    }

    pub(crate) async fn token(&self) -> Result<Token, AgentError> {
        Ok(Token::of_grant(&self.ctx.credentials.credentials().await?))
    }

    /// Whether the descriptor declares `capability`, so its accessor exposes it.
    fn offers(&self, capability: Capability) -> bool {
        self.capabilities.contains(&capability)
    }

    /// A `Validation` error carrying one agent rule keyed `<platform>.<key>`.
    pub(crate) fn violation(&self, key: &str, en: &str) -> AgentError {
        AgentError::Validation(vec![Violation::agent_rule(message(
            &self.platform_id,
            key,
            en,
        ))])
    }

    /// A forced fault wins over the token, and the token over the `outcome` option.
    pub(crate) fn reaction(&self, token: Token, options: PostOptions) -> Reaction {
        let unknown = |message: &str| AgentError::OutcomeUnknown {
            message: message.to_owned(),
        };
        if let Some(forced) = self.script.forced() {
            return match forced {
                Forced::Unauthorized => Reaction::Refuse(credentials::rejected_token()),
                Forced::RateLimited(retry_after) => {
                    Reaction::Refuse(AgentError::RateLimited { retry_after })
                }
                Forced::ServerError => Reaction::Lose(
                    AgentError::Remote {
                        status: Some(500),
                        code: None,
                        message: "the mock platform failed".to_owned(),
                    }
                    .after_send(),
                ),
                Forced::AcceptedThenLost => {
                    Reaction::StoreThenLose("the mock platform's response was lost")
                }
                Forced::LostBeforeAccept => {
                    Reaction::Lose(unknown("the request to the mock platform was lost"))
                }
                Forced::Malformed => {
                    Reaction::StoreThenLose("the mock platform's response could not be read")
                }
            };
        }
        if token.rejected() {
            return Reaction::Refuse(credentials::rejected_token());
        }
        match options.outcome {
            Outcome::Published => Reaction::Store { live: true },
            Outcome::Processing => Reaction::Store { live: false },
            Outcome::RetryableFailure => Reaction::Refuse(AgentError::Remote {
                status: Some(503),
                code: None,
                message: "the mock platform is unavailable".to_owned(),
            }),
            Outcome::PermanentFailure => Reaction::Refuse(AgentError::Remote {
                status: Some(400),
                code: Some("rejected".to_owned()),
                message: "the mock platform refused the post".to_owned(),
            }),
            Outcome::OutcomeUnknownLanded => {
                Reaction::StoreThenLose("the mock platform's response was lost")
            }
            Outcome::OutcomeUnknownLost => {
                Reaction::Lose(unknown("the request to the mock platform was lost"))
            }
            Outcome::NeedsReconnect => Reaction::Refuse(credentials::rejected_token()),
            Outcome::RateLimited => Reaction::Refuse(AgentError::RateLimited {
                retry_after: Some(RATE_LIMIT_DELAY),
            }),
            Outcome::QuotaExhausted => Reaction::Refuse(AgentError::Quota {
                bucket: POSTS_BUCKET.to_owned(),
                resets_at: next_midnight(self.now()),
            }),
        }
    }

    pub(crate) fn check_schedule(&self, at: Option<DateTime<Utc>>) -> Result<(), AgentError> {
        let Some(at) = at else { return Ok(()) };
        let lead = at - self.now();
        let min = TimeDelta::from_std(MIN_SCHEDULE_LEAD).ok();
        let max = TimeDelta::from_std(MAX_SCHEDULE_LEAD).ok();
        let fits = min.is_some_and(|min| lead >= min) && max.is_some_and(|max| lead <= max);
        if fits {
            Ok(())
        } else {
            Err(self.violation(
                "schedule_outside_window",
                "The mock platform schedules posts 10 minutes to 30 days ahead",
            ))
        }
    }

    /// One planned step per action a live publish would take.
    fn plan(request: &PublishRequest<'_>) -> Vec<String> {
        let media = request.variant.media.len();
        let mut steps: Vec<String> = (1..=media)
            .map(|i| format!("upload media item {i} of {media}"))
            .collect();
        steps.push(match request.native_publish_at {
            Some(at) => format!("schedule the post for {}", at.to_rfc3339()),
            None => "create the post".to_owned(),
        });
        steps
    }

    /// The shared live path for anything the mock stores under an idempotency key (posts and
    /// comments). A key the platform already answered returns that answer first, before any
    /// refusal, as a deduplicating platform would. The check and the store happen under one
    /// lock, so two clients sending the same key at once store one item. `store` gets whether
    /// the item is live yet.
    pub(crate) fn deliver(
        &self,
        reaction: Reaction,
        existing: impl FnOnce(&State) -> Option<PublishOutcome>,
        store: impl FnOnce(&mut State, bool) -> Result<PublishOutcome, AgentError>,
    ) -> Result<PublishOutcome, AgentError> {
        let mut state = self.platform().lock();
        if let Some(earlier) = existing(&state) {
            return Ok(earlier);
        }
        match reaction {
            Reaction::Refuse(err) | Reaction::Lose(err) => Err(err),
            Reaction::Store { live } => store(&mut state, live),
            Reaction::StoreThenLose(message) => {
                store(&mut state, true)?;
                Err(AgentError::OutcomeUnknown {
                    message: message.to_owned(),
                })
            }
        }
    }

    /// Uploads the chunks a multi-step publish has left (see
    /// [`MockScript::set_publish_steps`]), saving the container reference after each and
    /// stopping between chunks when the progress asks.
    async fn upload_chunks(
        &self,
        request: &PublishRequest<'_>,
        progress: &dyn PublishProgress,
    ) -> Result<(), AgentError> {
        self.script.record_container_ref(request.container_ref);
        let steps = self.script.publish_steps();
        let done = request
            .container_ref
            .and_then(|r| r.strip_prefix(CHUNKS_PREFIX))
            .and_then(|n| n.parse::<usize>().ok())
            .unwrap_or(0)
            .min(steps);
        for chunk in done..steps {
            if chunk > done && progress.cancel_requested() {
                return Err(AgentError::Cancelled);
            }
            self.platform().call(Call::Mutating).await;
            self.script.record_chunk();
            progress
                .save_container_ref(&format!("{CHUNKS_PREFIX}{}", chunk + 1))
                .await?;
            self.script.pass_chunk_gate(chunk + 1).await;
        }
        Ok(())
    }

    /// `mock_direct` sends email: a variant without a subject is refused.
    fn email_violations(&self, variant: &Variant) -> Vec<Violation> {
        let has_subject = variant
            .email
            .as_ref()
            .is_some_and(|e| !e.subject.trim().is_empty());
        if !self.offers(Capability::DirectSender) || has_subject {
            return Vec::new();
        }
        vec![
            Violation::agent_rule(message(
                &self.platform_id,
                "email_subject_missing",
                "The email needs a subject",
            ))
            .at("email.subject"),
        ]
    }

    /// The shared reconcile path for posts and comments: a scripted failure, then the
    /// variant's `reconcile` option, and for `found` the item `lookup` finds under `key`.
    pub(crate) async fn answer_reconcile(
        &self,
        variant: &Variant,
        key: DeliveryId,
        lookup: impl FnOnce(&State, DeliveryId) -> Option<PublishReceipt>,
    ) -> Result<ReconcileOutcome, AgentError> {
        let options = PostOptions::parse(&self.platform_id, &variant.options)
            .map_err(AgentError::Validation)?;
        if options.reconcile == ReconcileAnswer::Unsupported {
            // The platform offers no lookup, so nothing is called. `Capability` has no variant
            // for publishing itself; as the agent contract says, callers match on
            // `Unsupported` and ignore the capability, so posts carry the closest one too.
            return Err(AgentError::Unsupported {
                capability: Capability::Commenter,
            });
        }
        self.platform().call(Call::Read).await;
        if let Some(after) = self.script.retry_after() {
            return Err(AgentError::RateLimited {
                retry_after: Some(after),
            });
        }
        if self.script.fails_reconcile() {
            return Err(AgentError::Transport {
                message: "the mock platform's lookup timed out".to_owned(),
            });
        }
        Ok(match options.reconcile {
            ReconcileAnswer::Found => lookup(&self.platform().lock(), key)
                .map_or(ReconcileOutcome::Absent, ReconcileOutcome::Found),
            ReconcileAnswer::Absent => ReconcileOutcome::Absent,
            ReconcileAnswer::Unresolvable | ReconcileAnswer::Unsupported => {
                ReconcileOutcome::Unresolvable
            }
        })
    }

    /// Every violation `validate` reports for `variant`.
    pub(crate) fn violations(&self, variant: &Variant) -> Vec<Violation> {
        let mut out = PostOptions::parse(&self.platform_id, &variant.options)
            .err()
            .unwrap_or_default();
        out.extend(self.email_violations(variant));
        out
    }
}

/// The container reference of a multi-step publish: `chunks:<uploaded so far>`.
const CHUNKS_PREFIX: &str = "chunks:";

/// The next midnight UTC after `now`, when the daily `posts` bucket resets.
fn next_midnight(now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    now.date_naive()
        .succ_opt()
        .and_then(|day| day.and_hms_opt(0, 0, 0))
        .map(|midnight| midnight.and_utc())
}

#[async_trait]
impl AgentClient for MockClient {
    async fn account_info(&self, account: &AccountRef) -> Result<AccountInfo, AgentError> {
        self.platform().call(Call::Read).await;
        self.script.record_seen(&account.remote_id);
        let scripted = self.script.account_info();
        let active = match scripted {
            Some(InfoScript::Ok | InfoScript::NeedsReconnect) => true,
            Some(InfoScript::AuthFailed) => false,
            None => !self.token().await?.rejected(),
        };
        if !active {
            return Err(credentials::rejected_token());
        }
        let name = account
            .remote_id
            .strip_prefix(ACCOUNT_PREFIX)
            .unwrap_or(&account.remote_id);
        Ok(AccountInfo {
            remote_id: account.remote_id.clone(),
            display_name: name.to_owned(),
            handle: Some(format!("@{name}")),
            avatar_url: None,
            status: if scripted == Some(InfoScript::NeedsReconnect) {
                AccountStatus::NeedsReconnect
            } else {
                AccountStatus::Active
            },
        })
    }

    async fn discover_accounts(&self) -> Result<Discovered, AgentError> {
        self.platform().call(Call::Read).await;
        match self.ctx.credentials.credentials().await? {
            Credentials::Fields(fields) => credentials::discover(&fields),
            Credentials::OAuth(tokens) => {
                let script = self.script.oauth_discovery();
                if let Some(access_token) = script.refresh_to {
                    let refreshed = OAuthTokens {
                        access_token: SecretString::from(access_token),
                        ..tokens
                    };
                    self.ctx.credentials.store_refreshed(refreshed).await?;
                }
                if let Some(delay) = script.delay {
                    tokio::time::sleep(delay).await;
                }
                if script.fail {
                    return Err(AgentError::Transport {
                        message: "the mock platform failed discovery".to_owned(),
                    });
                }
                Ok(Discovered {
                    accounts: self.script.oauth_accounts(),
                })
            }
        }
    }

    async fn publish_options(&self, _account: &AccountRef) -> Result<OptionsSchema, AgentError> {
        Ok(self.options.clone())
    }

    async fn validate(
        &self,
        variant: &Variant,
        _media: &[MediaAsset],
    ) -> Result<Vec<Violation>, AgentError> {
        Ok(self.violations(variant))
    }

    async fn publish(
        &self,
        request: PublishRequest<'_>,
        progress: &dyn PublishProgress,
    ) -> Result<PublishOutcome, AgentError> {
        let options = PostOptions::parse(&self.platform_id, &request.variant.options)
            .map_err(AgentError::Validation)?;
        let publish_at = request.native_publish_at;
        if request.mode == PublishMode::DryRun {
            self.check_schedule(publish_at)?;
            return Ok(PublishOutcome::Simulated {
                planned: Self::plan(&request),
            });
        }
        if progress.cancel_requested() {
            return Err(AgentError::Cancelled);
        }
        let token = self.token().await?;
        self.upload_chunks(&request, progress).await?;
        self.script.pass_gate().await;
        if self.script.publish_steps() > 0 && progress.cancel_requested() {
            return Err(AgentError::Cancelled);
        }
        self.platform().call(Call::Mutating).await;
        // The window is checked after the key, so a retry of an accepted schedule whose lead
        // time has since shrunk still gets its receipt.
        let reaction = match self.check_schedule(publish_at) {
            Ok(()) => self.reaction(token, options),
            Err(err) => Reaction::Refuse(err),
        };
        let key = request.idempotency_key;
        let variant = request.variant;
        let now = self.now();
        self.deliver(
            reaction,
            |state| state.existing(key),
            |state, live| {
                let new = NewPost {
                    account: &variant.target.remote_id,
                    text: &variant.text,
                    // A natively scheduled post goes live at `publish_at`, not now.
                    live: live && publish_at.is_none(),
                    publish_at,
                    comments: options.comments,
                    now,
                };
                Ok(state.create_post(key, &new))
            },
        )
    }

    async fn status(
        &self,
        account: &AccountRef,
        receipt: &PublishReceipt,
    ) -> Result<PublishOutcome, AgentError> {
        self.platform().call(Call::Read).await;
        if let Some(after) = self.script.retry_after() {
            return Err(AgentError::RateLimited {
                retry_after: Some(after),
            });
        }
        let now = self.now();
        let mut state = self.platform().lock();
        let Some(post) = receipt
            .remote_id
            .as_deref()
            .and_then(|id| state.post_mut(id))
            .filter(|p| p.account == account.remote_id)
        else {
            return Err(crate::capabilities::not_found());
        };
        // A processing post goes live when asked about (unless the script keeps it
        // processing); a scheduled one once its time passes.
        post.live = match post.publish_at {
            None => post.live || !self.script.keeps_processing(),
            Some(_) => post.live_at(now),
        };
        let answer = crate::platform::outcome(post);
        state.release_seeds(now);
        Ok(answer)
    }

    async fn reconcile(
        &self,
        request: ReconcileRequest<'_>,
    ) -> Result<ReconcileOutcome, AgentError> {
        self.answer_reconcile(request.variant, request.idempotency_key, |state, key| {
            state.receipt_for(key)
        })
        .await
    }

    fn commenter(&self) -> Option<&dyn Commenter> {
        self.offers(Capability::Commenter).then_some(self)
    }

    fn inbox(&self) -> Option<&dyn InboxReader> {
        self.offers(Capability::InboxReader).then_some(self)
    }

    fn metrics(&self) -> Option<&dyn MetricsReader> {
        self.offers(Capability::MetricsReader).then_some(self)
    }

    fn direct(&self) -> Option<&dyn DirectSender> {
        self.offers(Capability::DirectSender).then_some(self)
    }

    fn native_scheduler(&self) -> Option<&dyn NativeScheduler> {
        self.offers(Capability::NativeScheduler).then_some(self)
    }

    async fn revoke(&self) -> Result<(), AgentError> {
        self.platform().call(Call::Mutating).await;
        if self.script.record_revoke() {
            return Err(AgentError::Remote {
                status: Some(500),
                code: None,
                message: "the mock platform failed to revoke the grant".to_owned(),
            });
        }
        Ok(())
    }
}
