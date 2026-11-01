//! Finding the chats a bot can post to.
//!
//! Discovery runs `getMe`, one read-only peek at the bot's recent updates, then each chat the
//! user named. A bot posts to a chat where it is an admin (with the right to post, in a
//! channel); one rule, [`can_post`], decides for both ways of finding a chat.

use std::collections::HashSet;

use postit_core::{AgentError, Discovered, DiscoveredAccount};

use crate::api::{Api, ApiError, Chat, ChatMember};
use crate::credentials::{ChatRef, Parsed};

/// Whether a bot with this standing in a chat of this type may post there.
pub(crate) fn can_post(chat_type: &str, member: &ChatMember) -> bool {
    let creator = member.status == "creator";
    match chat_type {
        "channel" => {
            creator || (member.status == "administrator" && member.can_post_messages == Some(true))
        }
        "group" | "supergroup" => creator || member.status == "administrator",
        _ => false,
    }
}

fn invalid(field: &str) -> AgentError {
    AgentError::InvalidCredentials {
        field: Some(field.to_owned()),
    }
}

/// The error for a token Telegram does not accept: `401`, or `404` for a malformed token.
pub(crate) fn token_error(err: ApiError) -> AgentError {
    match AgentError::from(err) {
        AgentError::Authentication { .. }
        | AgentError::Remote {
            status: Some(404), ..
        } => invalid("bot_token"),
        other => other,
    }
}

fn found(chat: &Chat) -> DiscoveredAccount {
    DiscoveredAccount {
        remote_id: chat.id.to_string(),
        display_name: chat
            .title
            .clone()
            .or_else(|| chat.username.clone())
            .unwrap_or_else(|| chat.id.to_string()),
        handle: chat.username.as_ref().map(|name| format!("@{name}")),
        // A chat photo's URL contains the bot token.
        avatar_url: None,
    }
}

/// The chats the bot's recent updates show it can post to. A `409` means a webhook or another
/// program owns the queue, which is not an error: discovery falls back to named chats.
async fn from_updates(api: &Api<'_>) -> Result<Vec<DiscoveredAccount>, AgentError> {
    let changes = match api.peek_updates().await {
        Ok(changes) => changes,
        Err(ApiError::Conflict) => return Ok(Vec::new()),
        Err(other) => return Err(other.into()),
    };
    // The latest update per chat wins; a bot added then removed is not offered.
    let mut latest: Vec<(i64, usize)> = Vec::new();
    for (i, change) in changes.iter().enumerate() {
        match latest.iter_mut().find(|(id, _)| *id == change.chat.id) {
            Some(entry) => entry.1 = i,
            None => latest.push((change.chat.id, i)),
        }
    }
    Ok(latest
        .into_iter()
        .map(|(_, i)| &changes[i])
        .filter(|change| can_post(&change.chat.kind, &change.member))
        .map(|change| found(&change.chat))
        .collect())
}

/// A chat the user named: read it, and check the bot may post there.
async fn named(api: &Api<'_>, bot: i64, chat: &ChatRef) -> Result<DiscoveredAccount, AgentError> {
    let unusable = |err: ApiError| match AgentError::from(err) {
        AgentError::AccountUnreachable { .. } => invalid("chats"),
        other => other,
    };
    let info = api.get_chat(chat).await.map_err(unusable)?;
    let member = api.get_chat_member(info.id, bot).await.map_err(unusable)?;
    if can_post(&info.kind, &member) {
        Ok(found(&info))
    } else {
        Err(invalid("chats"))
    }
}

/// The chats `parsed` reaches, the bot's updates first, then the named chats; a chat found
/// twice is kept once.
///
/// # Errors
/// [`AgentError::InvalidCredentials`] for a bad token or a named chat the bot cannot post to.
pub(crate) async fn discover(api: &Api<'_>, parsed: &Parsed) -> Result<Discovered, AgentError> {
    let bot = api.get_me().await.map_err(token_error)?;
    let mut accounts = from_updates(api).await?;
    for chat in &parsed.chats {
        accounts.push(named(api, bot.id, chat).await?);
    }
    let mut seen = HashSet::new();
    accounts.retain(|account| seen.insert(account.remote_id.clone()));
    Ok(Discovered { accounts })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(status: &str, post: Option<bool>) -> ChatMember {
        ChatMember {
            status: status.to_owned(),
            can_post_messages: post,
        }
    }

    #[test]
    fn can_post_rules() {
        assert!(can_post("channel", &member("administrator", Some(true))));
        assert!(!can_post("channel", &member("administrator", Some(false))));
        assert!(!can_post("channel", &member("administrator", None)));
        assert!(can_post("channel", &member("creator", None)));
        assert!(!can_post("channel", &member("member", None)));
        assert!(can_post("supergroup", &member("administrator", None)));
        assert!(can_post("group", &member("creator", None)));
        assert!(!can_post("supergroup", &member("member", None)));
        assert!(!can_post("supergroup", &member("left", None)));
        assert!(!can_post("private", &member("administrator", Some(true))));
    }
}
