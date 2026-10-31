//! [`EmailClient`]: the email agent bound to one grant, and its `send_batch`.
//!
//! A batch is sent sequentially on one session. Per recipient:
//!
//! - accepted: `Sent`, with the `Message-ID` as its remote ID;
//! - refused at `RCPT` or `DATA`: classified by [`crate::classify`];
//! - a `421` or a broken connection: the session reopens, at most [`MAX_REOPENS`] times per
//!   batch. A recipient whose message was not yet written is sent once more on the new
//!   session, since the server cannot have accepted it; one lost during or after the message
//!   is `Failed { retryable: true }` and never sent again in the batch. Once the reopens are
//!   spent, every recipient left is `Failed { retryable: true }`;
//! - a 5xx at `MAIL` refuses the sender: `AgentError::Authentication` when nothing in the
//!   batch was accepted, otherwise that recipient and the rest are `Failed { retryable: true }`;
//! - the first [`LEADING_SENDER_REJECTIONS`] recipients all refused with `5.7.x` and nothing
//!   accepted: `AgentError::Authentication`, because the relay is refusing the sender.
//!
//! A failure to open the first session fails the batch as a whole. Reasons are fixed codes,
//! never server text.
//!
//! The client is also the account's [`SenderDomainCheck`], over [`crate::dns`].

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use lettre::message::header::{HeaderName, HeaderValue};
use lettre::message::{Mailbox, MultiPart};
use lettre::{Address, Message};
use postit_core::{
    AccountInfo, AccountRef, AccountStatus, AgentClient, AgentContext, AgentError, Capability,
    DirectRecipient, DirectSendOutcome, DirectSendResult, DirectSender, Discovered,
    DiscoveredAccount, DomainFinding, EmailBlock, EmailDocument, MediaAsset, MediaAssetId,
    MediaError, OptionsSchema, PublishMode, PublishOutcome, PublishProgress, PublishReceipt,
    PublishRequest, ReconcileOutcome, ReconcileRequest, SenderDomainCheck, TEST_SEND_ID, Variant,
    Violation, ViolationCode, is_dkim_selector,
};
use url::Url;
use uuid::Uuid;

use crate::classify::{Classified, classify};
use crate::credentials::{self, Account};
use crate::descriptor::MAX_HTML_BYTES;
use crate::render::{RenderError, RenderedMail, Sender, Skeleton};
use crate::smtp::SmtpTransport;
use crate::transport::{
    EmailSession, EmailTransport, OutgoingMail, SendFailure, SentMail, TransportError,
};
use crate::{Shared, message};

/// Session reopens allowed per batch after a dropped connection.
pub const MAX_REOPENS: usize = 2;

/// Leading `5.7.x` refusals, with nothing accepted, that mean the relay refuses the sender.
pub const LEADING_SENDER_REJECTIONS: usize = 3;

/// The email agent bound to one grant.
pub struct EmailClient {
    pub(crate) shared: Arc<Shared>,
    pub(crate) ctx: AgentContext,
}

/// The batch error for a session that could not be opened.
pub(crate) fn open_error(err: TransportError) -> AgentError {
    match err {
        TransportError::AddressRejected => AgentError::InvalidCredentials {
            field: Some("host".to_owned()),
        },
        TransportError::PlainRefused | TransportError::StarttlsMissing => {
            AgentError::InvalidCredentials {
                field: Some("tls".to_owned()),
            }
        }
        TransportError::Authentication => AgentError::Authentication {
            message: err.to_string(),
        },
        TransportError::Lookup
        | TransportError::Connect
        | TransportError::Timeout
        | TransportError::Tls
        | TransportError::Protocol
        | TransportError::Closing => AgentError::Transport {
            message: err.to_string(),
        },
    }
}

/// What a recipient's mail failed with before reaching the server.
fn failed(retryable: bool, reason: &str) -> DirectSendOutcome {
    DirectSendOutcome::Failed {
        retryable,
        reason: reason.to_owned(),
    }
}

