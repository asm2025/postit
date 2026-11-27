# D3 exit flow by hand: request list

The step list behind the first entry of `2026-11-07-d3-publishing-rest-surface-verification.md`. It mirrors the automated flow in `backend/crates/server/tests/d3_flow.rs`, with real sign-in and the dev mock agents. Paste the result of each step (status, and the fields named under **Check**) when you report back; never paste a token.

## Setup

- API: `https://postit.local:44310` (the stack) or the native `cargo dev` address. Every request carries `Authorization: Bearer <token>`.
- Two signed-in users, both `active` (approve them in the admin users screen or `PATCH /users/{id}` as an admin): the **owner** and the **delegate**. Get each token from a browser sign-in (Zitadel) and keep them as two Bruno variables.
- Mock agents exist only in the dev build (`cargo dev`, or the development image, which builds with `mock-agent`). For the email step, run a local SMTP catcher (Papercut) on port 25. The dev config allows private SMTP hosts, so the account may use plain `tls: "none"`.
- On POST and PATCH calls that change a post or a job, `If-Match` takes the `ETag` header of the last answer for that post or job.
- Save each `id` you get back (account, list, media, post, job) as a variable.

## Owner

1. `GET /api/v1/me` and `GET /api/v1/platforms`. **Check:** `status: active`; `mock` and `email` are in the platform list.
2. **Connect a mock account.** `POST /api/v1/accounts/credentials/mock` with `{"account_name": "main", "token": "any-token"}`. The answer has a `connect_id`. Then `POST /api/v1/accounts/connect/{connect_id}/confirm` with `{}`. **Check:** `connected[0].account.id` (this is `mock_a`).
3. **Connect an email account.** `POST /api/v1/accounts/credentials/email` with
   `{"transport": "smtp", "from_name": "Corner Bakery", "from_address": "bakery@example.com", "postal_address": "12 Main St, Springfield", "host": "localhost", "port": 25, "tls": "none"}`, then confirm as above (`email_a`). In the Docker stack use `"host": "host.docker.internal"`.
4. `GET /api/v1/accounts`. **Check:** both accounts with `status: active` (an account that needs attention reads `needs_reconnect`, `revoked`, or `disconnected` in that field). Optionally `POST /accounts/{id}/check` on each.
5. **Contacts.** `POST /api/v1/contact-lists` with `{"name": "Customers"}` (`list_id`). Then `POST /api/v1/contacts/import`, a `multipart/form-data` body with an `options` part (`application/json`):
   `{"attest_consent": true, "list_id": "<list_id>", "columns": {"email": "email", "consent_source": "consent"}}`
   and a `file` part (`text/csv`, filename `contacts.csv`): `email,consent` then two rows, using addresses your Papercut can show. Poll `GET /api/v1/contacts/imports/{id}` until done. **Check:** two contacts imported, none rejected.
