//! Decision record (plan 02 P1): native `async fn` in traits is not dyn-compatible on
//! stable Rust 1.98 (the return-position `impl Future` cannot be boxed automatically
//! for a trait object). Traits that need `dyn` dispatch (`AgentClient`, `Agent`,
//! `MediaStore`, etc. in plan 03) use the `async-trait` crate, which desugars to a boxed
//! future and stays dyn-compatible. This module is a compile-time proof of that approach.

#[async_trait::async_trait]
trait AsyncGreeter: Send + Sync {
    async fn greet(&self) -> String;
}

struct StaticGreeter;

#[async_trait::async_trait]
impl AsyncGreeter for StaticGreeter {
    async fn greet(&self) -> String {
        "hello".to_owned()
    }
}

/// Compile-only: every trait the engine holds as `dyn` stays dyn-compatible.
#[cfg(test)]
#[allow(dead_code)]
mod dyn_compat {
    use crate::{
        Agent, AgentClient, Commenter, CredentialStore, DirectSender, HttpHandle, InboxReader,
        LinkSigner, MediaHost, MediaProbe, MediaStore, MetricsReader, NativeScheduler,
        PublishProgress,
    };

    fn agent(_: &dyn Agent) {}
    fn agent_client(_: &dyn AgentClient) {}
    fn commenter(_: &dyn Commenter) {}
    fn inbox_reader(_: &dyn InboxReader) {}
    fn metrics_reader(_: &dyn MetricsReader) {}
    fn direct_sender(_: &dyn DirectSender) {}
    fn native_scheduler(_: &dyn NativeScheduler) {}
    fn credential_store(_: &dyn CredentialStore) {}
    fn media_store(_: &dyn MediaStore) {}
    fn media_probe(_: &dyn MediaProbe) {}
    fn media_host(_: &dyn MediaHost) {}
    fn link_signer(_: &dyn LinkSigner) {}
    fn publish_progress(_: &dyn PublishProgress) {}
    fn http_handle(_: &dyn HttpHandle) {}
}

#[cfg(test)]
mod tests {
    use super::{AsyncGreeter, StaticGreeter};

    #[tokio::test]
    async fn dyn_async_trait_object_is_callable() {
        let greeter: Box<dyn AsyncGreeter> = Box::new(StaticGreeter);
        assert_eq!(greeter.greet().await, "hello");
    }
}