fn sender_refused() -> AgentError {
    AgentError::Authentication {
        message: "the mail server refused the sender".to_owned(),
    }
}

/// The violation a render error stands for.
fn render_violation(err: &RenderError) -> Violation {
    match err {
        RenderError::MergeTag { path, .. } | RenderError::MergeTagInUrl { path } => {
            Violation::new(ViolationCode::EmailMergeTagInvalid).at(path.clone())
        }
        RenderError::UrlScheme { path } => Violation::agent_rule(message(
            "link_scheme_not_allowed",
            "Links must start with http, https, mailto, or tel",
        ))
        .at(path.clone()),
        RenderError::ImageMissing { path } => {
            Violation::new(ViolationCode::MediaMissing).at(path.clone())
        }
        RenderError::PostalAddressMissing => {
            Violation::new(ViolationCode::EmailPostalAddressRequired)
        }
        RenderError::Compile => {
            Violation::agent_rule(message("compile_failed", "The email could not be built"))
                .at("email")
        }
    }
}

impl EmailClient {
    async fn account(&self) -> Result<Account, AgentError> {
        credentials::of_grant(&self.ctx.credentials.credentials().await?)
    }

    pub(crate) fn transport(&self, account: &Account) -> SmtpTransport {
        self.shared.transport(account)
    }

    /// The URL of each image the document embeds, signed for the image TTL, and a violation
    /// for each image it cannot serve.
    fn images(
        &self,
        doc: &EmailDocument,
        assets: &[MediaAsset],
    ) -> (HashMap<MediaAssetId, Url>, Vec<Violation>) {
        let mut urls = HashMap::new();
        let mut violations = Vec::new();
        for (i, block) in doc.blocks.iter().enumerate() {
            let EmailBlock::Image { asset: id, .. } = block else {
                continue;
            };
            if urls.contains_key(id) {
                continue;
            }
            let path = format!("email.blocks[{i}]");
            match assets.iter().find(|a| a.id == *id) {
                None => violations.push(
                    Violation::new(ViolationCode::MediaMissing)
                        .at(path)
                        .param("id", id.to_string()),
                ),
                Some(asset) => {
                    match self
                        .ctx
                        .media_host
                        .public_url(asset, self.shared.config.image_url_ttl)
                    {
                        Ok(url) => {
                            urls.insert(*id, url);
                        }
                        Err(MediaError::NotFound) => violations.push(
                            Violation::new(ViolationCode::MediaMissing)
                                .at(path)
                                .param("id", id.to_string()),
                        ),
                        Err(_) => violations.push(
                            Violation::new(ViolationCode::MediaNotReady)
                                .at(path)
                                .param("id", id.to_string()),
                        ),
                    }
                }
            }
        }
        (urls, violations)
    }

    /// Compiles the document, or every violation found. A missing postal address is
    /// reported and the rest is still checked.
    fn compile(
        &self,
        doc: &EmailDocument,
        assets: &[MediaAsset],
        sender: &Sender,
    ) -> Result<Skeleton, Vec<Violation>> {
        let (images, mut violations) = self.images(doc, assets);
        if !violations.is_empty() {
            return Err(violations);
        }
        let mut checked = sender.clone();
        if !credentials::has_postal_address(sender) {
            violations.push(Violation::new(ViolationCode::EmailPostalAddressRequired));
            "-".clone_into(&mut checked.postal_address);
        }
        match Skeleton::compile(doc, &images, &checked) {
            Ok(skeleton) if violations.is_empty() => Ok(skeleton),
            Ok(_) => Err(violations),
            Err(err) => {
                violations.push(render_violation(&err));
                Err(violations)
            }
        }
    }

    fn violations(&self, variant: &Variant) -> Vec<Violation> {
        let mut out = self
            .shared
            .descriptor
            .options_schema
            .validate(&variant.options);
        if variant.email.is_none() {
            out.push(Violation::new(ViolationCode::EmailDocumentRequired).at("email"));
        }
        out
    }

