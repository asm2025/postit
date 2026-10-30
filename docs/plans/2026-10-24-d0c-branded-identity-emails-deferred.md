# D0c deferred items

## Browser sign-up and set-password forms not exercised by an agent

- **Issue:** the sign-up and set-password forms on `https://postit.local:44315`, reached from the branded identity emails, were not exercised in a browser by an agent. The test users were created through the Zitadel API instead (see `.superpowers/sdd/2026-10-24-d0c-branded-identity-emails/live-report.md`).
- **Why it cannot be fixed now:** agent safety rules forbid typing passwords or creating accounts on non-localhost hosts, and `postit.local` is not localhost.
- **Trigger:** the maintainer's one manual click-through (sign up, receive the verification email, set a password) before merge, or the G1 checklist in `docs/deployment.md`.
