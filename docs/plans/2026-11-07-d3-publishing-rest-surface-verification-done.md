# D3 publishing REST surface: human verification

Checks only a person can do. Agent work that could not be finished is in `2026-11-07-d3-publishing-rest-surface-deferred.md` (none was left).

## Drive the exit flow by hand

- **Item:** run the D3 exit flow against `cargo dev` through `https://postit.local` with an HTTP client such as Bruno or curl: as the owner, connect accounts, import contacts, upload an image, create (or generate) a post, validate it, dry-run it, schedule a live publish and reschedule it, send a campaign, and read the job history, the calendar, and the notifications; then repeat the post, validate, schedule, publish, and campaign steps as a `publish` delegate with `X-Postit-Act-As`.
- **Why an agent cannot:** agents do not sign in on `postit.local`, and the run needs a person's Zitadel session (a bearer token for the owner and for the delegate). The automated version, `backend/crates/server/tests/d3_flow.rs`, runs against the mock agents, a mock OAuth provider, a mock AI provider, and an in-process SMTP sink with test tokens.
- **Trigger:** before D4 starts.