    async fn send_all(
        &self,
        account: &Account,
        skeleton: &Skeleton,
        recipients: &[DirectRecipient],
        open_tracking: bool,
    ) -> Result<Vec<DirectSendResult>, AgentError> {
        let transport = self.transport(account);
        let mut session = Some(transport.open().await.map_err(open_error)?);
        let mut reopens = 0;
        let mut batch = Batch::new(recipients.len());
        let mut rest = recipients.iter();
        'batch: while let Some(recipient) = rest.next() {
            let rendered =
                skeleton.for_recipient(recipient, self.ctx.link_signer.as_ref(), open_tracking);
            let (mail, remote_id) = match outgoing(account, &rendered, recipient) {
                Ok(built) => built,
                Err(reason) => {
                    batch.leading = None;
                    batch.record(recipient, failed(false, reason));
                    continue;
                }
            };
            let mut retried = false;
            loop {
                if session.as_ref().is_none_or(|s| s.is_broken()) {
                    session = None;
                    while session.is_none() && reopens < MAX_REOPENS {
                        reopens += 1;
                        session = transport.open().await.ok();
                    }
                }
                let Some(open) = session.as_mut() else {
                    batch.record(recipient, failed(true, "smtp_connection"));
                    batch.fail_rest(rest, "smtp_connection");
                    break 'batch;
                };
                let sent = open.send(&mail).await;
                match batch.after_send(recipient, sent, &remote_id, retried) {
                    Next::Continue => break,
                    Next::Reopen => {
                        session = None;
                        break;
                    }
                    Next::Retry => {
                        session = None;
                        retried = true;
                    }
                    Next::SenderRefused if batch.accepted == 0 => {
                        close(session).await;
                        return Err(sender_refused());
                    }
                    Next::SenderRefused => {
                        batch.fail_rest(rest, "smtp_not_attempted");
                        break 'batch;
                    }
                }
            }
        }
        close(session).await;
        Ok(batch.results)
    }
}

/// What the batch does after one send.
enum Next {
    /// Go on with the next recipient.
    Continue,
    /// The session is gone: open another before the next recipient.
    Reopen,
    /// The session was lost before the message was written: send it again on a new one.
    Retry,
    /// The relay refuses the sender: stop.
    SenderRefused,
}

/// The results so far, and the state the batch rules read.
struct Batch {
    results: Vec<DirectSendResult>,
    /// Recipients the server accepted.
    accepted: usize,
    /// Leading `5.7.x` refusals, until any other result.
    leading: Option<usize>,
}

impl Batch {
    fn new(capacity: usize) -> Self {
        Self {
            results: Vec::with_capacity(capacity),
            accepted: 0,
            leading: Some(0),
        }
    }

    fn record(&mut self, recipient: &DirectRecipient, outcome: DirectSendOutcome) {
        self.results.push(DirectSendResult {
            send_id: recipient.send_id,
            outcome,
        });
    }

    fn fail_rest(&mut self, rest: std::slice::Iter<'_, DirectRecipient>, reason: &str) {
        for recipient in rest {
            self.record(recipient, failed(true, reason));
        }
    }

