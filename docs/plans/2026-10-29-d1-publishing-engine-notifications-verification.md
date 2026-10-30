# D1 publishing engine and notifications: human verification

Checks that only a person can do. No agent performed them; the automated tests cover the same paths against fakes (wiremock push services, a test SMTP server, jsdom). Agent work that is deferred is in `2026-10-29-d1-publishing-engine-notifications-deferred.md`.

## Web Push end to end in a real browser

- **Item:** run `cargo dev` (from `backend/`) and `pnpm dev` (from `web/`), sign in at `https://postit.local:44315`, switch browser notifications on in the profile page's notification settings, then make a delivery fail on the mock agent (or record a notice through `Notifier` with a `psql` insert of a failing delivery) and confirm the browser shows the notification, that clicking it opens postit at `/?notification=<id>`, and that a blocked permission shows the blocked state without prompting again.
- **Why an agent cannot do it:** it needs a real browser granting the notification permission and a real push service delivering to it, plus signing in on `postit.local`, which agent safety rules do not allow. The web tests mock `Notification`, `PushManager`, and the service worker.
- **Trigger:** before merging D1, or at the latest the D4 web UI phase.

## Real push services and UnifiedPush distributors

- **Item:** send a push through each real service and confirm delivery and the `410` cleanup: Chrome (FCM), Firefox (Mozilla autopush), Safari (Apple web push), and a UnifiedPush distributor on Android (for example ntfy), including one on a LAN with `push.allow_private_endpoints` on.
- **Why an agent cannot do it:** it needs real browsers and devices subscribed to the live services; CI never calls them. Only the wiremock endpoints in `backend/crates/notify/tests/push.rs` were verified.
- **Trigger:** D6 (mobile push through UnifiedPush) for the distributor, and plan 03 G1 (first deployment) for the browser services.

## Re-pack the vault with the VAPID keys

- **Item:** re-pack `!ref/vault.7z` so it includes the development VAPID key `POSTIT__PUSH__VAPID_PRIVATE_KEY` that D1 Task 20 wrote into the extracted `!ref/vault/development/postit.env`. A machine with an older vault has push enabled without a key and refuses to start until it runs `cargo xtask vapid-keygen` into its extracted file. At G1, generate separate qa and production key pairs and add them to those environments' vault files (the Web Push steps in `docs/deployment.md`).
- **Why an agent cannot do it:** the archive password is out of band and only the maintainer re-packs `vault.7z` (repository rule); the qa and production keys are secrets for environments that do not exist yet.
- **Trigger:** now for the development key; plan 03 G1 for qa and production.

## Live alert and digest email

- **Item:** with a real SMTP provider configured, confirm that a critical notice sends one coalesced alert email about two minutes later (several notices of one job in one email), that the daily comment digest arrives at 08:00 in the user's time zone, and that both render correctly in English and Arabic in common mail clients.
- **Why an agent cannot do it:** it needs a real provider account, its credentials, and real mailboxes; the tests use a fake mail transport and snapshots.
- **Trigger:** plan 03 G1 (first deployment), with the identity-mail checks there.

## Remove leftover scratch folders

- **Item:** delete the scratch folder `K:\tmp-t16` left by D1 Task 16 (it holds only a leftover `patch.py`) and `K:\tmp-t22`, the scratch folder of D1 Task 22 (logs and a helper script). Neither holds anything the repository needs.
- **Why an agent cannot do it:** the agent safety check blocks removing folders outside the repository.
- **Trigger:** any time.
