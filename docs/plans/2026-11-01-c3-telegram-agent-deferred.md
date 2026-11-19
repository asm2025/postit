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

## The connection guide

- **Issue:** the connection guide for the clients: create a dedicated bot in BotFather, add it as a channel admin with "Post messages", add a private channel by its numeric ID (an invite link cannot be used), and revoke the token from BotFather to disconnect (`revoke` stays `Unsupported`, since `logOut` would disable the bot for 10 minutes). It must also say to name the chats again when replacing a revoked token: discovery only sees updates from the last 24 hours, so a new token with no chats named finds nothing and the old accounts stay parked. The `chats` help text already says so in English.
- **Why not now:** the clients that show the guide do not exist yet; the `telegram.*` keys ship with English text.
- **Trigger:** phase D4 (web), then D6 (mobile).

## The stored chat ID after a group upgrade

- **Issue:** a group upgraded to a supergroup gets a new chat ID, and the contract has no way to update `remote_account_id`. The agent follows `migrate_to_chat_id` inside each call, so publishing and checks keep working, at the cost of one refused request per post and per check.
- **Why not now:** nothing needs the current ID until the inbox does.
- **Trigger:** phase E2, if its update reader needs the current ID: add a `remote_account_id` update path then.

## Telegram counts characters, not UTF-16 units

- **Issue:** the live check on 2026-11-19 (public and private channel) showed Telegram accepts a text of 4,096 emoji (8,192 UTF-16 units) and refuses 4,097 with `message is too long`. So its limit counts characters (code points), and the agent's `TextMeasure::Utf16Units` stops an emoji-heavy post at half the length Telegram allows (2,048 emoji). Core has no code-point measure: it needs a new `TextMeasure` variant (a wire type, so `backend/openapi.json`, the generated web and mobile schemas, and the drafter's prompt wording follow), then the Telegram descriptor switches to it for text and, after the same probe on a 1,024-character caption, for captions.
- **Why not now:** a wire-type change across core, the generated clients, and the drafter is wider than a connector fix, and the cost today is only an early stop on a rare post, never a platform refusal.
- **Trigger:** the next change to `TextMeasure`, or phase D4 if a user hits the early stop. Probe a caption of 1,024 emoji with `live.rs` first, since the caption limit was not measured.

## An album over Telegram's request size limit

- **Resolved (2026-11-19):** the live probe put the limit between 48 and 72 MB (two 24 MB videos accepted; two 49 MB and three 24 MB refused with `413`). `compose::violations` now refuses an album whose files add up to more than 50,000,000 bytes (`ALBUM_MAX_BYTES`), with the violation `telegram.album_too_large` and a test.