    /// Records what one send did and says what comes next. A session lost before the
    /// message was written cannot have delivered it, so the first such loss asks for a retry
    /// (`retried` says whether this was one); a loss during or after the message is final.
    fn after_send(
        &mut self,
        recipient: &DirectRecipient,
        sent: Result<SentMail, SendFailure>,
        remote_id: &str,
        retried: bool,
    ) -> Next {
        let refusal = match sent {
            Ok(SentMail) => {
                self.accepted += 1;
                self.leading = None;
                self.record(
                    recipient,
                    DirectSendOutcome::Sent {
                        remote_id: Some(remote_id.to_owned()),
                    },
                );
                return Next::Continue;
            }
            Err(SendFailure::Connection { error, body_sent }) => {
                self.leading = None;
                if !body_sent && !retried {
                    return Next::Retry;
                }
                let reason = match error {
                    TransportError::Closing => "smtp_421",
                    _ => "smtp_connection",
                };
                self.record(recipient, failed(true, reason));
                return Next::Reopen;
            }
            Err(SendFailure::Unsupported(reason)) => {
                self.leading = None;
                self.record(recipient, failed(false, reason));
                return Next::Continue;
            }
            Err(SendFailure::Refused(refusal)) => refusal,
        };
        let reason = refusal.reason();
        let outcome = match classify(&refusal) {
            Classified::Sender => {
                if self.accepted > 0 {
                    self.record(recipient, failed(true, &reason));
                }
                return Next::SenderRefused;
            }
            Classified::Bounce => DirectSendOutcome::Bounced {
                permanent: true,
                reason,
            },
            Classified::Soft => DirectSendOutcome::Bounced {
                permanent: false,
                reason,
            },
            Classified::Rejected => failed(false, &reason),
        };
        self.leading = self
            .leading
            .filter(|_| refusal.is_sender_policy())
            .map(|count| count + 1);
        self.record(recipient, outcome);
        if self.accepted == 0 && self.leading == Some(LEADING_SENDER_REJECTIONS) {
            return Next::SenderRefused;
        }
        Next::Continue
    }
}

async fn close(session: Option<Box<dyn EmailSession>>) {
    if let Some(mut session) = session {
        session.close().await;
    }
}

/// The mail for one recipient and its remote ID (the `Message-ID` without brackets), or a
/// fixed reason code when it cannot be built.
fn outgoing(
    account: &Account,
    rendered: &RenderedMail,
    recipient: &DirectRecipient,
) -> Result<(OutgoingMail, String), &'static str> {
    let to: Address = recipient
        .address
        .trim()
        .parse()
        .map_err(|_| "address_invalid")?;
    let test = recipient.send_id == TEST_SEND_ID;
    // A test send's ID is fixed, and mail clients drop or thread repeated IDs.
    let local = if test {
        Uuid::now_v7().to_string()
    } else {
        recipient.send_id.to_string()
    };
    let id = format!("{local}@{}", account.from.domain());
    let mut builder = Message::builder()
        .from(Mailbox::new(
            account.from_name.clone(),
            account.from.clone(),
        ))
        .to(Mailbox::new(None, to.clone()))
        .subject(rendered.subject.clone())
        .message_id(Some(format!("<{id}>")));
    if let Some(reply_to) = &account.reply_to {
        builder = builder.reply_to(Mailbox::new(None, reply_to.clone()));
    }
    if !test {
        builder = builder
            .raw_header(HeaderValue::new(
                HeaderName::new_from_ascii_str("List-Unsubscribe"),
                format!("<{}>", rendered.unsubscribe),
            ))
            .raw_header(HeaderValue::new(
                HeaderName::new_from_ascii_str("List-Unsubscribe-Post"),
                "List-Unsubscribe=One-Click".to_owned(),
            ));
    }
    let message = builder
        .multipart(MultiPart::alternative_plain_html(
            rendered.text.clone(),
            rendered.html.clone(),
        ))
        .map_err(|_| "message_invalid")?;
    Ok((
        OutgoingMail {
            from: account.from.clone(),
            to,
            message: message.formatted(),
        },
        id,
    ))
}

#[async_trait]
impl DirectSender for EmailClient {
    async fn send_batch(
        &self,
        _account: &AccountRef,
        recipients: &[DirectRecipient],
        variant: &Variant,
        assets: &[MediaAsset],
    ) -> Result<Vec<DirectSendResult>, AgentError> {
        let violations = self.violations(variant);
        let Some(doc) = variant.email.as_ref().filter(|_| violations.is_empty()) else {
            return Err(AgentError::Validation(violations));
        };
        let account = self.account().await?;
        let skeleton = self
            .compile(doc, assets, &account.sender)
            .map_err(AgentError::Validation)?;
        if recipients.is_empty() {
            return Ok(Vec::new());
        }
        let open_tracking = variant
            .options
            .get("open_tracking")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        self.send_all(&account, &skeleton, recipients, open_tracking)
            .await
    }
}

