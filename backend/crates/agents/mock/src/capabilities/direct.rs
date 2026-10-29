//! `DirectSender` (`mock_direct`): each recipient's address picks its result.
//!
//! Compared in ASCII lowercase, an address containing `soft-bounce` bounces temporarily, one
//! containing `bounce` bounces permanently, one containing `fail` fails retryably, and any
//! other is sent as `mock-send-{send_id}`. A send ID already sent returns its first result
//! without sending again. Reasons never repeat the address.
//!
//! The whole batch fails when the variant has no email subject or the platform rejects the
//! token. Conformance faults apply to publishes and comments, not to batches; the publish gate
//! ([`crate::MockScript::hold_publishes`]) holds batches too, before any result is decided.

use async_trait::async_trait;
use postit_core::{
    AccountRef, AgentError, DirectRecipient, DirectSendOutcome, DirectSendResult, DirectSender,
    Variant,
};

use crate::client::MockClient;
use crate::credentials;
use crate::platform::{Call, MockSend, State};

fn outcome(state: &mut State, recipient: &DirectRecipient) -> DirectSendOutcome {
    let address = recipient.address.to_ascii_lowercase();
    if address.contains("soft-bounce") {
        DirectSendOutcome::Bounced {
            permanent: false,
            reason: "the mailbox is full; try again later".to_owned(),
        }
    } else if address.contains("bounce") {
        DirectSendOutcome::Bounced {
            permanent: true,
            reason: "the address does not exist".to_owned(),
        }
    } else if address.contains("fail") {
        DirectSendOutcome::Failed {
            retryable: true,
            reason: "the mock platform could not send the message".to_owned(),
        }
    } else {
        let send_id = recipient.send_id;
        if !state.sends.iter().any(|s| s.send_id == send_id) {
            state.sends.push(MockSend {
                send_id,
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
    ) -> Result<Vec<DirectSendResult>, AgentError> {
        let violations = self.violations(variant);
        if !violations.is_empty() {
            return Err(AgentError::Validation(violations));
        }
        let token = self.token().await?;
        self.platform().call(Call::Mutating).await;
        if token.rejected() {
            return Err(credentials::rejected_token());
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
