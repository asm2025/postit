//! `DirectSender` (`mock_direct`): each recipient's address picks its result.
//!
//! Compared in ASCII lowercase, an address containing `soft-bounce` bounces temporarily, one
//! containing `bounce` bounces permanently, one containing `fail` fails retryably, and any
//! other is sent as `mock-send-{send_id}`. A send ID already sent returns its first result
//! without sending again, except [`TEST_SEND_ID`], which every test email shares: each of
//! those is a new message and is recorded with its own address. Reasons are short codes
//! (`mock_soft_bounce`, `mock_bounce`, `mock_send_failed`) and never repeat the address. Each
//! call records the IDs of the assets it was given ([`crate::MockScript::batch_assets`]),
//! before anything else is checked.
//!
//! The whole batch fails when the variant has no email subject or the platform rejects the
//! token. Conformance faults apply to publishes and comments, not to batches; the publish gate
//! ([`crate::MockScript::hold_publishes`]) holds batches too, before any result is decided.

use async_trait::async_trait;
use postit_core::{
    AccountRef, AgentError, DirectRecipient, DirectSendOutcome, DirectSendResult, DirectSender,
    MediaAsset, TEST_SEND_ID, Variant,
};

use crate::client::MockClient;
use crate::credentials;
use crate::platform::{Call, MockSend, State};

fn outcome(state: &mut State, recipient: &DirectRecipient) -> DirectSendOutcome {
    let address = recipient.address.to_ascii_lowercase();
    if address.contains("soft-bounce") {
        DirectSendOutcome::Bounced {
            permanent: false,
            reason: "mock_soft_bounce".to_owned(),
        }
    } else if address.contains("bounce") {
        DirectSendOutcome::Bounced {
            permanent: true,
            reason: "mock_bounce".to_owned(),
        }
    } else if address.contains("fail") {
        DirectSendOutcome::Failed {
            retryable: true,
            reason: "mock_send_failed".to_owned(),
        }
    } else {
        let send_id = recipient.send_id;
        if send_id == TEST_SEND_ID || !state.sends.iter().any(|s| s.send_id == send_id) {
            state.sends.push(MockSend {
                send_id,
                address: recipient.address.clone(),
                remote_id: format!("mock-send-{send_id}"),
            });
        }
        DirectSendOutcome::Sent {
            remote_id: Some(format!("mock-send-{send_id}")),
        }
    }
}

#[async_trait]
impl DirectSender for MockClient {
    async fn send_batch(
        &self,
        _account: &AccountRef,
        recipients: &[DirectRecipient],
        variant: &Variant,
        assets: &[MediaAsset],
    ) -> Result<Vec<DirectSendResult>, AgentError> {
        self.script.record_batch_assets(assets);
        let violations = self.violations(variant);
        if !violations.is_empty() {
            return Err(AgentError::Validation(violations));
        }
        let token = self.token().await?;
        self.platform().call(Call::Mutating).await;
        if token.rejected() {
            return Err(credentials::rejected_token());
        }
        // A provider that is busy for now refuses the whole batch, nothing sent.
        if let Some(after) = self.script.retry_after() {
            return Err(AgentError::RateLimited {
                retry_after: Some(after),
            });
        }
        self.script.pass_gate().await;
        let mut state = self.platform().lock();
        Ok(recipients
            .iter()
            .map(|recipient| DirectSendResult {
                send_id: recipient.send_id,
                outcome: outcome(&mut state, recipient),
            })
            .collect())
    }
}