#[async_trait]
impl SenderDomainCheck for EmailClient {
    /// Checks the from address's domain. Nothing is sent and no session is opened.
    async fn check(
        &self,
        _account: &AccountRef,
        dkim_selector: Option<&str>,
    ) -> Result<Vec<DomainFinding>, AgentError> {
        if let Some(selector) = dkim_selector
            && !is_dkim_selector(selector)
        {
            return Err(AgentError::Validation(vec![
                Violation::agent_rule(message(
                    "dkim_selector_invalid",
                    "The DKIM selector must be one DNS label: letters, digits, and hyphens",
                ))
                .at("dkim_selector"),
            ]));
        }
        let account = self.account().await?;
        Ok(crate::dns::check(
            self.shared.txt.as_ref(),
            account.from.domain(),
            dkim_selector,
        )
        .await)
    }
}

#[async_trait]
impl AgentClient for EmailClient {
    async fn account_info(&self, account: &AccountRef) -> Result<AccountInfo, AgentError> {
        let setup = self.account().await?;
        let mut session = self.transport(&setup).open().await.map_err(open_error)?;
        session.close().await;
        Ok(AccountInfo {
            remote_id: account.remote_id.clone(),
            display_name: setup.from_name.unwrap_or_else(|| setup.from.to_string()),
            handle: None,
            avatar_url: None,
            status: AccountStatus::Active,
        })
    }

    async fn discover_accounts(&self) -> Result<Discovered, AgentError> {
        Ok(discovered(&self.account().await?))
    }

    async fn publish_options(&self, _account: &AccountRef) -> Result<OptionsSchema, AgentError> {
        Ok(self.shared.descriptor.options_schema.clone())
    }

    async fn validate(
        &self,
        variant: &Variant,
        media: &[MediaAsset],
    ) -> Result<Vec<Violation>, AgentError> {
        let mut out = self.violations(variant);
        let Some(doc) = &variant.email else {
            return Ok(out);
        };
        let account = self.account().await?;
        match self.compile(doc, media, &account.sender) {
            Err(violations) => out.extend(violations),
            Ok(skeleton) => {
                let bytes = skeleton.worst_case_html_bytes(self.ctx.link_signer.as_ref());
                if bytes > MAX_HTML_BYTES {
                    out.push(
                        Violation::new(ViolationCode::EmailTooLarge)
                            .at("email")
                            .param("max_bytes", MAX_HTML_BYTES)
                            .param("actual_bytes", bytes),
                    );
                }
            }
        }
        Ok(out)
    }

    async fn publish(
        &self,
        request: PublishRequest<'_>,
        _progress: &dyn PublishProgress,
    ) -> Result<PublishOutcome, AgentError> {
        // Campaigns go out through `send_batch`; a dry run only says so.
        match request.mode {
            PublishMode::DryRun => Ok(PublishOutcome::Simulated {
                planned: vec!["send the email to each recipient in batches".to_owned()],
            }),
            PublishMode::Live => Err(AgentError::Unsupported {
                capability: Capability::DirectSender,
            }),
        }
    }

    async fn status(
        &self,
        _account: &AccountRef,
        _receipt: &PublishReceipt,
    ) -> Result<PublishOutcome, AgentError> {
        Err(AgentError::Unsupported {
            capability: Capability::DirectSender,
        })
    }

    async fn reconcile(
        &self,
        _request: ReconcileRequest<'_>,
    ) -> Result<ReconcileOutcome, AgentError> {
        Err(AgentError::Unsupported {
            capability: Capability::DirectSender,
        })
    }

    fn direct(&self) -> Option<&dyn DirectSender> {
        Some(self)
    }

    fn domain_check(&self) -> Option<&dyn SenderDomainCheck> {
        Some(self)
    }
}

/// The one account a sending setup reaches: its from address.
pub(crate) fn discovered(account: &Account) -> Discovered {
    let address = account.from.to_string();
    Discovered {
        accounts: vec![DiscoveredAccount {
            display_name: account.from_name.clone().unwrap_or_else(|| address.clone()),
            remote_id: address,
            handle: None,
            avatar_url: None,
        }],
    }
}
