//! Stand-ins for services that arrive in later phases, so the account service can hand agents
//! a complete `AgentContext` today. Account connection and checks never reach them; B4
//! (media) and C2 (link signing) replace them.

use std::time::Duration;

use async_trait::async_trait;
use postit_core::{
    ByteStream, DirectRecipient, DirectSendId, LinkSigner, MediaAsset, MediaError, MediaHost,
    MediaStore,
};
use url::Url;

/// Media storage is not built yet: every call says the asset cannot be read.
pub struct UnavailableMedia;

#[async_trait]
impl MediaStore for UnavailableMedia {
    async fn open(&self, _asset: &MediaAsset) -> Result<ByteStream, MediaError> {
        Err(MediaError::NotFound)
    }
}

impl MediaHost for UnavailableMedia {
    fn public_url(&self, _asset: &MediaAsset, _ttl: Duration) -> Result<Url, MediaError> {
        Err(MediaError::NotReady)
    }
}

/// Link signing is not built yet: every link points at a path that answers 404.
pub struct UnavailableLinks {
    /// `server.public_url`, the base of the placeholder links.
    pub base: Url,
}

impl UnavailableLinks {
    fn link(&self) -> Url {
        self.base
            .join("unavailable")
            .unwrap_or_else(|_| self.base.clone())
    }
}

impl LinkSigner for UnavailableLinks {
    fn unsubscribe_url(&self, _recipient: &DirectRecipient) -> Url {
        self.link()
    }

    fn click_url(&self, _send_id: DirectSendId, _destination: &Url) -> Url {
        self.link()
    }

    fn open_pixel_url(&self, _send_id: DirectSendId) -> Url {
        self.link()
    }
}
