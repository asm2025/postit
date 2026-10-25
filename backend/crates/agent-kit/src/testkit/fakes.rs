//! In-memory implementations of the traits in an agent context.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;
use postit_core::{
    AgentError, ByteStream, CredentialStore, Credentials, DirectRecipient, DirectSendId,
    LinkSigner, MediaAsset, MediaAssetId, MediaError, MediaHost, MediaStore, OAuthTokens,
};
use url::Url;

/// The size of each piece [`MemoryMediaStore`] streams.
const PIECE: usize = 64 * 1024;

/// A [`MediaStore`] over bytes held in memory.
#[derive(Debug, Default)]
pub struct MemoryMediaStore {
    assets: Mutex<HashMap<MediaAssetId, Bytes>>,
}

impl MemoryMediaStore {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Stores the bytes served for `id`.
    pub fn insert(&self, id: MediaAssetId, bytes: Bytes) {
        self.assets.lock().unwrap().insert(id, bytes);
    }
}

#[async_trait]
impl MediaStore for MemoryMediaStore {
    async fn open(&self, asset: &MediaAsset) -> Result<ByteStream, MediaError> {
        let bytes = self
            .assets
            .lock()
            .unwrap()
            .get(&asset.id)
            .cloned()
            .ok_or(MediaError::NotFound)?;
        let pieces: Vec<std::io::Result<Bytes>> = (0..bytes.len())
            .step_by(PIECE)
            .map(|start| Ok(bytes.slice(start..bytes.len().min(start + PIECE))))
            .collect();
        Ok(futures::stream::iter(pieces).boxed())
    }
}

/// A [`MediaHost`] that issues `https://media.test/{id}?ttl={secs}` and remembers each issue.
#[derive(Debug, Default)]
pub struct MemoryMediaHost {
    issued: Mutex<Vec<(MediaAssetId, Duration)>>,
}

impl MemoryMediaHost {
    /// A host that has issued nothing.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Every `(asset, ttl)` pair issued so far, in order.
    #[must_use]
    pub fn issued(&self) -> Vec<(MediaAssetId, Duration)> {
        self.issued.lock().unwrap().clone()
    }
}

impl MediaHost for MemoryMediaHost {
    fn public_url(&self, asset: &MediaAsset, ttl: Duration) -> Result<Url, MediaError> {
        self.issued.lock().unwrap().push((asset.id, ttl));
        Url::parse(&format!(
            "https://media.test/{}?ttl={}",
            asset.id,
            ttl.as_secs()
        ))
        .map_err(|e| MediaError::Io(e.to_string()))
    }
}

/// A [`CredentialStore`] holding one grant's credentials in memory.
#[derive(Debug)]
pub struct MemoryCredentialStore {
    current: Mutex<Option<Credentials>>,
    refreshed: Mutex<Vec<OAuthTokens>>,
}

impl MemoryCredentialStore {
    /// A store holding `credentials`.
    #[must_use]
    pub fn new(credentials: Credentials) -> Self {
        Self {
            current: Mutex::new(Some(credentials)),
            refreshed: Mutex::default(),
        }
    }

    /// A store with no credentials: reading fails with [`AgentError::Authentication`].
    #[must_use]
    pub fn empty() -> Self {
        Self {
            current: Mutex::default(),
            refreshed: Mutex::default(),
        }
    }

    /// Every token set saved through `store_refreshed`, in order.
    #[must_use]
    pub fn refreshed(&self) -> Vec<OAuthTokens> {
        self.refreshed.lock().unwrap().clone()
    }
}

#[async_trait]
impl CredentialStore for MemoryCredentialStore {
    async fn credentials(&self) -> Result<Credentials, AgentError> {
        self.current
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| AgentError::Authentication {
                message: "no credentials stored".to_owned(),
            })
    }

    async fn store_refreshed(&self, tokens: OAuthTokens) -> Result<(), AgentError> {
        self.refreshed.lock().unwrap().push(tokens.clone());
        *self.current.lock().unwrap() = Some(Credentials::OAuth(tokens));
        Ok(())
    }
}

/// A [`LinkSigner`] whose links live under `https://links.test/` and carry no signature.
#[derive(Debug, Default, Clone, Copy)]
pub struct FakeLinkSigner;

impl LinkSigner for FakeLinkSigner {
    fn unsubscribe_url(&self, recipient: &DirectRecipient) -> Url {
        link(&format!("unsubscribe/{}", recipient.send_id))
    }

    fn click_url(&self, send_id: DirectSendId, destination: &Url) -> Url {
        let mut url = link(&format!("click/{send_id}"));
        url.query_pairs_mut()
            .append_pair("to", destination.as_str());
        url
    }

    fn open_pixel_url(&self, send_id: DirectSendId) -> Url {
        link(&format!("open/{send_id}"))
    }
}

fn link(path: &str) -> Url {
    Url::parse(&format!("https://links.test/{path}")).expect("a fixed host and a UUID path")
}
