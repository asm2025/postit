# C3 Telegram agent: human verification

Work only a person can do: it needs a real bot, real channels, and a phone. An agent cannot do it, so none of it is in the tests. Each entry names the item, why an agent cannot do it, and its trigger.

## A real channel and a real group

- **Item:** run `backend/crates/agents/telegram/tests/live.rs` with a real bot (`POSTIT_LIVE_TESTS=1`, `POSTIT_TELEGRAM_TOKEN`, `POSTIT_TELEGRAM_CHAT`; it posts for real; `cargo test -p postit-agent-telegram --test live -- --ignored --nocapture`), then check on a phone, for a public channel and a private one: a text post, a photo, a WebP, a GIF, an MP4, a MOV sent as a file, a 10-item album, a caption in a right-to-left script, and the link in each receipt. `live_text_image_and_album` covers the first three; `live_formats` sends the whole list, one post each, and reports every case that fails. A private channel has no `@name`: set `POSTIT_TELEGRAM_CHAT` to its numeric ID (`-100…`).
- **Done so far:** `live_text_image_and_album` on a public channel, text, image, and album, checked on desktop and mobile. `live_formats` ran on the public channel (2026-11-19): every case posted except one right-to-left caption, which failed once with `OutcomeUnknown` ("error sending request") and never reached the channel; the agent did not retry it, as designed. A rerun on both the public and the private channel posted the right-to-left caption.
- **Resolved (2026-11-19):** all cases posted on the public and the private channel and looked right on phone and desktop: text, photo, WebP, GIF (plays), MP4 (plays as a video), MOV (a downloadable `file0.mov` document, as intended for a file send), the 10-item album, and the right-to-left caption. Receipt links were returned for each.
- **Why an agent cannot:** it needs a bot token and channels an agent has no access to, and the result is judged on a phone.
- **Trigger:** now, or phase D3 at the latest.

## Limits in practice

- **Item:** an emoji-heavy 4,096-unit text, to learn whether `Utf16Units` can be relaxed; a 49 MB video on a real uplink within `upload_timeout`; an album of ten large videos, since the Bot API sets no limit on one request's total size.
- **How:** `live_text_limits` (4,096 ASCII, 4,096 emoji, 4,097 emoji) and `live_large_media` (a 49 MB video, then an album of ten) in `live.rs`, run with the same variables and `-- --ignored --nocapture --test-threads=1`. Each prints what Telegram accepted and how long it took; neither fails on a refusal. `POSTIT_LIVE_VIDEO_MB` and `POSTIT_LIVE_ALBUM_VIDEO_MB` (default 49 each; the album uploads ten times that) shrink the uploads on a slow uplink. Record the printed lines here: if 4,096 emoji are accepted, `Utf16Units` can be relaxed to characters.
- **Result, text (2026-11-19):** on both the public and the private channel, 4,096 ASCII and 4,096 emoji (8,192 UTF-16 units) were accepted and 4,097 emoji were refused (`400 message is too long`). Telegram counts characters, so `Utf16Units` can be relaxed; the change is in `2026-11-01-c3-telegram-agent-deferred.md`. 
- **Result, large media (2026-11-19):** a 49 MB MP4 was accepted in 53.6 s (about 7 Mbit/s, far inside the 10-minute `upload_timeout`). An album of ten 49 MB videos (490 MB in one request) was refused at once with `413 Request Entity Too Large`, so Telegram does limit one request's total size despite the docs. Narrowed with `live_album_total_limit`: nine to three items of 24 MB (72 MB and up) were refused with `413`, and two items of 24 MB (48 MB) were accepted in 103.8 s; two 49 MB items (98 MB) were refused. So the request limit lies between 48 and 72 MB, and the agent now refuses an album over 50,000,000 bytes with a clear violation (`ALBUM_MAX_BYTES`, in `compose::violations`).
- **Resolved (2026-11-19):** the 4,096-character text, the 49 MB video, and the album limits were measured on a real bot. Text counting is left as a deferred change (see the deferred file).
- **Why an agent cannot:** it depends on Telegram's live behavior and a real network.
- **Trigger:** with the entry above.

## Discovery on a real bot

- **Item:** add the bot to a channel and see it offered; confirm a bot with a webhook keeps it and still connects by name; connect a bot that another program is long-polling, and confirm that program recovers from any `409` and loses no update.
- **Why an agent cannot:** it needs a second program using the same bot, and Telegram's real update queue.
- **Trigger:** with the entries above.