6. **Media.** Small PNG: `POST /api/v1/media/uploads` with `{"filename": "bread.png", "size": <bytes>}`, then `PUT /api/v1/media/uploads/{id}` with `Content-Range: bytes 0-<last>/<size>` and the file as the body, then `POST /api/v1/media/uploads/{id}/complete`. (Or one call: `POST /api/v1/media` with the raw file and `Content-Length`.) **Check:** the asset is `ready` with a thumbnail (`GET /api/v1/media/{id}/thumbnail`).
7. **Draft with AI** (optional; needs the Ollama provider from the drafter checks, warmed): `POST /api/v1/ai/generate` with `{"mode": "post", "prompt": "Announce fresh bread", "targets": ["<mock_a>"], "provider_id": "<id>", "model": "qwen3.5:4b"}`. **Check:** `draft.post.text` and a `generation_id`. Without a provider, write the text yourself in step 8.
8. **Create the post.** `POST /api/v1/posts` with `{"text": "<text>", "disclose_ai_generated": true, "media": [{"media_id": "<media_id>", "alt_text": "a loaf"}], "targets": ["<mock_a>"], "generation_id": "<generation_id>"}` (drop `generation_id` and `disclose_ai_generated` if you skipped step 7). **Check:** 201 and an `ETag` header.
9. **Validate.** `POST /api/v1/posts/{id}/validate` with `{}`. **Check:** 200 and no violations in `targets`.
10. **Dry run.** `POST /api/v1/posts/{id}/publish` with `{"targets": ["<mock_a>"], "dry_run": true}` and `If-Match: <etag>`. **Check:** 201, a job whose deliveries are `simulated`, nothing live. The job's own status reads `published` (a job is `published` when every delivery went out or was simulated).
11. **Schedule a live publish.** `POST /api/v1/posts/{id}/publish` with `{"targets": ["<mock_a>"], "confirm": true, "at": "<ISO time about 1 hour ahead>"}` and `If-Match`. **Check:** 201 with a job (`job_id`) in a scheduled state. This needs live publishing on: `GET /api/v1/admin/settings/publishing` as an admin, and `PUT` it on if it is off.
12. **Calendar.** `GET /api/v1/calendar?from=<now>&to=<+2 hours>` (ISO times). **Check:** the job appears at its time.
13. **Reschedule.** `PATCH /api/v1/publish-jobs/{job_id}` with `{"at": "<now>"}` and `If-Match: <job etag>`. The answer is `{moved, failed}` (the delivery IDs moved, not the job); read the job with `GET /api/v1/publish-jobs/{job_id}` and poll it. **Check:** the delivery reaches `published` (the mock publishes at once).
14. **Notifications.** `GET /api/v1/notifications` and `GET /api/v1/notifications/unread-count`. **Check:** a notice for the published job.
15. **Email campaign.** `POST /api/v1/posts` with `{"text": "Fresh bread", "targets": ["<email_a>"]}`. Then `PUT /api/v1/posts/{id}/overrides/{email_a}` with `If-Match` and
    `{"patch": {"email": {"subject": "Fresh bread, {{first_name|friend}}", "language": "en-US", "blocks": [{"type": "text", "text": "Hello {{first_name|there}}, come early."}, {"type": "button", "label": "Order", "url": "https://bakery.example/order"}]}}, "options": {"lists": ["<list_id>"]}}`.
    Then `POST /api/v1/posts/{id}/publish` with `{"targets": ["<email_a>"], "confirm": true}` and the `If-Match` from the override answer. **Check:** 201, then in Papercut one message per contact, with the greeting and an unsubscribe link in each.
16. `GET /api/v1/publish-jobs`. **Check:** both jobs, newest first, with their states.

## Delegate

17. **Grant access (as the owner).** The delegate must have signed in to postit once first: lookup of an email with no user record answers `{"user": null}`. `GET /api/v1/users/lookup?email=<delegate's verified email>` for the delegate's `id`, then `POST /api/v1/delegations` with `{"delegate_id": "<id>", "level": "publish"}`.
18. **As the delegate**, send every request with `X-Postit-Act-As: <owner id>` (`GET /api/v1/me/workspaces` lists the workspaces; the owner's `owner.id` is the value). **Check:** `GET /api/v1/accounts` returns the owner's accounts.
19. **A campaign, a post, validate, schedule.**
    - `POST /api/v1/campaigns` with `{"name": "Spring"}` (`campaign_id`).
    - `POST /api/v1/posts` with `{"text": "Spring loaves", "campaign_id": "<id>", "targets": ["<mock_a>"]}`.
    - `POST /api/v1/posts/{id}/validate` with `{}`.
    - `POST /api/v1/posts/{id}/publish` with `{"targets": ["<mock_a>"], "confirm": true, "at": "<~1 hour ahead>"}` and `If-Match`.
    **Check:** the job's `created_by` is the delegate's id, not the owner's.
20. **Calendar and reschedule as the delegate.** `GET /api/v1/calendar?from=&to=`, then `PATCH /api/v1/publish-jobs/{job_id}` with `{"at": "<now>"}` and `If-Match`. **Check:** it publishes, and the owner's `GET /api/v1/publish-jobs` also shows the job.
21. **Email campaign as the delegate:** repeat step 15 with `X-Postit-Act-As`, using a different subject and a `Spring loaves are in.` body, and add `campaign_id` to the post. **Check:** Papercut gets the messages again; the post and job `created_by` are the delegate's.
22. **Limits.** With a delegate token and no `X-Postit-Act-As`, `GET /api/v1/posts` shows only the delegate's own posts. With `X-Postit-Act-As`, `POST /api/v1/delegations` answers `owner_only`. (Spot checks; stop after these.)

## What to report

For each numbered step: the status, and the **Check** value. Any `4xx` or `5xx` with its `code` and `detail`. The first step that surprises you is the interesting one; don't repair the data and carry on, tell me and I'll look.
