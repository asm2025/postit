# C3 Telegram agent: human verification

Work only a person can do: it needs a real bot, real channels, and a phone. An agent cannot do it, so none of it is in the tests. Each entry names the item, why an agent cannot do it, and its trigger.

## A real channel and a real group

- **Item:** run `backend/crates/agents/telegram/tests/live.rs` with a real bot (`POSTIT_LIVE_TESTS=1`, `POSTIT_TELEGRAM_TOKEN`, `POSTIT_TELEGRAM_CHAT`; it posts for real), then check on a phone, for a public channel and a private one: a text post, a photo, a WebP, a GIF, an MP4, a MOV sent as a file, a 10-item album, a caption in Arabic, and the link in each receipt.
- **Why an agent cannot:** it needs a bot token and channels an agent has no access to, and the result is judged on a phone.
- **Trigger:** now, or phase D3 at the latest.

## Limits in practice

- **Item:** an emoji-heavy 4,096-unit text, to learn whether `Utf16Units` can be relaxed; a 49 MB video on a real uplink within `upload_timeout`; an album of ten large videos, since the Bot API sets no limit on one request's total size.
- **Why an agent cannot:** it depends on Telegram's live behavior and a real network.
- **Trigger:** with the entry above.

## Discovery on a real bot

- **Item:** add the bot to a channel and see it offered; confirm a bot with a webhook keeps it and still connects by name; connect a bot that another program is long-polling, and confirm that program recovers from any `409` and loses no update.
- **Why an agent cannot:** it needs a second program using the same bot, and Telegram's real update queue.
- **Trigger:** with the entries above.
