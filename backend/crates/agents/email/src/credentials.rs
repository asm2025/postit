//! The account's credentials: who the mail comes from and how to reach the SMTP server.
//!
//! Header fields are parsed into `lettre` addresses, never formatted into header text, and a
//! line break in `from_name` or `reply_to` is refused so no field can add a header. A bad
//! field answers `InvalidCredentials` naming the field, never its value.

use lettre::Address;
use postit_core::{AgentError, CredentialFields, Credentials};
use serde_json::Value;

use crate::render::Sender;
use crate::smtp::{SmtpServer, TlsMode};

/// One account's sending setup.
#[derive(Debug, Clone)]
pub(crate) struct Account {
    /// The sender as the renderer needs it.
    pub sender: Sender,
    /// The display name in `From`, when one is set.
    pub from_name: Option<String>,
    /// The `From` address and envelope sender.
    pub from: Address,
    /// The `Reply-To` address, when one is set.
    pub reply_to: Option<Address>,
    /// The SMTP server.
    pub smtp: SmtpServer,
}

fn invalid(field: &str) -> AgentError {
    AgentError::InvalidCredentials {
        field: Some(field.to_owned()),
    }
}

fn text<'a>(fields: &'a CredentialFields, name: &str) -> Option<&'a str> {
    fields
        .get_plain(name)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn header_text(fields: &CredentialFields, name: &str) -> Result<Option<String>, AgentError> {
    match text(fields, name) {
        Some(value) if value.contains(['\r', '\n']) => Err(invalid(name)),
        other => Ok(other.map(str::to_owned)),
    }
}

fn address(fields: &CredentialFields, name: &str) -> Result<Option<Address>, AgentError> {
    header_text(fields, name)?
        .map(|value| value.parse::<Address>().map_err(|_| invalid(name)))
        .transpose()
}

/// Reads the account from its stored fields.
pub(crate) fn parse(fields: &CredentialFields) -> Result<Account, AgentError> {
    match text(fields, "transport") {
        None | Some("smtp") => {}
        Some(_) => return Err(invalid("transport")),
    }
    let from_name = header_text(fields, "from_name")?;
    let from = address(fields, "from_address")?.ok_or_else(|| invalid("from_address"))?;
    let reply_to = address(fields, "reply_to")?;
    let tls = match text(fields, "tls") {
        Some("none") => TlsMode::None,
        None | Some("starttls") => TlsMode::Starttls,
        Some("tls") => TlsMode::Tls,
        Some(_) => return Err(invalid("tls")),
    };
    let port = match fields.get_plain("port") {
        None | Some(Value::Null) => match tls {
            TlsMode::None => 25,
            TlsMode::Starttls => 587,
            TlsMode::Tls => 465,
        },
        Some(value) => value
            .as_u64()
            .and_then(|port| u16::try_from(port).ok())
            .filter(|port| *port != 0)
            .ok_or_else(|| invalid("port"))?,
    };
    let host = text(fields, "host")
        .ok_or_else(|| invalid("host"))?
        .to_owned();
    let login = match text(fields, "username") {
        None => None,
        Some(user) => {
            let password = fields
                .get_secret("password")
                .ok_or_else(|| invalid("password"))?;
            Some((user.to_owned(), password.clone()))
        }
    };
    Ok(Account {
        sender: Sender {
            from_name: from_name.clone().unwrap_or_default(),
            from_address: from.to_string(),
            reply_to: reply_to.as_ref().map(ToString::to_string),
            postal_address: fields
                .get_plain("postal_address")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        },
        from_name,
        from,
        reply_to,
        smtp: SmtpServer {
            host,
            port,
            tls,
            login,
        },
    })
}

/// Reads the account from a grant's credentials.
pub(crate) fn of_grant(credentials: &Credentials) -> Result<Account, AgentError> {
    match credentials {
        Credentials::Fields(fields) => parse(fields),
        Credentials::OAuth(_) => Err(invalid("transport")),
    }
}

/// Whether the postal address has a line with text, as the footer needs.
pub(crate) fn has_postal_address(sender: &Sender) -> bool {
    sender
        .postal_address
        .lines()
        .any(|line| !line.trim().is_empty())
}
