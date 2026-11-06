//! The footer's strings, in each campaign language. They live in the agent because it may
//! not reach the catalog services.

use postit_core::EmailLanguage;

/// The footer's words in one language.
pub(crate) struct FooterText {
    /// Before the sender's name in the "you received this" line.
    pub before_sender: &'static str,
    /// After the sender's name.
    pub after_sender: &'static str,
    /// The unsubscribe link's text.
    pub unsubscribe: &'static str,
}

/// The footer for a campaign in `language`.
pub(crate) const fn footer(language: EmailLanguage) -> FooterText {
    match language {
        EmailLanguage::EnUs => FooterText {
            before_sender: "You received this email from ",
            after_sender: ".",
            unsubscribe: "Unsubscribe",
        },
    }
}
