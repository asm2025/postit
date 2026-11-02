//! The account's credentials: the bot token and the chats named by hand.
//!
//! The token goes into a URL path, so its shape is checked before any call. A bad field
//! answers `InvalidCredentials` naming the field, never its value.

use postit_core::{AgentError, CredentialFields};
use secrecy::{ExposeSecret, SecretString};
use serde_json::Value;

/// A chat the user named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatRef {
    /// `@name`, kept without the `@`.
    Username(String),
    /// A numeric chat ID.
    Id(i64),
}

/// The parsed credentials.
pub struct Parsed {
    /// The bot token.
    pub token: SecretString,
    /// Chats to add by hand, in the order given.
    pub chats: Vec<ChatRef>,
}

/// The error for a field that cannot be used; it names the field, never its value.
pub(crate) fn invalid(field: &str) -> AgentError {
    AgentError::InvalidCredentials {
        field: Some(field.to_owned()),
    }
}

/// Whether `token` is digits, a colon, then letters, digits, `_`, or `-`.
fn token_shape(token: &str) -> bool {
    let Some((id, secret)) = token.split_once(':') else {
        return false;
    };
    !id.is_empty()
        && id.bytes().all(|b| b.is_ascii_digit())
        && !secret.is_empty()
        && secret
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn username(name: &str) -> Option<ChatRef> {
    // Four characters is the shortest a username can be: collectible ones go that low.
    let valid = (4..=32).contains(&name.len())
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
    valid.then(|| ChatRef::Username(name.to_owned()))
}

/// One entry of `chats`.
fn chat(entry: &str) -> Option<ChatRef> {
    let rest = entry
        .strip_prefix("https://")
        .or_else(|| entry.strip_prefix("http://"))
        .unwrap_or(entry);
    for host in ["t.me/", "telegram.me/"] {
        if let Some(path) = rest.strip_prefix(host) {
            // A private invite (`+…`, `joinchat/…`) cannot be resolved, and a message link
            // (`c/…`, `name/42`) names no chat by itself.
            return username(path.trim_end_matches('/'));
        }
    }
    if let Some(name) = entry.strip_prefix('@') {
        return username(name);
    }
    entry
        .parse::<i64>()
        .ok()
        .filter(|id| *id != 0)
        .map(ChatRef::Id)
}

/// Reads the account from its stored fields.
///
/// # Errors
/// [`AgentError::InvalidCredentials`] naming `bot_token` or `chats`.
pub fn parse(fields: &CredentialFields) -> Result<Parsed, AgentError> {
    let token = fields
        .get_secret("bot_token")
        .map(|t| t.expose_secret().trim().to_owned())
        .filter(|t| token_shape(t))
        .ok_or_else(|| invalid("bot_token"))?;
    let mut chats = Vec::new();
    if let Some(Value::String(list)) = fields.get_plain("chats") {
        for entry in list.split([',', '\n']).map(str::trim) {
            if entry.is_empty() {
                continue;
            }
            chats.push(chat(entry).ok_or_else(|| invalid("chats"))?);
        }
    }
    Ok(Parsed {
        token: SecretString::from(token),
        chats,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::{Map, json};

    use super::*;

    fn fields(token: &str, chats: Option<&str>) -> CredentialFields {
        let mut plain = Map::new();
        if let Some(chats) = chats {
            plain.insert("chats".to_owned(), json!(chats));
        }
        let secret = BTreeMap::from([("bot_token".to_owned(), SecretString::from(token))]);
        CredentialFields::from_parts(plain, secret)
    }

    fn field_of(err: AgentError) -> Option<String> {
        match err {
            AgentError::InvalidCredentials { field } => field,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn token_shape_is_checked() {
        assert!(parse(&fields("123456:ABC-def_1", None)).is_ok());
        for bad in [
            "", "abc", "123456", "x:abc", "123:", "12/3:abc", "123:a?b", "123:a#b", "123:a b",
        ] {
            let err = parse(&fields(bad, None)).err().unwrap();
            assert_eq!(field_of(err).as_deref(), Some("bot_token"), "{bad}");
        }
    }

    #[test]
    fn chat_refs() {
        let parsed = parse(&fields(
            "1:a",
            Some("@MyChannel, https://t.me/other/ ,t.me/third,-1001234567890"),
        ))
        .unwrap();
        assert_eq!(
            parsed.chats,
            [
                ChatRef::Username("MyChannel".into()),
                ChatRef::Username("other".into()),
                ChatRef::Username("third".into()),
                ChatRef::Id(-1_001_234_567_890),
            ]
        );
        assert!(parse(&fields("1:a", None)).unwrap().chats.is_empty());
        assert!(parse(&fields("1:a", Some("  "))).unwrap().chats.is_empty());
        for bad in [
            "t.me/+AbCdEf",
            "t.me/joinchat/x",
            "@a b",
            "https://example.com/x",
            "plain",
        ] {
            let err = parse(&fields("1:a", Some(bad))).err().unwrap();
            assert_eq!(field_of(err).as_deref(), Some("chats"), "{bad}");
        }
    }

    #[test]
    fn four_character_usernames_are_accepted() {
        let parsed = parse(&fields("1:a", Some("@abcd"))).unwrap();
        assert_eq!(parsed.chats, [ChatRef::Username("abcd".into())]);
        let err = parse(&fields("1:a", Some("@abc"))).err().unwrap();
        assert_eq!(field_of(err).as_deref(), Some("chats"));
    }

    #[test]
    fn surrounding_spaces_are_ignored() {
        let parsed = parse(&fields("1:a", Some(" @MyChannel "))).unwrap();
        assert_eq!(parsed.chats, [ChatRef::Username("MyChannel".into())]);
    }
}
