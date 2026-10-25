//! Stand-ins for services that arrive in later phases, so the account service can hand agents
//! a complete `AgentContext` today. Account connection and checks never reach them; C2 (link
//! signing) replaces them.

use postit_core::{DirectRecipient, DirectSendId, LinkSigner};
use url::Url;

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
