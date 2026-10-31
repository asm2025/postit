//! Bounce classification. Each refusal is judged with the SMTP stage that gave it, because
//! only `RCPT` speaks about the address, and one sender-reputation refusal must not suppress
//! a whole list.
//!
//! | Stage  | Reply                                                           | Result     |
//! |--------|-----------------------------------------------------------------|------------|
//! | any    | 4xx                                                             | `Soft`     |
//! | `MAIL` | any 5xx                                                         | `Sender`   |
//! | `DATA` | any 5xx                                                         | `Rejected` |
//! | `RCPT` | `5.7.x`, or the sender codes `5.1.7` and `5.1.8`                | `Rejected` |
//! | `RCPT` | any other `5.1.x`, `5.2.1`, or plain `550`, `551`, `553`        | `Bounce`   |
//! | `RCPT` | any other 5xx                                                   | `Rejected` |
//!
//! Whether three leading `5.7.x` results mean the sender is refused is batch state, decided
//! in `send_batch`, not here.

use crate::transport::{SendRefusal, SmtpStage};

/// What a refusal means for the recipient.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Classified {
    /// The address will never work: suppress it.
    Bounce,
    /// Temporary: try again later.
    Soft,
    /// Refused for this mail, not for the address: fail it without suppressing.
    Rejected,
    /// The server refused the sender, not the recipient.
    Sender,
}

/// Classifies one refusal by its stage and codes.
#[must_use]
pub fn classify(refusal: &SendRefusal) -> Classified {
    if (400..500).contains(&refusal.code) {
        return Classified::Soft;
    }
    match refusal.stage {
        SmtpStage::Mail => Classified::Sender,
        SmtpStage::Data => Classified::Rejected,
        SmtpStage::Rcpt => match refusal.enhanced {
            Some((5, 7, _) | (5, 1, 7 | 8)) => Classified::Rejected,
            Some((5, 1, _) | (5, 2, 1)) => Classified::Bounce,
            _ if matches!(refusal.code, 550 | 551 | 553) => Classified::Bounce,
            _ => Classified::Rejected,
        },
    }
}

/// The enhanced status code (RFC 3463) at the start of a reply's text, such as `5.1.1` in
/// `5.1.1 <bob@example.com>: user unknown`. The class is 2, 4, or 5; the subject and detail
/// have one to three digits. The text itself is never kept.
#[must_use]
pub fn parse_enhanced(text: &str) -> Option<(u8, u16, u16)> {
    let token = text.split_whitespace().next()?;
    let mut parts = token.split('.');
    let class = parts.next()?;
    let subject = parts.next()?;
    let detail = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    let number =
        |part: &str| (1..=3).contains(&part.len()) && part.bytes().all(|b| b.is_ascii_digit());
    if !(class.len() == 1 && number(class) && number(subject) && number(detail)) {
        return None;
    }
    let class: u8 = class.parse().ok()?;
    if !matches!(class, 2 | 4 | 5) {
        return None;
    }
    Some((class, subject.parse().ok()?, detail.parse().ok()?))
}
