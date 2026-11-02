# C3 Telegram agent: deferred items

Work an agent can do, left for the phase that needs it. Each entry names the issue, why it is not in C3, and the trigger that picks it up.

## Telegram inbox and comments

- **Issue:** `InboxReader` and `Commenter` for Telegram. Comments on a channel post arrive in the linked discussion group (`ChatFullInfo.linked_chat_id`); the bot must be in that group, as an admin or with privacy mode off. Reading them means `getUpdates` with a confirming `offset`, and the update queue belongs to the bot, so to the grant, while `InboxReader::fetch_since` takes one account and one cursor. Fetching for one channel would consume another's updates, and confirming would erase the `my_chat_member` updates that discovery peeks at.
- **Why not now:** nothing can test a per-grant reader until the inbox exists, and the contract should not change without a user.
- **Trigger:** phase E2. It adds a per-grant update reader that routes one bot's updates to accounts, a confirming `offset`, a poll at least every 24 hours (Telegram keeps updates that long), and keeps discovery's peek working once updates are confirmed.

## Telegram account metric

- **Issue:** `MetricsReader` for Telegram. The Bot API has no per-post view count; `getChatMemberCount` gives the subscriber count, so an account metric is possible and a per-post one is not.
- **Why not now:** the metric tables and jobs belong to E3.
- **Trigger:** phase E3. Report the subscriber count as the account metric and no post views.

## Translated strings and the connection guide

- **Issue:** Arabic text for every `telegram.*` message key (credentials form, options, violations), and the connection guide for the clients: create a dedicated bot in BotFather, add it as a channel admin with "Post messages", add a private channel by its numeric ID (an invite link cannot be used), and revoke the token from BotFather to disconnect (`revoke` stays `Unsupported`, since `logOut` would disable the bot for 10 minutes).
- **Why not now:** the clients that show these strings do not exist yet; the keys ship with English text.
- **Trigger:** phase D4 (web), then D6 (mobile).

## The stored chat ID after a group upgrade

- **Issue:** a group upgraded to a supergroup gets a new chat ID, and the contract has no way to update `remote_account_id`. The agent follows `migrate_to_chat_id` inside each call, so publishing and checks keep working, at the cost of one refused request per post and per check.
- **Why not now:** nothing needs the current ID until the inbox does.
- **Trigger:** phase E2, if its update reader needs the current ID: add a `remote_account_id` update path then.
