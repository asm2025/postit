# C1 contacts and C2 email agent: human verification

Checks that only a person can do. No agent performed them. The automated tests cover the same paths against fakes: an in-process SMTP sink (`postit-agent-email`'s `testkit` feature), rendering snapshots, a fake DNS lookup, and the API test app. Agent work that is deferred is in `2026-10-30-c1-c2-contacts-email-agent-deferred.md`.

## A campaign through a real Papercut, viewed in its UI

- **Item:** connect an email account in development against the operator's Papercut (the host and port `postit-mail` uses in development, `tls` `none`, which `email.allow_private_hosts = true` in `development.toml` allows), build a list of a few consented contacts (one with a merge field, one without, one `unsubscribed`), and send a campaign to it with an image, a button, and open tracking on. In Papercut's UI, confirm that each subscribed contact got one message with an HTML and a plain-text part, the merge values and fallbacks are right, the footer shows the postal address and the unsubscribe link, the `List-Unsubscribe` and `List-Unsubscribe-Post` headers are present, the image loads, the button goes through `/public/t/c/…` to its destination, and the unsubscribed contact got nothing. Then follow the unsubscribe link: the page asks to confirm, and confirming marks the contact `unsubscribed`. Repeat once with an Arabic document (`language: "ar"`) and check right-to-left layout and the Arabic footer and page. Also send `POST /api/v1/accounts/{id}/test-email` and confirm it arrives without the unsubscribe headers and that its footer link shows the "this was a test" page.
- **Why an agent cannot do it:** it needs Papercut running on the maintainer's machine and a person looking at its UI and a browser; the agent safety rules do not allow signing in on `postit.local`. There is also no route to start a campaign until D3, so today it needs D3 or a test harness the maintainer runs against `PublishingService`. The automated end-to-end run (`backend/crates/server/tests/email_campaign.rs`) uses an in-process SMTP sink instead.
- **Trigger:** plan 03 D3 (the publish route), and again in D4 when the web composer can send a campaign.

## Rendering in real mail clients, including Arabic

- **Item:** send a campaign (English and Arabic documents, every block type, a long text block, an image with a link) to real Gmail (web and Android app), Outlook (web and desktop), and Apple Mail (macOS and iOS) mailboxes, at desktop and phone widths. Confirm the layout holds, the Arabic message reads right to left with an Arabic footer, Gmail does not clip it (the HTML stays under the 100 KiB limit), the plain-text part reads well, and the message does not land in spam when SPF, DKIM, and DMARC are set up (check the `dns-check` findings for the sending domain at the same time).
- **Why an agent cannot do it:** it needs real mailboxes on those providers, a real sending relay and domain, and a person to look at each client. The tests only snapshot the HTML and text that `mrml` produces.
- **Trigger:** plan 03 G1 (first deployment), when a public sending domain exists.

## One-click unsubscribe from a real mail client

- **Item:** in Gmail and Apple Mail (and Outlook where it offers it), use the client's own "Unsubscribe" button on a received campaign, which posts the RFC 8058 one-click request, and confirm the contact becomes `unsubscribed` with an `unsubscribed` suppression and that the next campaign skips the address. Also open the footer link in a browser and confirm the `GET` page changes nothing until the confirm button is pressed.
- **Why an agent cannot do it:** the one-click request is sent by the mail provider's servers to a publicly reachable `server.public_url`, so it needs a public deployment, a real mailbox, and a person clicking in the client. The tests post the one-click body to the server directly.
- **Trigger:** plan 03 G1 (first deployment).

## Real TLS and STARTTLS against a relay

- **Item:** connect email accounts against a real SMTP relay twice, once with `tls = starttls` (port 587) and once with `tls = tls` (implicit TLS, port 465), and send a test email and a small campaign through each. Confirm the certificate is checked against the configured host name (a certificate for another name must fail), the login works over the encrypted session, and a relay that does not offer STARTTLS is refused rather than used in clear.
- **Why an agent cannot do it:** it needs a real relay account and its credentials, which only the maintainer holds. The tests exercise only the refusal paths (STARTTLS missing, a failed handshake), because a TLS sink would need a trusted test certificate.
- **Trigger:** plan 03 G1 (first deployment), or earlier whenever the maintainer has relay credentials at hand (D3 at the latest, with the Papercut run).

## Reset the development database

- **Item:** reset the development database `postit` so it picks up this branch's edits to `backend/crates/data/migrations/0001_init.sql` (the single-migration policy): `media_assets.retain_until`, `suppressions.owner_opt_out`, the new default of `contact_imports.report` (`{"entries": [], "totals": {}}` instead of `[]`), `contact_imports.list_id` with its `(owner_id, list_id)` foreign key to `contact_lists` (`ON DELETE SET NULL (list_id)`), the `contact_imports_status_idx` index, the `contact_import_rows` table, and the `email_events_once_idx` unique index. These are every edit to the file on this branch (`git diff 075f3b9..HEAD -- backend/crates/data/migrations/0001_init.sql`). Until then, the development database does not match the code, so `cargo dev` and the dev stack cannot run this branch against it.
- **Why an agent cannot do it:** the agent safety check refused the destructive reset of the real development database. Every task used the scratch database `postit_c1_prepare` on the same server instead.
- **Trigger:** now, before running `cargo dev` or the dev stack on this branch.
- **Resolved:** 2026-11-01. The maintainer confirmed the reset came after `suppressions.owner_opt_out` landed.

## Remove the scratch database and helper folder

- **Item:** drop the scratch database `postit_c1_prepare` on the development Postgres server and delete `K:\cargo-target\postit-scripts\` (helper scripts and logs from Task 6). Neither holds anything the repository needs.
- **Why an agent cannot do it:** the agent safety check blocks destructive actions outside the repository, such as dropping a database or removing folders outside it.
- **Trigger:** after the branch merges; the closing gates and the cleanup pass still use the scratch database.
- **Resolved:** 2026-11-01, after the merge into `stage`. The maintainer confirmed the cleanup is done.
