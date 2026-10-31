//! `postit-agent-email`: the email agent. It renders an [`EmailDocument`] to responsive HTML
//! and plain text, and sends campaigns to consented contacts.
//!
//! # Rendering
//!
//! [`Skeleton::compile`] turns a document into HTML once per batch through `mrml`, with
//! sentinel placeholders where merge values and signed links go. [`Skeleton::for_recipient`]
//! fills them for one recipient in a single pass, so `mrml` never runs per recipient and a
//! value is never read again once written. The footer, with the sender's postal address and
//! the unsubscribe link, is added by the renderer and no block can remove it.
//!
//! Of the postit crates, the agent may depend on `postit-core`, `postit-http`, and
//! `postit-agent-kit` only, never on the database or the contacts service.
//!
//! [`EmailDocument`]: postit_core::EmailDocument

mod render;

pub use render::{Escape, MergeError, MergeTemplate, RenderError, RenderedMail, Sender, Skeleton};
