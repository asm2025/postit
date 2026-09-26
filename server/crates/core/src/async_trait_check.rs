//! Decision record (plan 02 P1): native `async fn` in traits is not dyn-compatible on
//! stable Rust 1.98 (the return-position `impl Future` cannot be boxed automatically
//! for a trait object). Traits that need `dyn` dispatch (`PlatformClient`, `PlatformPlugin`,
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

#[cfg(test)]
mod tests {
    use super::{AsyncGreeter, StaticGreeter};

    #[tokio::test]
    async fn dyn_async_trait_object_is_callable() {
        let greeter: Box<dyn AsyncGreeter> = Box::new(StaticGreeter);
        assert_eq!(greeter.greet().await, "hello");
    }
}
